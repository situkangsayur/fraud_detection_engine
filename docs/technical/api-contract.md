# API contract v1

> Document status: verified against commit `07ee765` on 2026-10-13.

External base URL: the gateway, `http://<host>:8080/api/v1` (routing table in architecture §7). The gateway strips
nothing, so services mount the same `/api/v1/...` paths. Internal service-to-service calls use the `/v1/...` paths
(no `/api` prefix) on the service's own port; the gateway never routes them. The browser talks only to the web BFF,
which maps `/api/<path>` to the gateway's `/api/v1/<path>` and adds the bearer token from the session.

Conventions (architecture §6): JSON with snake_case; lists return `{items,total,page,page_size}`; errors use RFC 7807;
`x-request-id` is propagated. `{pid}` is a project UUID and `{tid}` a tenant UUID.

Date-range query parameters (`from`, `to`) accept RFC 3339 date-times or plain `YYYY-MM-DD` days (UTC): a bare
`from` is that day's midnight, a bare `to` includes the whole day. JSON bodies still use RFC 3339.

**Auth legend:**
* **JWT(role)**: `Authorization: Bearer <jwt>`. The role is checked against the `prj[{pid}]` claim
  (`project_admin > approver > analyst > viewer`); tenant admins pass every project check of their tenant.
  **TA** = tenant_admin of `{tid}`. **PA** = platform admin.
* **INT**: `Authorization: Bearer ${INTERNAL_API_TOKEN}` plus `X-Tenant-Id`, `X-Project-Id`, and optionally `X-Actor`.
* **KEY**: data-source API key in `X-Api-Key`.

Every service also exposes `GET /health/live`, `GET /health/ready`, `GET /metrics`, `GET /openapi.json` and
`GET /docs` (Scalar UI on Rust services, Swagger UI on Python services). Through the gateway the OpenAPI documents are
`GET /api/openapi/{core,rule,graph,ml,llm,ingest}.json`.

---

## A. core-api (`core-api:8080`)

### A.1 Auth, tenants, users
| Method | Path | Auth | Notes |
|---|---|---|---|
| POST | `/api/v1/auth/login` | — | `{email,password}` → `{access_token, token_type, expires_in, refresh_token, user}`. The web BFF moves the refresh token into an httpOnly cookie |
| POST | `/api/v1/auth/refresh` | refresh token | rotation with reuse detection → new pair |
| POST | `/api/v1/auth/logout` | JWT | revokes the refresh-token family |
| GET | `/api/v1/me` | JWT | user, tenant, and the projects with roles |
| GET/POST | `/api/v1/tenants` | PA | list/create `{slug,name, admin:{email,full_name,password}}` |
| GET/PATCH | `/api/v1/tenants/{tid}` | PA or TA | |
| GET/POST | `/api/v1/tenants/{tid}/users` | TA or PA | `{email, full_name, password, tenant_role}` |
| PATCH | `/api/v1/tenants/{tid}/users/{uid}` | TA or PA | `{tenant_role?, is_active?, password?}` |

### A.2 Projects
| Method | Path | Auth | Notes |
|---|---|---|---|
| GET | `/api/v1/projects` | JWT | projects the user can see (the tenant's projects for a TA) |
| POST | `/api/v1/projects` | TA | `{slug, name, description, stage, business_context, timezone, currency, template?: stage-name \| "none", ml_config?, llm_config?, graph_config?}`. Creates default settings, the `canonical` internal data source, adds the creator as project_admin, and calls rule-service `POST /v1/projects/{pid}/bootstrap` when a template is given |
| GET | `/api/v1/projects/{pid}` | JWT(viewer) | includes `ml_config`, `llm_config`, `graph_config`, plus `summary:{events_30d, open_cases, active_rules, active_models}` |
| PATCH | `/api/v1/projects/{pid}` | JWT(project_admin) | any editable field; `ml_config` is validated by ml-service `POST /v1/algorithms/validate-config` |
| POST | `/api/v1/projects/{pid}/archive` | TA | |
| GET | `/api/v1/projects/{pid}/members` | JWT(viewer) | members with roles |
| POST | `/api/v1/projects/{pid}/members` | JWT(project_admin) | `{user_id, role}` |
| PUT/DELETE | `/api/v1/projects/{pid}/members/{uid}` | JWT(project_admin) | PUT `{role}` |
| GET | `/api/v1/projects/{pid}/settings` | JWT(viewer) | all keys (defaults merged) |
| PUT | `/api/v1/projects/{pid}/settings/{key}` | JWT(project_admin) | validated per key: `decision_thresholds {review, decline}`, `engine_weights {rules, supervised, unsupervised, graph}`, `graph_scores {fraud_distance_scores, shared_fraud_entity_score}`, `timeouts {graph_ms, ml_ms, rules_ms}` (each 10–5000), `cases {auto_create_on, dedupe_open_per_customer}` (`approve` is not allowed in `auto_create_on`; one open case per customer is always enforced by a unique index and new events attach to it), `engine_combination` (`"noisy_or"` default \| `"weighted_average"`), `rules_unavailable_decision` (`approve\|review\|decline`, default `review`). Unknown keys → 422 |

### A.3 Ingest & events
| Method | Path | Auth | Notes |
|---|---|---|---|
| POST | `/api/v1/projects/{pid}/events` | JWT(analyst) or INT | `CanonicalEventIn` → `DecisionOut` |
| POST | `/api/v1/ingest/{source_slug}` | KEY | raw record in the source's own shape → `DecisionOut` (201), or problem+json 422 with mapping errors |
| POST | `/api/v1/ingest/{source_slug}/batch` | KEY | `{records:[≤1000], mode?}` → `{accepted, rejected, errors:[{index,reason}], decisions:[{external_id, event_id, decision\|null, final_score\|null}]}` (`decision` is null for `load_only`) |
| POST | `/v1/internal/projects/{pid}/sources/{source_id}/batch` | INT | same as above; used by ingest-service jobs (`job_id` in the body is used to tag ingest errors) |
| POST | `/api/v1/projects/{pid}/score/simulate` | JWT(analyst) | `{event}` or `{source_id, record}` → `DecisionOut` with `persisted:false` |
| POST | `/api/v1/projects/{pid}/events/{id}/rescore` | JWT(analyst) | re-runs the scoring stages (graph metrics, ML, rules, combine, persist); the new decision replaces the old one, which is kept in audit |
| GET | `/api/v1/projects/{pid}/events` | JWT(viewer) | filters `event_type, decision, customer_id, source_id, from, to, min_score, q`. `q` matches the event `external_id` exactly or by prefix (index `ix_events_external_prefix`, migration `0012`) or the customer `external_id` exactly. Sorted by `occurred_at` desc |
| GET | `/api/v1/projects/{pid}/events/{id}` | JWT(viewer) | event, payload, features, decision, labels, case |
| GET | `/api/v1/projects/{pid}/customers[/{id}]` | JWT(viewer) | search `q, risk_label`; detail includes stats |
| GET | `/api/v1/projects/{pid}/customers/{id}/events` | JWT(viewer) | |

`CanonicalEventIn`: `{external_id, event_type, occurred_at, customer:{external_id, full_name?, email?, phone?,
registered_at?, kyc_level?, segment?, attributes?}, <canonical event fields — feature-catalog §1>, card_number?,
account_number? (hashed immediately, never stored), payload?:{}}`.

`DecisionOut`:
```json
{ "event_id": "uuid", "external_id": "T-1", "project_id": "uuid", "decision": "review", "final_score": 63.4,
  "engine_scores": { "rules": 72.0, "supervised": 55.1, "unsupervised": 31.0, "graph": 70.0 },
  "reasons": [ { "code": "RL-ATO-001", "engine": "rules", "contribution": 40.0, "message": "New device + credential change 24h" } ],
  "rule_results": [ "… rule-dsl §7 trace items …" ],
  "ml": { "fraud_probability": 0.551, "anomaly_score": 0.31, "cluster_id": 4, "cluster_fraud_rate": 0.12,
          "supervised_model": {"id": "uuid", "version": 3, "algorithm": "mlp_backprop"},
          "unsupervised_model": {"id": "uuid", "version": 1} },
  "graph": { "distance_to_fraud": 2, "fraud_neighbors_1": 0, "fraud_neighbors_2": 1, "component_size": 7,
             "shared_entity_count": 2, "degree": 3, "community_fraud_rate": 0.2 },
  "degraded": [], "case_id": "uuid|null", "latency_ms": 42, "persisted": true }
```

### A.4 Decisions, cases, labels
| Method | Path | Auth | Notes |
|---|---|---|---|
| GET | `/api/v1/projects/{pid}/decisions/{event_id}` | JWT(viewer) | |
| GET | `/api/v1/projects/{pid}/cases[/{id}]` | JWT(viewer) | filters `status, assigned_to, typology, priority, customer_id`; detail includes event, decision, customer, graph summary |
| PATCH | `/api/v1/projects/{pid}/cases/{id}` | JWT(analyst) | `{status?, assigned_to?, priority?, note?}`; status `open\|in_review\|resolved_fraud\|resolved_legit`, priority 1–5 |
| POST | `/api/v1/projects/{pid}/cases/{id}/resolve` | JWT(analyst) | `{label, fraud_type?, notes?, apply_to_customer?}` → labels. When a customer label changes, core-api calls graph-service `PUT /v1/projects/{pid}/customers/{cid}/label` |
| POST | `/api/v1/projects/{pid}/labels` | JWT(analyst) or INT | `{subject_type: event\|customer, subject_id, label: fraud\|legit, fraud_type?, source?: analyst\|chargeback\|customer_report\|dataset\|system, notes?, apply_to_customer?}`. A `customer` label (or an event label with `apply_to_customer:true`) updates `customers.risk_label` and notifies graph-service |
| GET | `/api/v1/projects/{pid}/labels` | JWT(viewer) | |

### A.5 Data sources, mappings, field catalog
| Method | Path | Auth | Notes |
|---|---|---|---|
| GET/POST | `/api/v1/projects/{pid}/data-sources` | viewer / analyst | kinds `webhook\|file\|postgres\|mysql\|internal`, mode `score\|load_only`; create returns `api_key` **once** for webhook sources |
| GET/PATCH/DELETE | `/api/v1/projects/{pid}/data-sources/{id}` | viewer / analyst / project_admin | |
| POST | `/api/v1/projects/{pid}/data-sources/{id}/rotate-key` | project_admin | |
| GET/POST | `/api/v1/projects/{pid}/data-sources/{id}/mappings` | viewer / analyst | create a draft version (validated) |
| POST | `/api/v1/projects/{pid}/data-sources/{id}/mappings/preview` | analyst | `{mapping, records}` → `[{ok, event?, customer?, label?, errors?}]` |
| POST | `/api/v1/projects/{pid}/data-sources/{id}/mappings/{v}/activate` | analyst | registers `source.*` fields in `field_catalog` |
| GET | `/api/v1/projects/{pid}/data-sources/{id}/errors` | viewer | dead-letter rows |
| GET | `/api/v1/projects/{pid}/field-catalog` | viewer | built-ins (from code) + source fields; filters `entity, source_id, velocity_enabled, q` |
| PATCH | `/api/v1/projects/{pid}/field-catalog/{path}` | project_admin | `{velocity_enabled?, description?, pii?}` |
| GET | `/v1/internal/projects/{pid}/field-catalog` | INT | used by llm-service (rule DSL field paths for prompts). rule-service reads `core.field_catalog` directly (read grant, 60 s cache) |

### A.6 Analytics & audit
| Method | Path | Auth | Notes |
|---|---|---|---|
| GET | `/api/v1/projects/{pid}/analytics/overview` | viewer / INT | `from,to` → `{totals:{events, approve, review, decline}, by_event_type, by_label_fraud_type, daily:[{date, events, review, decline, avg_score}], score_histogram, engine_avg, open_cases, degraded_rate}` |
| GET | `/api/v1/projects/{pid}/analytics/drift` | viewer / INT | PSI per numeric feature, recent 7d vs previous 30d → `[{feature, psi, recent_mean, baseline_mean, status}]` |
| GET | `/api/v1/projects/{pid}/analytics/typologies` | viewer / INT | labelled fraud counts & amounts per fraud_type and week |
| GET | `/api/v1/projects/{pid}/audit` | approver | filters `actor, action, subject_type, subject_id, from, to` |
| GET | `/api/v1/tenants/{tid}/audit` | TA | same filters |

---

## B. rule-service (`rule-service:8081`)

| Method | Path | Auth | Notes |
|---|---|---|---|
| POST | `/v1/projects/{pid}/evaluate` | INT | query `rule_timeout_ms?` (1–5000, overrides the per-rule budget `RULE_TIMEOUT_MS`, default 50). Body `{event_id, occurred_at, event_type, context:{event, source, customer, features, ml, graph}, customer_id, dry_run?:bool}` → `{rules_score, rulesets:[{ruleset_id, code, score, shadow}], rule_results:[…], actions:{force_decline, force_approve, force_review}, reasons:[…], duration_ms}`. Unless `dry_run`, also writes `rule_hits` + `rule_eval_counters` (asynchronously when `EVAL_ASYNC_WRITES=true`, the default) |
| POST | `/v1/projects/{pid}/bootstrap` | INT | `{template}` (`pre_payment, post_payment, returns, promo, account_security, payout`; files in `rule-service/templates/`) → creates template rules/rulesets/lists (status `active`, created_by = system) |
| GET | `/api/v1/projects/{pid}/rules` | viewer / INT | filters `kind, status, typology, q`; items include the current version, `stats_7d` and `serving:{live_version, shadow_version}` |
| POST | `/api/v1/projects/{pid}/rules` | analyst | envelope (rule-dsl §2) → rule (draft v1); 422 on validation errors |
| GET/PUT | `/api/v1/projects/{pid}/rules/{id}` | viewer / analyst | PUT creates a new draft version (the approved version keeps serving). There is no DELETE: retire instead |
| GET | `/api/v1/projects/{pid}/rules/{id}/versions/{v}` | viewer | |
| POST | `/api/v1/projects/{pid}/rules/validate` | viewer / INT | envelope → `{valid, errors, referenced_fields, referenced_lists}` |
| POST | `/api/v1/projects/{pid}/rules/test` | analyst | `{rule: envelope, event_id? , context?}` → `{outcome, contribution, trapped_reason, trace}` (loads the stored context of `event_id` from core tables) |
| POST | `/api/v1/projects/{pid}/rules/{id}/backtest` and `/rules/backtest` (inline envelope) | analyst / INT | `{from?, to?, since_days?, limit? (1–50000), version?}` (inline form: `{rule: envelope, from?, to?, since_days?, limit?}`) → `{evaluated, matched, trapped, hit_rate, labeled_fraud_matched, labeled_legit_matched, labeled_fraud_total, precision, recall, sample_matches, by_day, truncated}`. Nothing is persisted |
| POST | `/api/v1/projects/{pid}/rules/{id}/{submit\|approve\|reject\|retire}` | analyst (submit) / approver | approve body `{target_status: active\|shadow, comment?}`; approver ≠ submitter |
| GET | `/api/v1/projects/{pid}/rules/performance` | viewer / INT | `since_days` → `{since_days, items:[{rule_id, code, status, evaluated, matched, trapped, hit_rate, precision, last_hit_at}]}` |
| GET/POST/PUT/DELETE | `/api/v1/projects/{pid}/rulesets[/{id}]` | viewer / analyst | `{code,name,description,event_types,typologies,aggregation,max_score, change_note?}`. Every edit creates a new ruleset version (draft, `rules.ruleset_versions`); list items include `serving:{live_version, shadow_version}`, detail includes `versions[]` and `approvals[]`. DELETE only for a ruleset that was never approved (409 otherwise: retire it) |
| PUT | `/api/v1/projects/{pid}/rulesets/{id}/rules` | analyst | `[{rule_id, weight, pinned_version?}]` → new draft ruleset version |
| POST | `/api/v1/projects/{pid}/rulesets/{id}/{submit\|approve\|reject\|retire\|backtest}` | analyst (submit, backtest) / approver | backtest adds `score_histogram` and `decision_distribution` (uses the project's decision thresholds) |
| GET/POST/PATCH/DELETE | `/api/v1/projects/{pid}/reference-lists[/{id}]` | viewer / analyst | project lists (listing also returns tenant-wide lists, flagged `scope:"tenant"`) |
| GET/POST/PATCH/DELETE | `/api/v1/tenants/{tid}/reference-lists[/{id}]` | any tenant member (read) / TA (write) | tenant-wide lists |
| GET/POST | `…/reference-lists/{id}/entries` | viewer / analyst | upsert `{entries:[{key, attributes?, valid_from?, valid_until?, reason?}]}` |
| POST | `…/reference-lists/{id}/import` | analyst | multipart CSV (first column = key) |
| DELETE | `…/reference-lists/{id}/entries/{entry_id}` | analyst | |
| POST | `/api/v1/projects/{pid}/formulas/evaluate` | viewer | `{expr, variables}` → `{value}` \| `{trapped:true, reason}`; parse error → 422 `{message, position}` |
| GET | `/api/v1/projects/{pid}/proposals[/{id}]` | viewer | |
| POST | `/api/v1/projects/{pid}/proposals` | analyst or INT | `{source, proposal_type, target_rule_id?, definition, rationale, citations, evidence, report_id?, llm_model?}`. Validated and backtested (last 30 days) on create; results stored in `validation` and `backtest` |
| POST | `/api/v1/projects/{pid}/proposals/{id}/{approve\|reject}` | approver | approve new/modify → rule version in **shadow** (activation is a separate approval); retire → retired |

## C. graph-service (`graph-service:8082`)

| Method | Path | Auth | Notes |
|---|---|---|---|
| POST | `/v1/projects/{pid}/links` | INT | `{customer:{id, external_id, risk_label, email?, phone?}, event:{id, occurred_at, device_id?, ip_address?, instrument_fingerprint?, card_bin?, card_last4?, recipient_fingerprint?, shipping_address?, billing_address?, ref_transaction_id?, api_client_id?}}` → `{entity_ids, new_links, similarity_links_created}` (idempotent) |
| POST | `/v1/projects/{pid}/metrics` | INT | `{customer_id, link_kinds?, include_similar?, max_depth?}` (defaults from `graph_config`) → `{distance_to_fraud (null = none within depth), fraud_neighbors_1, fraud_neighbors_2, component_size, shared_entity_count, degree, community_fraud_rate?, shared_with_fraud_kinds[]}` (rule-dsl §5 `graph.*`) |
| POST | `/v1/projects/{pid}/metric` | INT | a single graph-rule metric `{customer_id, metric, link_kinds, include_similar, max_depth}` → `{value}` (used by rule-service for `graph` rules) |
| PUT | `/v1/projects/{pid}/customers/{cid}/label` | INT | `{risk_label}` |
| GET | `/api/v1/projects/{pid}/graph/customers/{cid}/neighborhood` | viewer | `depth≤3, link_kinds, include_similar, limit_nodes≤300` → `{nodes:[{id, type, label, kind?, risk_label?, is_center?}], edges:[{id, source, target, kind, similarity?}]}` |
| GET | `/api/v1/projects/{pid}/graph/customers/{cid}/fraud-proximity` | viewer | `max_depth, link_kinds, include_similar` → `{distance, path, nearest_fraud_customer_id, fraud_within:{"1":n,"2":n,"3":n}}` |
| GET | `/api/v1/projects/{pid}/graph/components` | viewer | `min_size, only_with_fraud` → `[{component_id, size, fraud_count, fraud_rate, sample_customer_ids}]` (cached 5 min) |
| GET | `/api/v1/projects/{pid}/graph/stats` | viewer | `{customers, entities, links, similarity_links, supernodes:[…]}` |
| GET | `/api/v1/projects/{pid}/graph/search` | viewer | `q` → customers/entities by external_id / masked value |
| GET | `/v1/projects/{pid}/export` | INT | streamed NDJSON of customer–customer weighted edges (projection through shared entities) for Louvain in ml-service |

## D. ml-service (`ml-service:8001`)

| Method | Path | Auth | Notes |
|---|---|---|---|
| GET | `/api/v1/ml/algorithms` | JWT | plugin catalogue `[{name, kind, version, display_name, description, param_schema, source, status, error}]` |
| POST | `/api/v1/ml/algorithms/reload` | PA or INT | rescans the plugin dir → `{loaded, invalid:[{module, error}]}` |
| POST | `/v1/algorithms/validate-config` | INT | `{ml_config}` → `{valid, errors}` |
| GET | `/api/v1/projects/{pid}/ml/models` | viewer | `kind` filter |
| GET | `/api/v1/projects/{pid}/ml/models/{id}` | viewer | metrics, history, feature names, algorithms |
| POST | `/api/v1/projects/{pid}/ml/supervised/train` | analyst | `{algorithm?, params?, since_days? (1–3650, default all history), label_maturity_days? (0–365, default 14; unlabelled events older than this count as legit, 0/null = labelled events only)}` → 202 `{model_id, version, status:"training"}` |
| POST | `/api/v1/projects/{pid}/ml/unsupervised/train` | analyst | `{anomaly_algorithm?, anomaly_params?, clustering_algorithm?, clustering_params?, since_days? (default 90)}` → 202 `{model_id, version, status}` |
| POST | `/api/v1/projects/{pid}/ml/models/{id}/{submit\|approve\|reject}` | analyst / approver | approve → `active` (archives previous) + hot swap |
| POST | `/v1/projects/{pid}/supervised/predict` | INT | `{event_id?, features, source?, explain?}` → `{model_id, model_version, algorithm, fraud_probability, top_features}`; 404 `no_active_model` |
| POST | `/v1/projects/{pid}/supervised/predict/batch` | INT | `{items:[{event_id?, features, source?}] (≤ 5000)}` → `{model_id, model_version, algorithm, results:[{event_id, fraud_probability}]}` |
| POST | `/v1/projects/{pid}/unsupervised/score` | INT | `{event_id?, features, source?}` → `{model_id, model_version, anomaly_score, cluster_id, cluster_fraud_rate}` |
| GET | `/api/v1/projects/{pid}/ml/unsupervised/clusters` | viewer | `model_id?` (default active) |
| PATCH | `/api/v1/projects/{pid}/ml/unsupervised/clusters/{model_id}/{cluster_id}` | analyst | `{label, notes}` |
| GET | `/api/v1/projects/{pid}/ml/unsupervised/projection` | viewer | `model_id?, limit≤5000` → `[{event_id, x, y, cluster_id, anomaly_score, label?}]` |
| GET | `/api/v1/projects/{pid}/ml/unsupervised/anomalies` | viewer | `model_id?, limit, min_score` |
| GET | `/api/v1/projects/{pid}/ml/graph-communities` | viewer | `min_size` |
| POST | `/api/v1/projects/{pid}/ml/graph-communities/recompute` | analyst | Louvain over the graph-service export |

## E. llm-service (`llm-service:8002`)

| Method | Path | Auth | Notes |
|---|---|---|---|
| GET/POST | `/api/v1/tenants/{tid}/regulations` | JWT (any member of the tenant) / TA or analyst+ in any project | multipart `file` + `{code, title, issuer, doc_type: regulation\|internal_policy\|sop\|other, effective_date?, supersedes_id?}` → 202 `{regulation_id, status:"processing"}` |
| GET/DELETE | `/api/v1/tenants/{tid}/regulations/{id}` | JWT / TA | detail includes `changes` if it supersedes another |
| GET/PUT | `/api/v1/projects/{pid}/llm/regulations` | viewer / project_admin | attached regulation ids |
| POST | `/api/v1/projects/{pid}/llm/regulations/search` | viewer | `{query, k}` (restricted to attached docs) |
| POST | `/api/v1/projects/{pid}/llm/chat` | viewer | `{conversation_id?, message}` → `{conversation_id, answer, tool_calls, citations, proposals}` |
| POST | `/api/v1/projects/{pid}/llm/chat/stream` | viewer | SSE: `event: conversation\|token\|tool\|citation\|done\|error` (`done` carries the same body as the non-streaming call) |
| GET | `/api/v1/projects/{pid}/llm/conversations[/{id}]` | viewer | own conversations |
| POST | `/api/v1/projects/{pid}/llm/analysis/{rule-relevance\|fraud-situation\|regulation-impact\|recommend-rules}` | analyst | async (202) → `{report_id}`; the report is schema-constrained JSON; `recommend-rules` also creates pending proposals in rule-service |
| GET | `/api/v1/projects/{pid}/llm/reports[/{id}]` | viewer | |
| POST | `/v1/mapping/suggest` | INT | `{fields, canonical_fields}` → `{suggestions:[{source_path, target, confidence, reason}]}` |

LLM tools (14; function calling, INT token + tenant/project headers, read-only except `create_rule_proposal`):
`search_regulations`, `list_rules`, `get_rule`, `get_rules_performance`, `backtest_rule_definition`,
`validate_rule_definition`, `get_analytics_overview`, `get_feature_drift`, `get_typology_stats`,
`get_anomaly_clusters`, `get_graph_components`, `get_graph_communities`, `get_project_context`,
`create_rule_proposal`.

## F. ingest-service (`ingest-service:8003`)

| Method | Path | Auth | Notes |
|---|---|---|---|
| POST | `/api/v1/projects/{pid}/data-sources/{id}/inspect` | analyst | multipart `file` \| JSON `{sql:{table?, query?, limit?}}` \| `{records}` → `{upload_id?, schema:{fields}, suggested_mapping, confidence, preview}` (also stores `inferred_schema`) |
| POST | `/api/v1/projects/{pid}/data-sources/{id}/jobs` | analyst | `{upload_id? , mode}` → `{job_id}` |
| GET | `/api/v1/projects/{pid}/data-sources/{id}/jobs` | viewer | |
| GET | `/api/v1/projects/{pid}/ingest-jobs/{job_id}` | viewer | progress |
| POST | `/api/v1/projects/{pid}/ingest-jobs/{job_id}/cancel` | analyst | |
