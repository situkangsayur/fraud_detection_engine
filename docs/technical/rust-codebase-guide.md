# Panduan Codebase Rust — "kenapa strukturnya begini"

Dokumen ini untuk developer yang terbiasa membangun rule engine di **Java** dengan banyak abstraksi (interface,
abstract class, inheritance, DI container), lalu masuk ke codebase Rust platform ini. Isinya menjelaskan **kenapa**
struktur dan code-nya dibuat seperti ini, dan prinsip software engineering apa yang dipegang, dengan contoh file
nyata di repo.

> Ringkas: abstraksinya tetap ada, tapi alatnya berbeda. Di Rust, **trait** menggantikan interface, **enum + pattern
> matching** menggantikan hierarki class, **crate** menggantikan modul/layer Maven, **`Result`** menggantikan
> exception, dan **constructor biasa** menggantikan DI container.

---

## 1. Peta workspace

```
services/rust/                    ← Cargo workspace (≈ Maven multi-module / Gradle multi-project)
├── Cargo.toml                    ← versi dependency ditetapkan SEKALI (≈ dependencyManagement / BOM)
└── crates/
    ├── platform/       lib   infrastruktur bersama: config, error→HTTP, auth, tenant-tx, http client, audit, server
    ├── contracts/      lib   DTO antar service (EvaluateRequest, GraphMetrics, DecisionOut, field catalog)
    ├── rule-engine/    lib   DOMAIN MURNI: model DSL, parser formula, evaluator, statistik. Tanpa DB/HTTP.
    ├── core-api/       bin   orkestrator: tenant, project, ingest, mapping, fitur, keputusan, case
    ├── rule-service/   bin   service rule: CRUD + lifecycle + evaluate + backtest (memakai rule-engine)
    └── graph-service/  bin   service graph: entity resolution, BFS, metrik, komponen
```

| Konsep Java | Padanan di sini |
|---|---|
| Maven parent POM + BOM | `services/rust/Cargo.toml` → `[workspace.dependencies]` |
| Modul `*-common`, `*-api-model` | crate `platform`, `contracts` |
| Modul domain yang "tidak boleh import Spring" | crate `rule-engine` (tidak bergantung ke sqlx/axum sama sekali) |
| Spring Boot app | crate `bin` (`core-api`, `rule-service`, `graph-service`) |

**Kenapa rule-engine dipisah jadi crate sendiri?** Karena di Rust batas crate adalah **batas dependency yang
dipaksakan compiler**. Di Java, aturan "domain tidak boleh tahu database" hanya konvensi, dan cepat atau lambat
seseorang meng-import `EntityManager` ke domain. Di sini `rule-engine/Cargo.toml` tidak punya `sqlx` atau `axum`,
jadi **mustahil** secara teknis domain menyentuh DB. Efeknya:
* rule engine bisa dites 100% di memori (81 test, milidetik),
* rule engine yang sama dipakai untuk evaluate live, backtest, test rule dari UI, dan validasi output LLM,
* kalau suatu hari rule dieksekusi di tempat lain (mis. edge/WASM), crate-nya bisa dibawa apa adanya.

---

## 2. Arsitektur di dalam setiap service: Hexagonal (Ports & Adapters)

Setiap service punya susunan folder yang sama:

```
src/
├── domain/        logika bisnis murni. Tanpa IO. Paling banyak unit test.
├── app/ | application/   use case (orkestrasi): memanggil domain + port
├── adapters/      implementasi port: Postgres (sqlx), HTTP ke service lain, crypto
├── api/           HTTP (axum): routing, DTO, auth → memanggil use case
└── main.rs        wiring: baca config, buat pool/klien, rangkai semuanya, jalankan server
```

Arah dependency selalu **ke dalam**: `api → app → domain`, dan `adapters → (port di) domain/app`. Domain tidak pernah
mengimpor adapter.

Ini sama dengan Clean/Hexagonal Architecture yang mungkin Anda pakai di Java (package `domain`, `application`,
`infrastructure`, `web`). Bedanya hanya pada **cara menulis port-nya**.

### 2.1 Port = `trait` (pengganti `interface`)

Contoh nyata: `crates/rule-engine/src/ports.rs`

```rust
#[async_trait]
pub trait DataProvider: Send + Sync {
    async fn velocity(&self, q: &VelocityQuery) -> Result<VelocityData, ProviderError>;
    async fn reference_lookup(&self, list: &str, key: &str) -> Result<RefLookup, ProviderError>;
    async fn graph_metric(&self, q: &GraphMetricQuery) -> Result<Option<f64>, ProviderError>;
}
```

Padanan Java:

```java
public interface DataProvider {
    VelocityData velocity(VelocityQuery q) throws ProviderException;
    ...
}
```

Implementasinya ada di **service**, bukan di engine: `rule-service/src/adapters/data_provider.rs` →
`impl DataProvider for PgDataProvider`, yang meng-compile `VelocityQuery` menjadi SQL berparameter. Di test,
implementasinya berupa mock in-memory.

Port lain dengan pola yang sama:
* `core-api/src/application/ports.rs` → `RuleEngineClient`, `GraphClient`, `MlClient` (orkestrator tidak tahu apakah
  lawan bicaranya HTTP, mock, atau in-process).
* `graph-service/src/domain/ports.rs` → `GraphStore` (BFS di domain, query batch di adapter Postgres; test memakai
  `domain/memory.rs`).
* `platform/src/auth.rs` → `ProjectDirectory` (cara mencari tenant pemilik project).

### 2.2 Decorator tanpa inheritance

Java biasanya: `class CachingDataProvider extends/implements DataProvider { DataProvider delegate; ... }`.
Di Rust: `rule-service/src/adapters/provider_cache.rs`

```rust
impl<P: DataProvider> DataProvider for CachedProvider<P> { ... }
```

`CachedProvider<P>` membungkus provider **apa pun** yang memenuhi trait. Karena ini generic, compiler membuat versi
khusus (monomorphization), jadi tidak ada biaya virtual call. Hasilnya decorator pattern tanpa class hierarchy.

### 2.3 `dyn Trait` vs generic: kapan pakai yang mana

| Pilihan | Seperti di Java | Dipakai saat |
|---|---|---|
| `impl<P: DataProvider>` / `fn f<P: DataProvider>(p: &P)` | generic dengan bound, tapi di-*specialize* compiler | hot path, butuh performa (decorator cache) |
| `Arc<dyn RuleEngineClient>` | referensi ke interface (dynamic dispatch) | wiring di state aplikasi, ingin bisa ganti implementasi saat runtime/test |

---

## 3. Hierarki rule → `enum` + `match` (pengganti abstract class)

Di Java, rule engine biasanya berbentuk seperti ini:

```java
abstract class Rule { abstract Outcome evaluate(Context c); }
class SimpleRule extends Rule { ... }
class VelocityRule extends Rule { ... }
class CompositeRule extends VelocityRule { ... }
```

Di sini (`rule-engine/src/model.rs`):

```rust
pub enum RuleDefinition {
    Simple(SimpleRule),
    Velocity(Box<VelocitySpec>),
    Composite(Box<CompositeRule>),
    Reference(ReferenceRule),
    Graph(GraphRule),
}
```

dan evaluator melakukan `match` pada setiap varian. Kenapa ini lebih baik untuk rule engine:

1. **Exhaustiveness check.** Kalau besok kita menambah `RuleDefinition::Sequence`, compiler menandai *setiap* `match`
   yang belum menangani varian baru: evaluator, validator, backtest, dan UI serializer di Rust. Di Java, subclass
   baru yang lupa di-handle baru ketahuan saat runtime.
2. **Data dan perilaku terpisah.** Model rule adalah data murni yang bisa di-serialize (JSON dari UI/LLM →
   `serde` → enum), sedangkan perilaku ada di evaluator. Ini cocok untuk rule yang disimpan di DB dan diedit
   pengguna. Tidak perlu `instanceof`, visitor pattern, atau reflection.
3. **Validasi di batas sistem.** `#[serde(deny_unknown_fields)]` + tagged enum membuat JSON salah bentuk (mis. output
   LLM yang berhalusinasi) ditolak dengan path error yang tepat, seperti `definition.when.all[1].op`.

Hasil evaluasi juga berupa enum: `Outcome::{Match, NoMatch, Trapped(reason)}` (`eval/mod.rs`). Tri-state "trapped"
bukan exception dan bukan `null`. Ia adalah nilai eksplisit yang **wajib** ditangani oleh pemanggil.

---

## 4. Error handling: `Result` bukan exception

* Setiap fungsi yang bisa gagal mengembalikan `Result<T, E>`. Operator `?` meneruskan error ke atas, mirip `throws`,
  tapi **terlihat di signature** dan tidak bisa "lupa di-catch".
* `platform/src/error.rs` mendefinisikan `AppError` (NotFound, Validation, Forbidden, Conflict, Upstream, …) dan
  `impl IntoResponse for AppError`, yang mengubah error menjadi response RFC 7807 `application/problem+json`. Ini
  padanan `@ControllerAdvice` di Spring, tapi tanpa magic: konversinya berupa implementasi trait biasa.
* `impl From<sqlx::Error> for AppError`: unique violation → `Conflict`, row not found → `NotFound`, jadi handler
  cukup menulis `repo.insert(...).await?`.
* Lint workspace: `unwrap_used`/`expect_used = warn`. Panic tidak dipakai untuk alur normal, dan `panic = "abort"`
  di release.

---

## 5. "Dependency injection" tanpa container

Tidak ada Spring context. Wiring dilakukan eksplisit di `main.rs`/`state.rs`: buat `PgPool`, buat klien HTTP, bungkus
dengan `Arc`, dan masukkan ke `AppState`. Handler axum menerima state lewat extractor `State<AppState>`.

Kenapa ini dianggap *best practice* di Rust:
* **Tidak ada magic runtime.** Kalau ada dependency yang kurang, compile gagal, bukan `NoSuchBeanDefinitionException`
  saat startup.
* **Test mudah.** Buat `AppState` dengan implementasi port palsu (mis. wiremock untuk rule/graph/ml di test
  core-api).
* **Startup cepat, image kecil.** Binary rule-service ±54 MB distroless, start dalam milidetik.

### Auth sebagai extractor (pengganti filter + annotation)

`platform/src/auth.rs` → `impl FromRequestParts for Caller`. Axum menjalankan kode ini **sebelum** handler. Kalau
token tidak valid, handler tidak pernah dipanggil. Di handler:

```rust
async fn create_rule(caller: Caller, Path(pid): Path<ProjectId>, ...) -> AppResult<...> {
    let tenant = caller.require_project_role(pid, ProjectRole::Analyst).await?;
    ...
}
```

Ini padanan `@PreAuthorize("hasRole('ANALYST')")`, tapi berupa kode yang bisa di-step debugger dan dites unit.

---

## 6. Multi-tenant yang aman: tipe + database

* **Newtype id:** `TenantId`, `ProjectId`, `UserId` (bukan `Uuid` mentah). Compiler menolak kalau `ProjectId`
  tertukar dengan `TenantId`. Kesalahan klasik Java `find(String id, String tenantId)` dengan argumen tertukar tidak
  bisa terjadi.
* **`TenantTx::begin(&pool, tenant)`** (`platform/src/db.rs`) memulai transaksi dan menjalankan
  `set_config('app.tenant_id', …, true)`. Semua query tenant lewat sini, sehingga Row-Level Security di Postgres
  aktif. Walaupun developer lupa menulis `WHERE tenant_id = ...`, database tetap tidak mengembalikan data tenant lain
  (lapisan pertahanan kedua).

---

## 7. Konkurensi & performa

* **tokio async**: satu thread melayani ribuan request yang sedang menunggu IO (DB, HTTP), mirip virtual threads di
  Java 21.
* Evaluasi rule berjalan **paralel per rule** (`futures::join_all`) dengan **budget waktu per rule**. Rule yang
  lambat menjadi `trapped: timeout`, tidak memblokir keputusan.
* Orkestrator core-api menjalankan panggilan ML paralel (`tokio::join!`) dengan timeout per engine. Engine yang gagal
  dicatat di `degraded`, dan keputusan tetap keluar.
* Tanpa GC: latensi stabil (graph metrics p95 26 ms; evaluate rule warm ±8–9 ms pada build debug).

---

## 8. Shared-database reads (trade-off yang disengaja)

Service berbagi satu cluster Postgres tetapi **schema per service** (`core`, `rules`, `graph`, `ml`, `llm`,
`ingest`). Tiap service hanya **menulis** schema miliknya. Ada beberapa **read** lintas schema yang disengaja dan
dibatasi oleh `GRANT`:
* rule-service membaca `core.events` untuk agregasi velocity (hop HTTP per agregasi akan melanggar budget latensi),
* core-api membaca kolom `status` di `rules.rules` / `ml.models` untuk ringkasan project.

Semua read tersebut tercantum di `db/migrations/0007…0011` dan di dokumen ini. Kalau suatu saat perlu database
terpisah per service, ini titik-titik yang harus diganti dengan API/replikasi.

---

## 9. Prinsip software engineering yang dipegang

| Prinsip | Wujudnya di codebase |
|---|---|
| Single Responsibility | satu service per engine; satu modul per concern (mapping, combine, features, psi) |
| Open/Closed | jenis rule baru = varian enum + match (compiler memandu); algoritma ML baru = plugin tanpa ubah platform |
| Liskov / Interface Segregation | port kecil & spesifik (`DataProvider`, `GraphStore`, `MlClient`) |
| Dependency Inversion | domain mendefinisikan port; adapter mengimplementasikan |
| Fail-safe defaults | RLS fail-closed, degraded mode, `trapped` eksplisit, platform admin tanpa akses data project |
| Immutability & auditability | rule/ruleset versioned immutable, audit log append-only, keputusan menyimpan versi rule/model |
| Security by design | PII di-hash dengan pepper per tenant, least-privilege DB role, secret tidak ter-log (`Secret` type) |
| Testability | domain murni diuji di memori; adapter diuji dengan Postgres asli; HTTP diuji dengan wiremock |
| 12-factor | config dari env, log JSON ke stdout, stateless service, disposable container |

---

## 10. Cara membaca codebase (urutan yang disarankan)

1. `docs/technical/architecture.md` → gambaran besar.
2. `crates/rule-engine/src/model.rs` → bentuk rule (JSON ⇄ enum).
3. `crates/rule-engine/src/eval/` → cara rule dievaluasi (Kleene logic, statistik).
4. `crates/rule-service/src/adapters/velocity_sql.rs` → dari `VelocityQuery` ke SQL aman.
5. `crates/core-api/src/application/pipeline.rs` → pipeline scoring end-to-end.
6. `crates/core-api/src/domain/combine.rs` → cara skor digabung menjadi keputusan.
7. `crates/platform/src/{auth,db,error}.rs` → konvensi yang dipakai semua service.

## 11. Perintah sehari-hari

```bash
cd services/rust
cargo test -p rule-engine                       # unit test engine
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
crates/rule-service/tests/run-integration.sh    # integration test dengan Postgres asli (Docker)
crates/core-api/tests/run-integration.sh
crates/graph-service/scripts/test-db.sh up  # lalu cargo test -p graph-service (lihat header script)
```
