# Technical Overview — Tech Stack, Architecture, AI and Methods

> Document status: verified against commit `07ee765` on 2026-10-13.

One-document tour of the platform for engineers, reviewers and researchers. Details live in the linked documents:
[architecture](architecture.md) · [rule DSL](rule-dsl.md) · [ML plugins](ml-plugins.md) ·
[multi-tenancy](multi-tenancy.md) · [data sources](data-sources.md) · [evaluation](evaluation.md) ·
[API contract](api-contract.md) · [Rust codebase guide](rust-codebase-guide.md) · [LLM & agent](llm-agentic.md). The non-technical summary is
[business/overview.md](../business/overview.md). The planned multi-agent evolution of the LLM assistant is described
in [multi-agent.md](multi-agent.md).

---

## 1. Tech stack

| Layer | Technology | Version | Why |
|---|---|---|---|
| Gateway | Traefik (file provider, no docker socket) | 3.1 | Path routing to services, IP allow-list, rate limits, body limits, security headers, OpenAPI aggregation |
| Core services | Rust (`core-api`, `rule-service`, `graph-service`) | toolchain 1.96.1 (Docker), axum 0.8, tokio, sqlx 0.8, utoipa 5 | Hot path of every scoring call: predictable latency, memory safety, one binary per service |
| Rule engine | Rust crate `rule-engine` | in-repo | Pure library (no I/O): formula parser, condition tree, statistics, aggregation, validation; unit- and property-tested in isolation |
| AI / ML service | Python 3.12, FastAPI ≥ 0.115, PyTorch ≥ 2.4 (CPU wheels), scikit-learn ≥ 1.5, pandas ≥ 2.2, networkx | — | ML ecosystem; only where models are trained or served |
| LLM service | Python 3.12, FastAPI, httpx, opensearch-py ≥ 2.6, pypdf ≥ 5 | — | RAG + tool-using agent; no LangChain, so every prompt and call is explicit and auditable |
| Ingest service | Python 3.12, FastAPI, pandas, pyarrow, SQLAlchemy | — | Schema inference and file/DB parsing (CSV, TSV, JSON/JSONL, Parquet, XLSX, Postgres/MySQL), SQL pull connectors |
| Database | PostgreSQL | 16.4 | System of record; row-level security per tenant; one schema + least-privilege role per service; `pg_trgm` for similarity |
| Vector / search | OpenSearch | 2.19.1 | kNN (HNSW, Lucene engine) + BM25 over regulation chunks, one index per tenant |
| LLM runtime | Ollama (`ollama/ollama:0.12.3`, optional `local-llm` profile or an external GPU host) or Gemini (OpenAI-compatible API) for chat | — | Local/private by default; cloud chat optional; embeddings always on Ollama (`bge-m3`) |
| Web | Nuxt 4.5 + Nuxt UI 4.11, Pinia, vue-i18n (ID/EN), ECharts, Cytoscape, CodeMirror | Node ≥ 22.19 | UI per engine plus a BFF that keeps tokens server-side |
| Auth | JWT (HS256) access + rotating refresh tokens, Argon2id password hashes, sealed session cookie in the BFF | — | Browser never sees tokens |
| Packaging | Docker Compose (+ `docker-compose.gpu.yml` for an NVIDIA GPU), uv, cargo-chef | — | One command to run everything |
| CI | GitHub Actions: fmt/clippy/test + integration (Rust), ruff/mypy/pytest (Python), eslint/typecheck/vitest/build (web), migrations + RLS smoke tests, image builds | — | Every service is gated |

## 2. Software architecture

### 2.1 System context

```mermaid
flowchart LR
  AN["Analysts / approvers / admins"] -->|"browser, UI :3000"| P["Fraud Detection Platform v2"]
  CS["Client systems"] -->|"events, webhooks :8080/api/v1"| P
  P -->|"approve / review / decline + reasons"| CS
  CDB[("Client DBs")] -->|"pull connectors"| P
  REG["Regulation / policy PDFs"] -->|"upload"| P
  P -->|"chat, embeddings"| LLM["Ollama (local / GPU host)<br/>Gemini (optional chat)"]
```

### 2.2 Containers

```mermaid
flowchart LR
  B[Browser] -->|"UI :3000"| GW["Traefik gateway<br/>allow-list, rate limits"]
  C["Client systems<br/>webhook / API"] -->|"/api/v1 :8080"| GW
  GW -->|ui entrypoint| WEB["web<br/>Nuxt BFF"]
  WEB -->|"/api/v1 + Bearer"| GW
  GW --> CORE["core-api<br/>Rust · orchestrator"]
  GW --> RULE["rule-service<br/>Rust"]
  GW --> GRAPH["graph-service<br/>Rust"]
  GW --> ML["ml-service<br/>Python"]
  GW --> LLM["llm-service<br/>Python"]
  GW --> ING["ingest-service<br/>Python"]
  CORE --> RULE & GRAPH & ML
  RULE -->|graph rules| GRAPH
  ML -->|edge export| GRAPH
  LLM -->|tools| CORE & RULE & GRAPH & ML
  ING -->|internal batch| CORE
  ING -->|mapping suggest| LLM
  CORE & RULE & GRAPH & ML & LLM & ING --> PG[("PostgreSQL 16<br/>schema per service + RLS")]
  LLM --> OS[("OpenSearch<br/>regulation chunks")]
  LLM --> OL["Ollama / Gemini"]
```

Principles:

* **Schema per service, shared cluster.** Each service owns its schema and connects with its own role; cross-service
  reads go through APIs or explicit, documented read grants (architecture §2.3). The only shared writes are the
  `core.audit_log` (append-only) and the `core.approvals` ledger.
* **Multi-tenancy by construction.** Every row carries `tenant_id` (+ `project_id`); every transaction sets
  `app.tenant_id` and Postgres RLS filters rows. Composite foreign keys stop a row from pointing at another tenant's
  project.
* **Hexagonal services.** Rust services split `domain` (pure logic) / `app` or `application` (use cases) /
  `adapters` (SQL, HTTP clients) / `api` (axum); the rule engine is a separate pure crate.
* **Degraded, never lost.** An event is committed before any engine is called; a failing engine is dropped from the
  decision and recorded in `decision.degraded`.
* **Governance everywhere.** Rules, rulesets and ML models are immutable versions with maker–checker approval and
  a shadow mode; every change goes to an append-only audit log.

### 2.3 Scoring pipeline (one event)

```mermaid
sequenceDiagram
  participant S as Source (webhook / API / file job)
  participant C as core-api
  participant G as graph-service
  participant M as ml-service
  participant R as rule-service
  S->>C: raw record
  C->>C: mapping → canonical event + customer (+ label), hash PAN / account
  C->>C: persist customer + event (committed)
  C->>G: link entities (phone, email, card, device, address, …)
  C->>C: compute features v1
  C->>G: graph metrics (distance to fraud, shared entities, component, community)
  par ML
    C->>M: supervised predict
  and
    C->>M: unsupervised score
  end
  C->>R: evaluate rulesets (event + source + customer + features + ml + graph)
  C->>C: combine scores → approve / review / decline (+ case)
  C-->>S: DecisionOut (score, engine scores, reasons, trace)
```

Time budgets (per project setting `timeouts`): graph 100 ms (links 3×), ML 200 ms, rules 300 ms; each rule also has
its own budget in rule-service. Graph metrics run before ML so that serving features equal training features.
`load_only` (backfill) events stop after features are stored.

## 3. AI architecture

The platform combines **four scoring engines** and one **LLM assistant**. No single model decides alone, and the
assistant never scores.

| Engine | Learns from | Strength | Weakness covered by others |
|---|---|---|---|
| Rules | analyst knowledge, regulations | explainable, immediate, enforce policy | cannot generalise to new patterns |
| Supervised ML | labelled history | high precision on known fraud types | blind to new typologies, needs labels |
| Unsupervised ML | unlabelled behaviour | finds new / rare patterns | noisy, not explainable alone |
| Graph | relationships between customers | rings, mules, shared devices/cards | weak for first-party, isolated fraud |
| LLM assistant | regulations + platform data | turns regulation and drift into rule proposals | never scores events; humans approve |

### 3.1 Decision fusion

Each engine yields a score `sᵢ ∈ [0,100]` with project weights `wᵢ` (default rules 0.45, supervised 0.30,
unsupervised 0.10, graph 0.15). The default fusion is a weighted **noisy-OR**:

```
final = 100 · (1 − Π (1 − sᵢ/100)^(wᵢ / w_max))
```

Independent evidence accumulates and a silent engine (s = 0) never dilutes a strong signal. `weighted_average` is
available as an alternative. Thresholds (default review ≥ 50, decline ≥ 80) and `force_*` rule actions produce the
decision; every decision carries up to 8 attributed reasons.

### 3.2 ML plugins

Algorithms are **plugins** discovered at runtime (hot reload via `POST /api/v1/ml/algorithms/reload`, validation and
smoke test before use). Training is per project, asynchronous (`ML_TRAINING_WORKERS`), with a time-based split,
imbalance handling, label maturity (unlabelled events older than `label_maturity_days`, default 14, count as legit),
and model registry + approval.

* Supervised (built-in): `mlp_backprop` (PyTorch), `logistic_regression`, `gradient_boosting`, `random_forest`.
* Anomaly (built-in): `isolation_forest`, `local_outlier_factor`, `autoencoder`; `knn_distance_anomaly` is the
  example external plugin in `plugins/`.
* Clustering (built-in): `hdbscan`, `kmeans`, `dbscan`, `gaussian_mixture`.
* Defaults (`core.projects.ml_config`): `mlp_backprop` + `isolation_forest` / `hdbscan`.

### 3.3 LLM assistant (RAG + tool-using agent)

```mermaid
flowchart LR
  subgraph ingest["Regulation library (per tenant)"]
    PDF[Regulation / policy PDF] --> X[extract text] --> CH["structure-aware chunking<br/>BAB › Bagian › Pasal › ayat, Lampiran"]
    CH --> EMB["embeddings bge-m3<br/>(Ollama)"] --> OS[("OpenSearch<br/>reg-chunks-{tenant}")]
    PDF -.->|supersedes| DIFF[change diff + summary]
  end
  subgraph chat["Chat (sync or SSE)"]
    Q[Analyst question] --> A["agent loop<br/>≤ 6 tool rounds"]
    A -->|search_regulations| HY["hybrid search<br/>BM25 + kNN → RRF"] --> OS
    A -->|"13 read-only platform tools"| T["rules, performance, analytics,<br/>drift, typologies, clusters, graph,<br/>validate, backtest, project context"]
    A -->|create_rule_proposal| P["pending proposal<br/>validated + backtested 30 d"]
    A --> ANS["answer with citations<br/>KODE §Pasal"]
  end
  subgraph analyses["Analyses (async jobs → llm.reports)"]
    AN["rule-relevance · fraud-situation ·<br/>regulation-impact · recommend-rules"] --> SO["structured JSON output<br/>(schema-constrained, repaired)"]
    SO --> P
  end
  P --> HUM["human approval → shadow → second approval → active"]
```

```mermaid
sequenceDiagram
  participant U as Analyst (web BFF)
  participant L as llm-service
  participant P as Ollama / Gemini
  participant T as platform services (INT token)
  participant O as OpenSearch
  U->>L: POST /llm/chat/stream {message}
  L->>L: load project context (business_context, llm_config, attached regulations)
  loop until no tool call or 6 rounds
    L->>P: chat + tool specs (num_ctx, num_predict)
    P-->>L: tool calls
    L->>O: search_regulations (hybrid, attached docs only)
    L->>T: read-only tools (rules, analytics, drift, graph, ML, validate, backtest)
    L-->>U: SSE tool / citation events
  end
  L->>P: final answer
  L-->>U: SSE token … done {answer, citations, proposals}
```

* **Retrieval:** hybrid BM25 (exact legal terms) + kNN (semantic), fused with Reciprocal Rank Fusion, restricted to
  documents attached to the project; `retrieval_k` = 6, and the agent always retrieves at least 4 chunks.
* **Agent:** native tool calling with **14 allow-listed tools** (13 read-only + `create_rule_proposal`). Each tool
  has a timeout (20 s), results are truncated to 6 000 characters, and integer arguments from the model are
  validated and clamped.
* **Analyses:** four asynchronous analyses produce stored reports. Prompts are slimmed to fit the context window
  (clusters and graph components are summarised; prompt size is logged and a warning is emitted when it exceeds
  `OLLAMA_NUM_CTX`). Output is grammar-constrained JSON: free-form objects stay open (`additionalProperties`),
  free-text fields carry `maxLength`, and `OLLAMA_NUM_PREDICT` (default 4096) bounds every generation.
* **Guardrails:** prompts forbid inventing numbers or articles; proposals are schema-validated by rule-service,
  auto-repaired (up to 2 attempts), backtested on 30 days, and start as **pending** → approval → **shadow** before
  they can score.
* **Providers:** `LLM_PROVIDER=ollama` (default; `<think>` blocks of reasoning models are stripped, `OLLAMA_THINK`
  can disable thinking, `OLLAMA_TIMEOUT_S` default 180 s, `OLLAMA_NUM_CTX` default 8192) or `gemini` for chat
  (`GEMINI_MODEL`, default `gemini-flash-latest`). Embeddings stay on Ollama so the index never depends on the chat
  provider. GPU sizing is in [`../guides/deployment.md`](../guides/deployment.md).

## 4. Methods and science

### 4.1 Rules

* **Tri-state logic:** `match` / `no_match` / `trapped` (evaluation error: division by zero, null, missing reference,
  timeout). Trapped outcomes are logged and can score or route to review.
* **Formula operands:** user-typed functions such as `F(x,y,z) = 2x + 2^y / z^2` (implicit multiplication, math and
  statistics functions).
* **Velocity statistics** over a customer's history:

| Function | Definition |
|---|---|
| z-score | `(x − μ) / σ` of the history |
| gaussian tail | `P(X ≥ x)` (or lower/two-sided) under `N(μ, σ)` |
| percentile rank | share of history ≤ x |
| linear trend | OLS over bucketed series → slope, forecast, residual z = `(actual − forecast)/σ_res` |
| poisson tail | `λ` = mean bucket count; `P(N ≥ current count)` |

* **Ruleset aggregation:** `sum`, `max`, `probabilistic_or` (`1 − Π(1 − pᵢ)`, default), `weighted_average`, capped
  by `max_score`.

### 4.2 Machine learning

* **MLP with backpropagation:** fully connected layers, ReLU, batch-norm, dropout; loss = binary cross-entropy with
  `pos_weight = N_neg / N_pos` (clamped to 1–1000) for class imbalance; Adam optimiser; early stopping on validation
  **PR-AUC** (more honest than ROC-AUC when fraud is rare). Explanation per prediction = gradient × input.
* **Isolation Forest:** anomaly score from the average path length to isolate a point in random trees
  (`s(x) = 2^(−E[h(x)] / c(n))`), min-max normalised on the training distribution.
* **HDBSCAN:** density-based hierarchical clustering. Points that belong to no cluster are noise (−1); per-cluster
  fraud rate and distinguishing features are profiled for analysts.
* **Validation:** time-based split (train on the past, test on the future; stratified fallback when a side lacks a
  class) to avoid leakage; metrics ROC-AUC, PR-AUC, confusion matrix, calibration.
* **Drift:** Population Stability Index `PSI = Σ (aᵢ − eᵢ) · ln(aᵢ / eᵢ)` per feature between the recent 7 days and
  the previous 30 days.

### 4.3 Graph

* **Entity resolution:** customers are linked through shared phone, email, device, address, card, bank account and
  referenced transaction (default link kinds), plus `ip` and `api_client` when a project opts in. Matching is exact
  after normalisation (cards and bank accounts only as tenant-peppered HMAC fingerprints, never raw), and through
  similar phones/emails/addresses (one-digit phone edits, same last 9 digits, same e-mail local part, PostgreSQL
  `pg_trgm` trigram similarity above the project threshold). Supernodes (degree above `supernode_degree_cap`) are
  skipped.
* **Metrics:** BFS shortest distance to a known fraudster (score map `{1: 90, 2: 70, 3: 40}`), number of fraud
  neighbours, shared fraud entities, degree, connected-component size.
* **Communities:** Louvain modularity optimisation (networkx, in ml-service over the graph-service edge export);
  community fraud rate feeds graph rules.

### 4.4 Evaluation (synthetic, out-of-sample)

The simulator generates realistic typologies (carding, account takeover, system breach, account takeover of bank
accounts, money mules, return abuse, promo abuse). Results with the default threshold 50
([evaluation.md](evaluation.md)):

| Project | Recall | Precision | FPR | AUC final |
|---|---|---|---|---|
| checkout (pre-payment) | 95.2% | 40.0% | 7.4% | 0.986 |
| post-payment | 82.2% | 17.7% | 32.1% | 0.862 |
| returns | 100% | 13.7% | 34.7% | 0.998 |
| promo | 100% | 63.4% | 5.5% | 0.999 |

High AUC with low precision in post-payment/returns means the ranking is good but the threshold is too low; per-project
threshold recommendation is on the backlog. Public research datasets (Sparkov, PaySim, SAML-D) can be seeded as
separate tenants for reproducible experiments ([research-dataset.md](research-dataset.md),
[../guides/experiments.md](../guides/experiments.md)).

## 5. Engineering practices

* **Testing:** unit tests per crate/package (rule-engine also has property tests for the formula parser), Rust
  integration tests against a real Postgres 16 (with RLS and roles), Python tests with HTTP mocks, web unit tests,
  migration + RLS smoke tests, end-to-end simulation.
* **Security:** IP allow-list at the gateway, rate limits on auth and ingest, JWT with rotating refresh, Argon2id,
  RLS, least-privilege DB roles, no tokens in the browser, anti-CSRF header + Origin host check on mutating BFF
  calls, audit log that cannot be updated or deleted, 64 KB header limit on the web.
* **Operations:** health/readiness endpoints, structured JSON logs with request ids, Prometheus metrics, OpenAPI per
  service (`/api/openapi/<service>.json`), seeding and demo reset (`deploy/seed.sh demo|research|master|restore`,
  `deploy/demo/reset-demo.sh`, nightly master restore, `deploy/demo/wipe-tenant.sh`).

## 6. References

* Breiman, L. (2001). *Random Forests.* Machine Learning 45.
* Rumelhart, D., Hinton, G., Williams, R. (1986). *Learning representations by back-propagating errors.* Nature 323.
* Liu, F. T., Ting, K. M., Zhou, Z.-H. (2008). *Isolation Forest.* ICDM.
* Breunig, M. et al. (2000). *LOF: Identifying density-based local outliers.* SIGMOD.
* Campello, R., Moulavi, D., Sander, J. (2013). *Density-based clustering based on hierarchical density estimates (HDBSCAN).* PAKDD.
* Blondel, V. et al. (2008). *Fast unfolding of communities in large networks (Louvain).* J. Stat. Mech.
* Cormack, G., Clarke, C., Büttcher, S. (2009). *Reciprocal rank fusion outperforms Condorcet and individual rank learning methods.* SIGIR.
* Lewis, P. et al. (2020). *Retrieval-Augmented Generation for knowledge-intensive NLP tasks.* NeurIPS.
* Saito, T., Rehmsmeier, M. (2015). *The precision-recall plot is more informative than the ROC plot on imbalanced datasets.* PLOS ONE.
* Otoritas Jasa Keuangan (2024). *POJK No. 12 Tahun 2024 tentang Penerapan Strategi Anti Fraud bagi Lembaga Jasa Keuangan.*
