# Architecture — Fraud Detection Platform v2

Status: **authoritative design contract**. Services implement against this document and
[`multi-tenancy.md`](multi-tenancy.md), [`rule-dsl.md`](rule-dsl.md), [`api-contract.md`](api-contract.md),
[`feature-catalog.md`](feature-catalog.md), [`data-sources.md`](data-sources.md), [`ml-plugins.md`](ml-plugins.md)
and the SQL in `db/migrations/`. When code and docs disagree, fix one of them in the same change.

## 1. Goals

A **multi-tenant** fraud platform. One tenant (company) owns many **projects**, one per protection point with its own
behaviour. For example, a marketplace might run:

| Project stage | Example | Typical decision point |
|---|---|---|
| `pre_payment` | checkout protection | before payment authorisation (approve / review / decline) |
| `post_payment` | settlement, chargeback prevention | after payment, before fulfilment/shipment |
| `returns` | refund / return abuse | on return or refund request |
| `promo` | voucher / cashback abuse | on promo redemption and cashback payout |
| `account_security` | login / ATO | login, credential change |
| `payout` | seller or wallet withdrawals | before payout |
| `custom` | anything else | |

Each project has its own data sources and schema mapping, customers and events, **graph data set**, rulesets,
reference lists (optionally shared tenant-wide), ML algorithm choice and models, LLM configuration, and the set of
regulations/policies (vector DB) it follows.

Fraud typologies covered:

| Code | Typology | Typical signals |
|---|---|---|
| `carding` | stolen card / card testing | many small auths, many cards per device, BIN country ≠ IP country, card shared across customers |
| `account_takeover` | e-commerce ATO | new device + recent credential change + address change + high value |
| `bank_account_takeover` | stolen bank / e-wallet account | new device, new beneficiary, balance drain, night activity |
| `system_breach` | hacked banking/API system | burst across many accounts from one API client/IP, off-hours, abnormal volume |
| `promo_abuse` | policy abuse (voucher/cashback/discount), with transactions that are technically legit | multi-accounts linked by device/address/phone variants, high discount ratio, new accounts |
| `refund_abuse` | return/refund abuse | serial returns, "item not received" claims, returns shortly after delivery, linked accounts |
| `money_mule` | mule accounts | fan-in/fan-out transfers, graph proximity to fraud |

Five engines produce a decision: **Rules**, **Supervised ML**, **Unsupervised ML**, **Graph**, and the **LLM assistant**,
which only advises humans.

Non-functional targets:
* p95 scoring latency < 150 ms (without the LLM).
* Every decision is explainable.
* Every config change is versioned, goes through maker–checker approval, and is audited.
* Tenant isolation is enforced in the database (RLS).
* The whole stack runs with `docker compose up`.

## 2. Services

Every engine is its own backend service. Only the **gateway** is exposed.

```
                           ┌───────────────────────────┐
 Analyst ─▶ web (Nuxt 4) ─▶│ gateway (Traefik)  :80    │◀─ client systems (ingest, API keys)
           :3000 BFF       └──┬───────┬──────┬──────┬──┘
     path-based routing       │       │      │      │
  ┌───────────────────────────┘       │      │      └───────────────────────────┐
  ▼                                   ▼      ▼                                  ▼
┌──────────────────────┐ ┌──────────────────┐ ┌───────────────────┐ ┌─────────────────────┐
│ core-api  (Rust)     │ │ rule-service     │ │ graph-service     │ │ ml-service (Python) │
│ tenants, projects,   │ │ (Rust)           │ │ (Rust)            │ │ algorithm PLUGINS,  │
│ users, data sources, │ │ rules, rulesets, │ │ entity resolution,│ │ train, predict,     │
│ ingest ORCHESTRATOR, │ │ ref lists, DSL,  │ │ links, similarity,│ │ cluster, anomaly,   │
│ features, decisions, │ │ evaluate, back-  │ │ metrics, paths,   │ │ Louvain communities │
│ cases, labels,       │ │ test, proposals  │ │ components        │ └─────────────────────┘
│ analytics, audit     │ └──────────────────┘ └───────────────────┘ ┌─────────────────────┐
└──────────────────────┘                                            │ llm-service (Python)│
  ▲ internal calls: core-api ─▶ graph ─▶ ml ─▶ rule (evaluate)      │ RAG, chat, analysis,│
                                                                    │ recommendations     │
┌──────────────────────┐   ┌──────────────┐  ┌──────────────┐       └──────────┬──────────┘
│ ingest-service (Py)  │   │ PostgreSQL 16│  │ OpenSearch 2 │◀─────────────────┤
│ schema inference,    │   │ schema/service│ │ vectors per  │       ┌──────────▼──────────┐
│ file/SQL connectors  │   │ + RLS        │  │ tenant       │       │ Ollama              │
└──────────────────────┘   └──────────────┘  └──────────────┘       └─────────────────────┘
```

| Service | Tech | DB schema owned | Responsibilities |
|---|---|---|---|
| `gateway` | Traefik v3 | — | TLS termination (prod), path routing, rate limit, request-id, body size limits |
| `web` | Nuxt 4, TS, Nuxt UI, Pinia, ECharts, Cytoscape.js | — | UI with a BFF: server routes keep the JWT in an httpOnly cookie and forward `/api/**` to the gateway |
| `core-api` | Rust, axum, sqlx | `core` | auth/JWT, tenants, projects, members, settings, data sources + mapping engine, **scoring orchestration**, customers, events, features, decisions, cases, labels, analytics, audit |
| `rule-service` | Rust, axum, sqlx, `rule-engine` crate | `rules` | rules, versions, rulesets, reference lists, formula eval, validation, **evaluate**, backtest, rule stats, proposals + maker–checker, stage templates |
| `graph-service` | Rust, axum, sqlx | `graph` | entity extraction/normalisation, links, similarity links, supernode handling, graph metrics, neighbourhood, fraud proximity, components, export |
| `ml-service` | Python 3.12, FastAPI, PyTorch CPU, scikit-learn, networkx | `ml` | **algorithm plugin registry** (hot-loadable), training jobs per project, model registry, predict/score, clusters, projections, graph communities |
| `llm-service` | Python 3.12, FastAPI, httpx, opensearch-py | `llm` | regulation/policy library (tenant) + project attachment, chunk/embed/index, change diff, RAG chat with tools, analyses, rule recommendations → proposals |
| `ingest-service` | Python 3.12, FastAPI, pandas, pyarrow, SQLAlchemy | `ingest` | schema inference, mapping suggestions, file/SQL import jobs, pull connectors |
| `postgres` | PostgreSQL 16 | all | one cluster, **one schema per service**, RLS on tenant data |
| `opensearch` | OpenSearch 2.x k-NN | — | `reg-chunks-<tenant_id>` index per tenant |
| `ollama` | Ollama | — | chat model + embedding model |
| `migrate` | core-api binary `migrate` subcommand | owner of all schemas | runs `db/migrations/*.sql` once, then exits; other services wait for it |

### 2.1 Why separate services

* **Independent scaling.** `rule-service` and `graph-service` are on the hot path and scale horizontally. ML training
  can hog CPU without hurting scoring latency. LLM calls are slow and bursty.
* **Independent release and failure isolation.** The orchestrator degrades gracefully when ML, graph or LLM is down
  (see §3). Rules and core are the minimum viable path.
* **Right language per job.** Rust for the latency-critical, safety-critical engines (rules, graph, orchestration);
  Python for the ML/LLM ecosystem.
* **Ownership boundaries** match the schemas. A service writes **only** its own schema. Cross-service **reads** are
  limited to documented grants (e.g. rule-service reads `core.events` for velocity aggregates, because a network hop per
  aggregate would break the latency budget). This is a deliberate, documented trade-off; see
  `docs/technical/rust-codebase-guide.md` §"shared-database reads".

### 2.2 Rust workspace (`services/rust`)

```
services/rust/
  Cargo.toml                  # [workspace] — shared dependency versions, lints, profiles
  crates/
    platform/                 # shared infrastructure: config, telemetry, problem+json errors, JWT/INT auth
                              # extractors, tenant context (RLS), db pool, pagination, http client, audit client
    contracts/                # DTOs shared between Rust services (EvaluateRequest, GraphMetrics, …)
    rule-engine/              # PURE domain library: DSL model, formula parser, evaluator, statistics.
                              # No IO: data access goes through traits (ports) implemented by rule-service
    core-api/                 # bin (+ `migrate` and `healthcheck` subcommands)
    rule-service/             # bin
    graph-service/            # bin
```

## 3. Scoring pipeline (core-api orchestrator)

Entry points: `POST /api/v1/projects/{pid}/events` (canonical, JWT or INT) and `POST /api/v1/ingest/{source_slug}`
(raw record, data-source API key), plus the batch variants.

```
 1 resolve tenant/project/source ─▶ 2 apply mapping (canonical event + customer + label) ─▶ 3 upsert customer
 ─▶ 4 persist event ─▶ 5 graph-service POST /v1/projects/{pid}/links (entity resolution, returns entity ids)
 ─▶ 6 compute features v1 (SQL aggregates in core) ─▶ persist event_features
 ─▶ 7 parallel: graph-service metrics │ ml-service supervised predict │ ml-service unsupervised score
 ─▶ 8 rule-service POST /v1/projects/{pid}/evaluate  (context = event+source+customer+features+ml+graph)
 ─▶ 9 combine → decision ─▶ 10 persist decision (+ case) ─▶ 11 respond
```

* **Timeouts:** graph 100 ms, ml 200 ms, rules 300 ms (project settings `timeouts`).
* **Degraded mode:** if graph or ML fails, the engine is dropped from the combination and recorded in
  `decision.degraded`. If **rule-service** fails, the fallback is `settings.rules_unavailable_decision`
  (default `review`), `degraded` includes `rules`, and the event is flagged for re-scoring.
  **Ingest never loses an event:** the event is persisted before any engine call.
* `load_only` events (historical backfill) stop after step 6.
* `POST /api/v1/projects/{pid}/score/simulate` runs steps 2 and 6–9 without persisting anything.

### 3.1 Combination

Each engine produces a score in `[0,100]`:

| Engine | Score |
|---|---|
| rules | max over **active** rulesets of the ruleset aggregate (rule-dsl §7) |
| supervised | `fraud_probability × 100` (active supervised model of the project) |
| unsupervised | `anomaly_score × 100` (active unsupervised model) |
| graph | `graph.fraud_distance_scores[distance_to_fraud]` (default `{1:90,2:70,3:40}`), maxed with a shared-fraud-entity score |

Only **available** engines take part; failed or model-less engines are dropped. Weights come from project setting
`engine_weights` (default rules 0.45, supervised 0.30, unsupervised 0.10, graph 0.15). The combination method is the
project setting `engine_combination`:

| Method | Formula | Behaviour |
|---|---|---|
| `noisy_or` (default) | `100·(1 − Π(1 − sᵢ/100)^(wᵢ/w_max))` | independent evidence accumulates; an engine with score 0 multiplies by 1, so it never dilutes another engine's strong signal; a single engine at full weight passes through unchanged |
| `weighted_average` | `Σ wᵢ·sᵢ / Σ wᵢ` | calibrated blend; quiet engines pull the score down |

The default was chosen after the end-to-end simulation. With the weighted average, a strong rule hit (e.g. 60) in a
project without an ML model was diluted to 0.75·60 = 45 by a quiet graph engine, and fell below the review threshold.
Reason contributions are an attribution of `final_score`: engine shares sum to the final score, and a rule's share
within the rules engine is proportional to its rule-level contribution.

Actions override the thresholds with this precedence: `force_decline` > `force_approve` > `force_review` > thresholds
(`decision_thresholds`, default review ≥ 50, decline ≥ 80). A trapped rule with `on_trapped="review"` counts as
`force_review`. Review and decline decisions open a case (one open case per customer, and later events attach to it).
Shadow rules/rulesets are evaluated and traced but never affect the score.

`decision.reasons`: up to 8 `{code, engine, contribution, message}` sorted by contribution.

## 4. Governance

* **Maker–checker:** rules, rulesets, model activation, mapping activation and LLM proposals follow
  `draft → pending_approval → active|shadow → retired`. The approver must differ from the submitter and hold the
  approver role in that project (enforced in the service and by a DB CHECK on `approvals`).
* Rules **and rulesets** are **versioned and immutable** (`rules.rule_versions`, `rules.ruleset_versions`; a
  ruleset version snapshots its config and membership). **The approval ledger (`core.approvals`) decides what
  serves**, not the draft rows. Editing a live rule or ruleset creates a new draft version while the last approved
  version keeps serving. A version approved as `shadow` runs next to the live one (champion/challenger).
  Rule metadata (name, typologies, event_types) is versioned inside `definition._envelope`. Decisions record
  `rule_id@version` and model versions.
* **Audit:** `core.audit_log` is append-only (a trigger blocks UPDATE/DELETE). Every service writes through
  `POST /internal/audit` on core-api, or directly in core.
* **The LLM never activates anything.** It creates `pending` proposals. A human approval creates a **shadow** rule, and
  a second approval promotes it to `active`.

## 5. Security

* **Tenant isolation:** see `multi-tenancy.md`. RLS is on every tenant table and fails closed when no tenant is set.
* **AuthN:** core-api issues JWT access tokens (HS256 by default, 60 min) plus rotating refresh tokens (httpOnly
  cookie via the web BFF). All services verify JWTs themselves with the shared secret. Claims:
  `sub, tid (tenant), trole (tenant_admin|member), padmin (platform admin bool), prj {project_id: role}, exp, iat, jti`.
* **AuthZ:** project role in `prj` claim: `project_admin > approver > analyst > viewer`. A tenant admin is implicitly
  `project_admin` on every project of the tenant.
* **Service-to-service:** `Authorization: Bearer ${INTERNAL_API_TOKEN}` plus `X-Tenant-Id`, `X-Project-Id`, and
  `X-Actor` (user id, for audit) headers. Only accepted on internal networks and routes. mTLS is on the production
  backlog.
* **PII:** card and account numbers are hashed with an HMAC plus a per-tenant pepper derived as
  `HMAC(PII_PEPPER, tenant_id)`, so the same card links within a tenant but never across tenants. Only BIN and last4
  are kept. Emails and phones are normalised for linking and masked for the viewer role.
* **Plugins** run arbitrary Python code. Only the platform admin can install them (file volume plus reload; see
  `ml-plugins.md`).
* **LLM:** regulation text is untrusted data (delimited, never treated as instructions). Tools are allow-listed.
  The only write tool creates a pending proposal.

## 6. Cross-cutting conventions

* **API:** JSON, snake_case, UUIDs, RFC 3339 UTC. Lists return `{items,total,page,page_size}` (page ≥ 1,
  page_size ≤ 200, default 50). Errors use RFC 7807 problem+json `{type,title,status,detail,errors?}`.
  `x-request-id` is propagated everywhere.
* **OpenAPI** on every service: Rust uses utoipa (`/openapi.json`, `/docs`), Python uses FastAPI defaults. The gateway
  exposes them under `/api/docs/<service>`.
* **Observability:** JSON logs, Prometheus `/metrics`, `/health/live` and `/health/ready` on every service.
* **Config:** 12-factor env vars only (`.env.example`).
* **Containers:** multi-stage builds, non-root user, pinned images, healthchecks, read-only root FS where possible.
* **Testing:** rule-engine unit tests; Rust service integration tests against real Postgres (`#[sqlx::test]`); pytest
  for Python services (Ollama/OpenSearch mocked); vitest for web; CI runs all of them.
* **Authorship:** manifests list `Hendri Karisma <situkangsayur@gmail.com>` only.

## 7. Ports

| Service | Internal port | Published (dev) |
|---|---|---|
| gateway | 80 | 8080 |
| web | 3000 | 3000 |
| core-api | 8080 | — |
| rule-service | 8081 | — |
| graph-service | 8082 | — |
| ml-service | 8001 | — |
| llm-service | 8002 | — |
| ingest-service | 8003 | — |
| postgres | 5432 | 5433 |
| opensearch | 9200 | — |
| ollama | 11434 | 11434 |

Gateway routes (all under `/api/v1`):

| Path prefix | Service |
|---|---|
| `/auth`, `/me`, `/tenants`, `/admin`, `/projects` (collection + project root), `/projects/{pid}/{members,settings,data-sources,events,customers,decisions,cases,labels,analytics,audit,score,field-catalog}`, `/ingest` | core-api |
| `/projects/{pid}/{rules,rulesets,reference-lists,formulas,proposals}`, `/tenants/{tid}/reference-lists` | rule-service |
| `/projects/{pid}/graph` | graph-service |
| `/projects/{pid}/ml`, `/ml/algorithms` | ml-service |
| `/projects/{pid}/llm`, `/tenants/{tid}/regulations` | llm-service |
| `/projects/{pid}/data-sources/{id}/inspect`, `/projects/{pid}/data-sources/{id}/jobs`, `/projects/{pid}/ingest-jobs` | ingest-service |
