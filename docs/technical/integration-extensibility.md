# Integration & extensibility: what adapts to a client, and what is a plugin

> Document status: verified against commit `7e3d4f4` on 2026-10-21. Answers two recurring questions:
> *"Can the request/response and data structure follow the client's format?"* and *"Is this built on plugins?"*

## 1. Short answer

| Area | Adapts to the client? | How | Code change needed? |
|---|---|---|---|
| **Client data structure** (fields, nesting, names, formats, dates, currencies, phone formats) | **Yes** | data source + versioned **mapping JSON** with transforms ([`data-sources.md`](data-sources.md) §3); schema inference suggests the mapping; unmapped fields stay usable as `source.<path>` | no |
| **Request format** (how the client sends data) | **Yes** | webhook `POST /api/v1/ingest/{slug}` with the client's own JSON record, batch (≤ 1 000), file upload (CSV/TSV/JSON/JSONL/Parquet/XLSX), SQL pull from Postgres/MySQL | no |
| **Response format** (what the client gets back) | **No, fixed** | always `DecisionOut` (§3) or the batch summary; no per-client response template, no outbound callback/webhook yet | yes → backlog **E19** |
| **Detection logic** | Yes | Rule DSL (5 rule kinds, formulas, rulesets, reference lists), per-project thresholds and engine weights, link kinds for the graph | no (configuration + maker–checker) |
| **ML algorithms** | Yes, **plugin** | Python plugin classes hot-loaded from `PLUGIN_DIR` (`./plugins`) | a plugin file, no platform change |
| **LLM provider / model** | Partly | today `.env` (Ollama or Gemini, one global provider) + per-project `llm_config.chat_model`; per-agent / multi-model / API keys via the AI Console | AI Console = epic **E18** ([TRD](trd-agent-ai-console.md)) |
| **LLM tools** | No | 20 tools defined in code (`agent/tools.py`) | yes (by design: least privilege, reviewed code) |

So the platform is **configuration-driven for data and rules**, and **plugin-based for ML algorithms**. It is not
a general plugin system: rule kinds, the graph engine, the scoring fusion and the LLM tools are built in.

## 2. Data in: the client's own structure

```
client record (own JSON / CSV row / SQL row)
   │  data source (webhook | file | postgres | mysql), API key per webhook source
   ▼
mapping JSON (versioned: draft → active → archived, maker–checker)
   │  from: dotted paths · const · default · value_map
   │  transforms: to_number(locale) · parse_datetime(format, timezone) · normalize_phone(country)
   │              normalize_email · hash_pan / hash_account · pan_bin / pan_last4 · concat · coalesce · regex_extract …
   ▼
canonical event + customer (+ optional label)    unmapped fields → source.<path> (field catalog)
   ▼
scoring: rules (any canonical, feature, ml, graph or source.* path) · ML · graph · fusion → DecisionOut
```

* Example: a client sending `{"trx_id": "...", "created": "21/10/2026 10:15:00", "user": {"hp": "0812..."},
  "total": "1.500.000,50"}` is mapped once in the UI (**Sumber data** page). Nothing in the client changes.
* Mapping errors return `problem+json 422` with the failing field; bad rows of a batch go to `core.ingest_errors`.
* Raw card and account numbers are hashed during normalisation and never stored.

## 3. Data out: the fixed decision contract

`POST /api/v1/ingest/{slug}` (and `POST /projects/{pid}/events`) returns `DecisionOut`:

| Field | Meaning |
|---|---|
| `event_id`, `external_id`, `project_id` | ids (the client's own `external_id` is echoed back) |
| `decision` | `approve` \| `review` \| `decline` |
| `final_score` | 0–100 after fusion |
| `engine_scores` | per engine: rules, supervised, unsupervised, graph |
| `reasons[]` | human-readable reasons (rule hits, model drivers, graph proximity) |
| `rule_results[]` | per-rule trace (matched, score, mode live/shadow) |
| `ml`, `graph` | model summary, graph metrics (`distance_to_fraud`, shared entities, …) |
| `degraded[]` | engines skipped (timeout/down); the decision still comes from the others |
| `case_id` | case opened for `review`/`decline`, if any |
| `latency_ms`, `persisted` | timing; `false` for simulations |

Batch ingest returns `{accepted, rejected, errors[], decisions[{external_id, event_id, decision, final_score}]}`.

**Gap (E19, not built):** a client that needs, for example, `{"status": "REJECT", "risk": 87}` must translate
`DecisionOut` on its side today. Proposed:
1. **Response template per data source.** The same mapping engine, run in reverse (`DecisionOut` → client JSON), is
   versioned and approved like a mapping.
2. **Outbound webhook / callback.** Signed (HMAC) POST to the client when a decision is final, a case is resolved or a
   label changes, with retries and a dead-letter table. This serves asynchronous flows (`review` resolved by an analyst).
3. **Status-code policy per source.** For example, an always-200 mode for clients that treat 4xx as transport errors.

## 4. ML plugins

* Contract: [`ml-plugins.md`](ml-plugins.md) §1. Subclass `SupervisedPlugin`, `AnomalyPlugin` or `ClusteringPlugin`,
  give it a JSON Schema for its hyper-parameters (the UI form is generated from it), and implement fit, predict/score, save and load.
* Discovery: put a `.py` file (or package) exposing `PLUGINS = [...]` in `./plugins`, then **Algoritma (plugin) →
  Reload** (platform admin) or restart ml-service. Each plugin is validated and smoke-tested before it can be selected.
* Example: `plugins/example_knn_anomaly.py`.
* Limits: plugin dependencies are not installed (supply-chain risk), and models are trained and activated through
  maker–checker like built-ins.

## 5. Where configuration lives

| What | Where in the UI | Stored in |
|---|---|---|
| Mapping, data sources, API keys | Data → Sumber data | `core.data_sources`, `core.data_source_mappings` |
| Rules, rulesets, reference lists, formulas | Rule engine | `rules.*` (versioned, maker–checker) |
| Thresholds, engine weights, graph link kinds, timeouts, LLM language/model | Pengaturan project | `core.project_settings`, `core.projects.*_config` |
| ML algorithm + hyper-parameters, model activation | ML supervised / unsupervised | `ml.models` |
| Regulations attached to a project | Asisten LLM → Regulasi proyek | `llm.project_regulations` |
| LLM provider, keys, models per agent | `.env` today → **AI Console** (E18) | `llm.ai_*` (planned) |
| Theme, language (per user, per browser) | Account menu (top right) → Tema / Bahasa, or **Profil saya** | browser storage / `fraud_locale` cookie |
