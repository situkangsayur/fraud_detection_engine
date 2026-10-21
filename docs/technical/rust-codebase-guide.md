# Rust Codebase Guide — "why it is structured this way"

> Document status: verified against commit `07ee765` on 2026-10-13. (Translated from the earlier Indonesian
> edition; content updated to the current code.)

This document is for developers used to building rule engines in **Java** with many abstractions (interfaces,
abstract classes, inheritance, a DI container) who are now entering the Rust codebase of this platform. It explains
**why** the structure and code look the way they do, and which software-engineering principles they follow, with
real files from the repo as examples.

> In short: the abstractions are still there, but the tools differ. In Rust, a **trait** replaces an interface,
> **enum + pattern matching** replaces a class hierarchy, a **crate** replaces a Maven module/layer, **`Result`**
> replaces exceptions, and **plain constructors** replace the DI container.

---

## 1. Workspace map

```
services/rust/                    ← Cargo workspace (≈ Maven multi-module / Gradle multi-project)
├── Cargo.toml                    ← dependency versions set ONCE (≈ dependencyManagement / BOM), lints, profiles
├── Dockerfile                    ← one image per service (--build-arg SERVICE=…), cargo-chef, Rust 1.96.1
└── crates/
    ├── platform/       lib   shared infrastructure: config, error→HTTP, auth, tenant tx, http client, audit,
    │                         PII hashing, pagination, date-range parsing, server, CLI subcommands
    ├── contracts/      lib   DTOs between services (EvaluateRequest, GraphMetrics, DecisionOut, field catalog)
    ├── rule-engine/    lib   PURE DOMAIN: DSL model, formula parser, evaluator, statistics, validation. No DB/HTTP.
    ├── core-api/       bin   orchestrator: tenants, projects, ingest, mapping, features, decisions, cases, analytics
    ├── rule-service/   bin   rules: CRUD + lifecycle + evaluate + backtest + proposals (uses rule-engine)
    └── graph-service/  bin   graph: entity resolution, BFS, metrics, components, export
```

| Java concept | Equivalent here |
|---|---|
| Maven parent POM + BOM | `services/rust/Cargo.toml` → `[workspace.dependencies]` |
| `*-common`, `*-api-model` modules | crates `platform`, `contracts` |
| Domain module that "must not import Spring" | crate `rule-engine` (no dependency on sqlx/axum at all) |
| Spring Boot app | `bin` crates (`core-api`, `rule-service`, `graph-service`) |

**Why is rule-engine its own crate?** In Rust a crate boundary is a **dependency boundary enforced by the
compiler**. In Java "the domain must not know the database" is only a convention, and sooner or later someone
imports `EntityManager` into the domain. Here `rule-engine/Cargo.toml` has no `sqlx` or `axum`, so the domain
**cannot** touch the DB. As a result:
* the rule engine is tested entirely in memory (80+ unit and integration tests plus property tests for the formula
  parser, in milliseconds);
* the same engine serves live evaluation, backtests, rule tests from the UI, and validation of LLM output;
* if rules ever need to run elsewhere (e.g. at the edge or in WASM), the crate can be taken as is.

---

## 2. Inside each service: Hexagonal (Ports & Adapters)

Every service has the same folder layout:

```
src/
├── domain/              pure business logic. No IO. Most unit tests live here.
├── app/ | application/  use cases (orchestration): call domain + ports
│                        (core-api uses `application/`, rule-service and graph-service use `app/`)
├── adapters/            port implementations: Postgres (sqlx), HTTP to other services, crypto
├── api/                 HTTP (axum): routing, DTOs, auth → calls use cases; OpenAPI (utoipa)
└── main.rs              wiring: read config, build pools/clients, assemble everything, run the server
```

Dependencies always point **inwards**: `api → app → domain`, and `adapters → (ports in) domain/app`. The domain
never imports an adapter.

This is the same Clean/Hexagonal Architecture you may know from Java (`domain`, `application`, `infrastructure`,
`web` packages). The difference is only **how the ports are written**.

### 2.1 Port = `trait` (instead of `interface`)

Real example: `crates/rule-engine/src/ports.rs`

```rust
#[async_trait]
pub trait DataProvider: Send + Sync {
    async fn velocity(&self, query: &VelocityQuery) -> Result<VelocityData, ProviderError>;
    async fn reference_lookup(&self, list: &str, key: &str) -> Result<RefLookup, ProviderError>;
    async fn graph_metric(&self, query: &GraphMetricQuery) -> Result<Option<f64>, ProviderError>;
}
```

Java equivalent:

```java
public interface DataProvider {
    VelocityData velocity(VelocityQuery q) throws ProviderException;
    ...
}
```

The implementation lives in the **service**, not in the engine: `rule-service/src/adapters/data_provider.rs` →
`impl DataProvider for PgDataProvider`, which compiles a `VelocityQuery` into parameterised SQL
(`adapters/velocity_sql.rs`). In tests the implementation is an in-memory mock.

Other ports with the same pattern:
* `rule-engine/src/ports.rs` → `Timer` (the pure engine enforces per-rule time budgets without depending on a
  runtime; rule-service implements it with `tokio::time::sleep`).
* `rule-engine/src/validate.rs` → `FieldCatalog` (field-path validation; rule-service backs it with
  `core.field_catalog`, cached 60 s).
* `core-api/src/application/ports.rs` → `RuleEngineClient`, `GraphClient`, `MlClient` (the orchestrator does not
  know whether it talks HTTP, a mock, or something in-process).
* `graph-service/src/domain/ports.rs` → `GraphStore` (BFS in the domain, batched queries in the Postgres adapter;
  tests use `domain/memory.rs`).
* `platform/src/auth.rs` → `ProjectDirectory` (how to find the tenant that owns a project).
* `platform/src/server.rs` → `ReadinessCheck` (what `/health/ready` checks per service).

### 2.2 Decorators without inheritance

Java usually: `class CachingDataProvider implements DataProvider { DataProvider delegate; ... }`.
In Rust: `rule-service/src/adapters/provider_cache.rs`

```rust
impl<P: DataProvider> DataProvider for CachedProvider<P> { ... }
```

`CachedProvider<P>` wraps **any** provider that implements the trait. It holds two caches: reference lookups shared
across requests for a few seconds (`REFERENCE_CACHE_TTL_SECS`), and velocity/graph queries memoised within one
evaluation only (single-flight, so identical queries from several rules run once). Because it is generic, the
compiler generates a specialised version (monomorphisation), so there is no virtual-call cost. The result is the
decorator pattern without a class hierarchy.

### 2.3 `dyn Trait` vs generics: when to use which

| Choice | Like in Java | Used when |
|---|---|---|
| `impl<P: DataProvider>` / `fn f<P: DataProvider>(p: &P)` | a bounded generic, but *specialised* by the compiler | hot path, performance matters (cache decorator) |
| `Arc<dyn RuleEngineClient>` | a reference to an interface (dynamic dispatch) | wiring in application state, swapping implementations at runtime / in tests |

---

## 3. Rule hierarchy → `enum` + `match` (instead of an abstract class)

In Java a rule engine usually looks like this:

```java
abstract class Rule { abstract Outcome evaluate(Context c); }
class SimpleRule extends Rule { ... }
class VelocityRule extends Rule { ... }
class CompositeRule extends VelocityRule { ... }
```

Here (`rule-engine/src/model.rs`):

```rust
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuleDefinition {
    Simple(SimpleRule),
    Velocity(Box<VelocitySpec>),
    Composite(Box<CompositeRule>),
    Reference(ReferenceRule),
    Graph(GraphRule),
}
```

and the evaluator (`rule-engine/src/eval/`) `match`es on every variant. Why this is better for a rule engine:

1. **Exhaustiveness check.** If we add `RuleDefinition::Sequence` tomorrow, the compiler flags *every* `match` that
   does not handle the new variant: evaluator, validator, backtest, and serialisers. In Java a forgotten subclass is
   only discovered at runtime.
2. **Data and behaviour are separate.** The rule model is plain serialisable data (JSON from the UI/LLM → `serde` →
   enum) and the behaviour lives in the evaluator. That fits rules that are stored in the DB and edited by users. No
   `instanceof`, visitor pattern or reflection is needed.
3. **Validation at the system boundary.** `#[serde(deny_unknown_fields)]` + a tagged enum reject malformed JSON (e.g.
   hallucinated LLM output) with a precise error path such as `definition.when.all[1].op`.

The evaluation result is an enum too: `Outcome::{Match, NoMatch, Trapped(reason)}` (`eval/mod.rs`). Tri-state
"trapped" is neither an exception nor `null`. It is an explicit value the caller **must** handle.

---

## 4. Error handling: `Result`, not exceptions

* Every fallible function returns `Result<T, E>`. The `?` operator propagates errors upwards, similar to `throws`,
  but it is **visible in the signature** and cannot be "forgotten in a catch".
* `platform/src/error.rs` defines `AppError` (`NotFound`, `Validation`, `BadRequest`, `Unauthorized`, `Forbidden`,
  `Conflict`, `Upstream`, `Unavailable`, …) and `impl IntoResponse for AppError`, which turns an error into an
  RFC 7807 `application/problem+json` response. This is the equivalent of Spring's `@ControllerAdvice`, but without
  magic: the conversion is an ordinary trait implementation.
* `impl From<sqlx::Error> for AppError`: unique violation → `Conflict`, row not found → `NotFound`, so a handler can
  simply write `repo.insert(...).await?`.
* Workspace lints: `unwrap_used` / `expect_used = "warn"` (tests may `#[allow]` them). Panics are not used for normal
  control flow, and release builds use `panic = "abort"`.

---

## 5. "Dependency injection" without a container

There is no Spring context. Wiring is explicit in `main.rs` / `state.rs`: build the `PgPool`, build HTTP clients,
wrap them in `Arc`, and put them into `AppState`. Axum handlers receive the state through the `State<AppState>`
extractor.

Why this is considered *best practice* in Rust:
* **No runtime magic.** A missing dependency is a compile error, not a `NoSuchBeanDefinitionException` at startup.
* **Easy testing.** Build an `AppState` with fake port implementations (e.g. `wiremock` for rule/graph/ml in the
  core-api tests).
* **Fast startup, small images.** The rule-service binary is roughly 54 MB in a distroless image and starts in
  milliseconds.

Configuration is a `clap` struct per service read from env vars (`platform/src/config.rs`, the equivalent of
`@ConfigurationProperties`); secrets are wrapped in `Secret` so `Debug` never prints them.

### Auth as an extractor (instead of a filter + annotation)

`platform/src/auth.rs` → `impl FromRequestParts for Caller`. Axum runs this code **before** the handler. If the token
is invalid, the handler is never called. In a handler:

```rust
async fn create_rule(caller: Caller, Path(pid): Path<ProjectId>, ...) -> AppResult<...> {
    let tenant = caller.require_project_role(pid, ProjectRole::Analyst).await?;
    ...
}
```

This is the equivalent of `@PreAuthorize("hasRole('ANALYST')")`, but as code you can step through in a debugger and
unit-test. `Caller` covers both end users (JWT) and services (`INTERNAL_API_TOKEN` + `X-Tenant-Id`,
`X-Project-Id`, `X-Actor`); `caller.require_service()` restricts internal routes such as `/v1/projects/{pid}/evaluate`.

---

## 6. Safe multi-tenancy: types + database

* **Newtype ids:** `TenantId`, `ProjectId`, `UserId` (not raw `Uuid`). The compiler rejects a `ProjectId` passed where
  a `TenantId` is expected. The classic Java bug `find(String id, String tenantId)` with swapped arguments cannot
  happen.
* **`TenantTx::begin(&pool, tenant)`** (`platform/src/db.rs`) starts a transaction and runs
  `set_config('app.tenant_id', …, true)`. All tenant queries go through it, so Postgres Row-Level Security is active.
  Even if a developer forgets `WHERE tenant_id = ...`, the database still returns no data of another tenant (second
  line of defence).
* **Statement timeouts:** the pool sets `DATABASE_STATEMENT_TIMEOUT_MS` (default 5 s) and an idle-in-transaction
  timeout, so a slow query fails fast instead of holding a connection. (Migration `0012` added an index after the
  event search hit this timeout on large projects.)

---

## 7. Concurrency & performance

* **tokio async:** one thread serves thousands of requests that are waiting on IO (DB, HTTP), similar to virtual
  threads in Java 21.
* Rules are evaluated **concurrently** (`futures::join_all` in `rule-engine/src/ruleset.rs`) with a **per-rule time
  budget** (`RULE_TIMEOUT_MS`, default 50 ms). A slow rule becomes `trapped: timeout` and does not block the
  decision. Backtests fan out with `buffer_unordered` (`BACKTEST_CONCURRENCY`).
* The core-api orchestrator calls graph metrics first, then supervised and unsupervised ML **in parallel**
  (`tokio::join!`), each with its own timeout. A failed engine is recorded in `degraded`, and a decision is still
  produced.
* No GC: stable latency (graph metrics p95 26 ms; warm rule evaluation ≈ 8–9 ms on a debug build).

---

## 8. Shared-database reads (a deliberate trade-off)

Services share one Postgres cluster but have **one schema per service** (`core`, `rules`, `graph`, `ml`, `llm`,
`ingest`). Each service **writes** only its own schema, except for three documented shared writes: every service
inserts into `core.audit_log`, rule-service and ml-service write maker–checker rows into `core.approvals`, and
ingest-service updates `core.data_sources.(cursor_state, inferred_schema)`. A few cross-schema **reads** are
deliberate and limited by `GRANT`:

| Reader | Reads | Migration | Why |
|---|---|---|---|
| rule-service | `core.events`, `event_features`, `customers`, `labels`, `event_labels`, `field_catalog`, `decisions` | `0007` | velocity aggregates (an HTTP hop per aggregate would break the latency budget), rule test/backtest context, validation |
| rule-service | `core.project_settings` | `0011` | ruleset backtests use the project's decision thresholds |
| ml-service | `core.events`, `event_features`, `customers`, `labels`, `event_labels` | `0007` | training data |
| graph-service | `ml.graph_communities`, `ml.graph_community_stats` | `0007` | `community_fraud_rate` metric |
| core-api | `rules.rules (id, tenant_id, project_id, status)`, `ml.models (…, kind, status)` | `0010` | project summary counters |
| ingest-service | `core.data_sources`, `core.data_source_mappings` | `0007` | import jobs, pollers |

If a separate database per service is ever needed, these are the points to replace with APIs or replication.

---

## 9. Software-engineering principles

| Principle | How it shows in the codebase |
|---|---|
| Single Responsibility | one service per engine; one module per concern (mapping, combine, features, psi, settings) |
| Open/Closed | new rule kind = enum variant + match (the compiler guides you); new ML algorithm = plugin without changing the platform |
| Liskov / Interface Segregation | small, specific ports (`DataProvider`, `Timer`, `GraphStore`, `MlClient`) |
| Dependency Inversion | the domain defines ports; adapters implement them |
| Fail-safe defaults | RLS fails closed, degraded mode, explicit `trapped`, platform admin without project data access |
| Immutability & auditability | rules/rulesets are immutable versions, append-only audit log, decisions store rule/model versions |
| Security by design | PII hashed with a per-tenant pepper (`platform/src/pii.rs`), least-privilege DB roles, secrets never logged (`Secret` type) |
| Testability | pure domain tested in memory; adapters tested against a real Postgres; HTTP tested with wiremock |
| 12-factor | config from env, JSON logs to stdout, stateless services, disposable containers |

---

## 10. Suggested reading order

1. [`architecture.md`](architecture.md) → the big picture.
2. `crates/rule-engine/src/model.rs` → the shape of a rule (JSON ⇄ enum).
3. `crates/rule-engine/src/eval/` → how rules are evaluated (Kleene logic, statistics).
4. `crates/rule-service/src/adapters/velocity_sql.rs` → from `VelocityQuery` to safe SQL.
5. `crates/core-api/src/application/pipeline.rs` → the end-to-end scoring pipeline.
6. `crates/core-api/src/domain/combine.rs` → how scores become a decision.
7. `crates/platform/src/{auth,db,error}.rs` → conventions used by every service.

## 11. Everyday commands

```bash
cd services/rust
cargo test -p rule-engine                       # engine unit + property tests
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
crates/rule-service/tests/run-integration.sh    # integration tests against a real Postgres (Docker)
crates/core-api/tests/run-integration.sh
eval "$(crates/graph-service/scripts/test-db.sh)" && cargo test -p graph-service
crates/graph-service/scripts/test-db.sh --stop
```
