# Panduan Developer

## 1. Peta repository

| Path | Isi | Bahasa/tooling |
|---|---|---|
| `services/rust/` | workspace: `platform`, `contracts`, `rule-engine` (lib) + `core-api`, `rule-service`, `graph-service` (bin) | Rust stable, cargo, clippy, rustfmt |
| `services/python/ml-service` | plugin algoritma, training, serving, clustering, Louvain | Python 3.12, uv, FastAPI, PyTorch CPU, scikit-learn |
| `services/python/llm-service` | RAG regulasi, chat + tools, analisis, proposal | Python 3.12, uv, FastAPI, httpx, opensearch-py |
| `services/python/ingest-service` | inferensi skema, saran mapping, import job, pull connector | Python 3.12, uv, pandas, pyarrow, SQLAlchemy |
| `web/` | UI + BFF | Nuxt 4, TypeScript, Nuxt UI, Pinia, ECharts, Cytoscape (Node ≥ 22.19) |
| `db/migrations/` | **satu-satunya** sumber DDL | SQL (sqlx migrator) |
| `tools/simulator/` | data sintetis + ground truth untuk evaluasi | Python 3.12, uv |
| `plugins/` | plugin ML eksternal (hot reload) | Python |

Baca dulu: [`architecture.md`](../technical/architecture.md) → [`rust-codebase-guide.md`](../technical/rust-codebase-guide.md)
→ kontrak yang relevan (`rule-dsl.md`, `api-contract.md`, …).

## 2. Aturan main

1. **Kontrak dulu.** Perubahan API/skema/DSL ditulis di `docs/technical/*` pada commit yang sama dengan kodenya.
2. **Skema hanya lewat migrasi baru** di `db/migrations/NNNN_nama.sql`. Migrasi lama tidak diedit, karena checksum
   sqlx akan gagal. Uji dengan `db/tests/test_migrations.sh`.
3. **Service hanya menulis ke schema miliknya.** Read lintas schema harus lewat GRANT eksplisit di migrasi dan
   didokumentasikan (lihat `rust-codebase-guide.md` §8).
4. **Semua query data tenant berjalan di transaksi ber-tenant:** `TenantTx::begin` (Rust) atau `tenant_session`
   (Python), dan tetap memfilter `project_id` secara eksplisit.
5. **Tidak ada secret atau PII di log.** Nomor kartu/rekening hanya disimpan sebagai hash.
6. **Maker–checker** tidak boleh dilewati oleh kode, termasuk oleh service token.
7. Sebelum push, semua pemeriksaan di bagian 3 harus hijau (CI menjalankan hal yang sama).
8. **Atribusi:** jangan menghapus header lisensi, `NOTICE`, atau `AUTHORS`. Kontributor baru menambahkan namanya
   di `AUTHORS`.

## 3. Perintah per bagian

### Rust

```bash
cd services/rust
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
crates/rule-service/tests/run-integration.sh      # Postgres sungguhan via Docker
crates/core-api/tests/run-integration.sh
crates/graph-service/scripts/test-db.sh           # lihat header script
```

### Python (setiap service dan simulator)

```bash
cd services/python/ml-service          # atau llm-service, ingest-service, tools/simulator
uv sync
uv run ruff check . && uv run ruff format --check .
uv run mypy src                        # ingest-service: app, simulator: simulator
uv run pytest -q
```

Test integrasi Python memakai Postgres sungguhan jika variabel `TEST_DATABASE_URL` / `TEST_ADMIN_DATABASE_URL`
di-set (lihat README tiap service). Jika tidak di-set, test tersebut di-skip.

### Web

```bash
cd web
npm ci
npm run dev:mock        # UI penuh tanpa backend (mock API)
NUXT_API_BASE_URL=http://127.0.0.1:8080 npm run dev -- --port 3090   # terhadap backend sungguhan
npm run lint && npm run typecheck && npm test && npm run build
```

### Skema database

```bash
db/tests/test_migrations.sh       # Postgres sekali pakai: role, semua migrasi, smoke test RLS
```

## 4. Resep umum

### Menambah jenis rule atau operator

1. `rule-engine/src/model.rs`: tambahkan varian enum. Compiler menunjukkan setiap `match` yang harus diperbarui
   (evaluator, validator).
2. Jika butuh data baru, tambahkan method pada port `DataProvider` (`ports.rs`), lalu implementasikan di
   `rule-service/src/adapters/data_provider.rs`.
3. Test di `rule-engine/tests/`, dokumentasi di `rule-dsl.md`, editor di `web/app/components/rules/`.

### Menambah fitur (feature catalog)

1. Hitung fitur di `core-api/src/adapters/features_sql.rs` (+ `domain/features.rs`).
2. Daftarkan di `contracts/src/catalog.rs` (`BUILTIN_FIELDS`) dan `docs/technical/feature-catalog.md`.
3. Mengubah arti fitur yang sudah ada berarti menaikkan `FEATURE_SET_VERSION`.

### Menambah algoritma ML (plugin)

Buat file di `plugins/` yang mengekspor `PLUGINS = [KelasPlugin]` sesuai kontrak di
[`ml-plugins.md`](../technical/ml-plugins.md) (contoh: `plugins/example_knn_anomaly.py`). Lalu jalankan
`POST /api/v1/ml/algorithms/reload` (platform admin). Plugin divalidasi dan di-smoke-test otomatis.

### Menambah template stage

Tambahkan/ubah file JSON di `services/rust/crates/rule-service/templates/` (satu file per stage) dan daftarkan di `src/domain/templates.rs`.
Unit test template memvalidasi semua rule dengan validator sungguhan.

### Menambah endpoint

1. Kontrak di `api-contract.md`.
2. Handler di `api/` service terkait (auth lewat extractor `Caller`), use case di `app/`/`application/`.
3. Route gateway di `deploy/gateway/dynamic.yml` jika prefix path-nya baru.
4. Tipe di `web/shared/types/api.ts` (model rule DSL di `web/shared/rules/dsl.ts`) dan fixture mock.

## 5. Debugging

* Setiap request membawa `x-request-id` di semua service, sehingga bisa dicari di log JSON:
  `docker compose logs core-api rule-service | grep <request-id>`.
* Metrics Prometheus: `/metrics` di setiap service (internal network).
* Keputusan aneh? Buka detail event di UI. Jejak rule, skor per engine, dan `degraded` menjelaskan asal skornya.
* Log lebih rinci per service: `RUST_LOG=rule_service=debug,platform=debug` (Rust) atau `LOG_LEVEL=debug` (Python).

## 6. Evaluasi kualitas deteksi

Lihat [`evaluation.md`](../technical/evaluation.md): simulator dengan `--run-tag` dan `--truth-out` menghasilkan
lalu lintas baru beserta ground truth, lalu hasilnya di-join dengan `core.decisions`. Jalankan ulang evaluasi setelah
mengubah logika scoring.
