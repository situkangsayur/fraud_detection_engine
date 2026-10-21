# Multi-tenancy & projects

> Document status: verified against commit `07ee765` on 2026-10-13.

## 1. Model

```
platform
 ├─ platform admins                      core.app_users (tenant_id NULL, is_platform_admin)
 ├─ audit trail                          core.audit_log (append-only, tenant_id NULL = platform action)
 └─ tenant (company)                     core.tenants (status active|suspended)
     ├─ users (tenant_admin | member)    core.app_users, core.refresh_tokens
     ├─ regulation/policy library        llm.regulations, llm.regulation_changes (+ OpenSearch index reg-chunks-<tenant_id>)
     ├─ tenant-wide reference lists      rules.reference_lists (project_id IS NULL), rules.reference_entries
     └─ projects                         core.projects
         ├─ members + role               core.project_members (project_admin|approver|analyst|viewer)
         ├─ settings                     core.project_settings (thresholds, engine weights, graph scores, timeouts…)
         ├─ data sources + mappings      core.data_sources, core.data_source_mappings, core.field_catalog,
         │                               core.ingest_errors, ingest.jobs, ingest.uploads
         ├─ customers, events, features  core.customers, core.events, core.event_features
         ├─ decisions, cases, labels     core.decisions, core.cases, core.labels (+ view core.event_labels)
         ├─ approvals                    core.approvals (rule, ruleset, model, proposal; mapping reserved)
         ├─ rules, rulesets, ref lists   rules.rules, rules.rule_versions, rules.rulesets, rules.ruleset_versions,
         │                               rules.ruleset_rules, rules.reference_lists, rules.rule_hits,
         │                               rules.rule_eval_counters, rules.proposals
         ├─ graph data set               graph.nodes_customer, graph.entities, graph.entity_links, graph.entity_similarity
         ├─ ML config + models           core.projects.ml_config, ml.models, ml.clusters, ml.event_anomaly,
         │                               ml.graph_communities, ml.graph_community_stats
         └─ LLM config + attached regs   core.projects.llm_config, llm.project_regulations, llm.reports,
                                         llm.conversations, llm.messages
```

`ml.algorithms` (the plugin registry) is platform-wide.

Each project is an isolated fraud "workspace": its own customers, graph and models. Two projects of the same
tenant never share events or graph links. **Sharing across projects is explicit:**
* tenant-wide reference lists (e.g. a company blacklist), usable from rules in any project of the tenant
  (name resolution: project list first, then the tenant-wide list with the same name);
* the tenant regulation library, where each project attaches the documents it follows;
* labels do **not** propagate automatically. Cross-project fraud propagation (e.g. a customer confirmed as fraud in
  `returns` is flagged in `pre_payment`) is on the backlog as an explicit opt-in "linked projects" feature.

## 2. Project definition

`core.projects` columns: `id, tenant_id, slug (unique per tenant, ^[a-z0-9][a-z0-9-]{1,62}$), name, description,
stage (pre_payment|post_payment|returns|promo|account_security|payout|custom), business_context` (free text: what is
protected, the business flow, and the decision point; this goes into LLM prompts), `timezone` (default
`Asia/Jakarta`), `currency` (default `IDR`), `status (active|archived)`, `ml_config`, `llm_config`, `graph_config`,
`created_by`, `created_at`, `updated_at`.

```jsonc
// ml_config (column default shown; params are validated against each plugin's param_schema)
{ "supervised":   { "algorithm": "mlp_backprop", "params": {} },
  "unsupervised": { "anomaly_algorithm": "isolation_forest", "anomaly_params": {},
                    "clustering_algorithm": "hdbscan", "clustering_params": {} },
  "features": { "include": ["*"], "exclude": [], "extra_source_fields": [] } }
// e.g. "params": { "hidden_layers": [64, 32] }, "extra_source_fields": ["source.order.item_count"]

// llm_config — chat_model null = the service default (OLLAMA_CHAT_MODEL, or GEMINI_MODEL with LLM_PROVIDER=gemini)
{ "chat_model": null, "temperature": 0.1, "language": "id", "system_prompt_extra": "" }
// e.g. "system_prompt_extra": "Fokus pada abuse voucher untuk flash sale."

// graph_config (column default)
{ "link_kinds": ["email","phone","device","card","bank_account","address","ref_transaction"],
  "include_similar": true, "max_depth": 3, "supernode_degree_cap": 50, "similarity_threshold": 0.85 }
```

`link_kinds` may also contain `ip` and `api_client`; they are excluded by default because shared NATs and API
clients create noisy links. graph-service caches each project's `graph_config` for 60 s.

Algorithm names must exist in `ml.algorithms` (the plugin registry) and params must validate against the plugin's
`param_schema`. ml-service validates both, and core-api calls ml-service `POST /v1/algorithms/validate-config` on
save.

**Stage templates:** creating a project bootstraps rule-service templates (`POST /v1/projects/{pid}/bootstrap
{template}`; files `services/rust/crates/rule-service/templates/<stage>.json`). The `template` field defaults to the
stage (none for `custom`); `"none"` skips it. Bootstrap is retried with backoff, idempotent, and survives restarts
(bookkeeping row `template_bootstrap` in `core.project_settings`). Templates create reference lists, rules and
rulesets (status `active`); they do not change project settings:

| Template | Rulesets | Reference lists |
|---|---|---|
| `pre_payment` | blocklists & whitelists, carding, ATO at checkout, network & model | `card_blacklist`, `device_blacklist`, `vip_whitelist`, `merchant_limits` |
| `post_payment` | fulfilment checks, payout checks | `mule_accounts`, `merchant_limits` |
| `returns` | refund abuse | `serial_returners` |
| `promo` | promo abuse | `promo_device_blacklist` |
| `account_security` | login protection, account changes | `device_blacklist` |
| `payout` | payout protection | `mule_accounts` |

## 3. Database isolation (defence in depth)

1. Every tenant-owned table has `tenant_id uuid NOT NULL`. Project-owned tables also have `project_id uuid NOT NULL`
   with a composite FK `(tenant_id, project_id) → core.projects(tenant_id, id)`, so a row can never point to another
   tenant's project. Deleting a tenant cascades to its projects and their data.
2. **Row-Level Security** is enabled on every tenant table:
   `USING (tenant_id = core.current_tenant()) WITH CHECK (tenant_id = core.current_tenant())`.
   `core.current_tenant()` = `NULLIF(current_setting('app.tenant_id', true), '')::uuid`. An unset tenant matches **no
   rows** (fails closed). The view `core.event_labels` is `security_invoker`, so RLS of the underlying tables applies.
3. Services connect as non-owner roles (`core_api`, `rule_service`, `graph_service`, `ml_service`, `llm_service`,
   `ingest_service`), so RLS always applies. The `migrator` role owns the schemas and is used only by the migrate
   job. Cross-schema reads are explicit grants (architecture §2.3).
4. Every request/job runs in a transaction that begins with
   `SELECT set_config('app.tenant_id', $1, true)` (transaction-local, which is pool-safe). The Rust `platform` crate
   exposes `TenantTx::begin(&pool, tenant_id)`; the Python services use a `tenant_session(tenant_id)` context
   manager. **Queries still filter by `project_id` explicitly.** RLS is the safety net, not the primary filter.
5. Cross-tenant platform operations touch only tables without RLS: `core.tenants`, `core.app_users`,
   `core.refresh_tokens`, and `core.audit_log` (whose readers must filter by tenant/project explicitly). Two
   `SECURITY DEFINER` functions return routing ids only:
   * `core.resolve_source_key(prefix)` → `(tenant_id, project_id, data_source_id, slug, api_key_hash, is_active)`
     for webhook API-key resolution (core-api);
   * `core.list_pollable_sources()` → `(tenant_id, project_id, data_source_id)` of active Postgres/MySQL sources with
     polling enabled (ingest-service poller, migration `0008`); connection details are then read under the tenant's
     RLS context.

## 4. Vector DB isolation

One OpenSearch index per tenant: `reg-chunks-<tenant_id>` (prefix `OPENSEARCH_INDEX_PREFIX`). Each chunk carries
`regulation_id`, `code`, `version`, `section`, `issuer`, `effective_date` and the embedding. Project-scoped retrieval
adds a `terms` filter on the project's attached `regulation_id`s (`llm.project_regulations`). Deleting a regulation deletes its chunks. There is no
tenant-delete API (tenants are suspended via `PATCH /tenants/{tid}`); the operator script
`deploy/demo/wipe-tenant.sh` deletes a tenant's rows and drops its index.

## 5. Identity

* `core.app_users.email` is globally unique (`citext`). A user belongs to exactly one tenant (`tenant_id`), except
  platform admins (`tenant_id` NULL, `is_platform_admin`).
* JWT claims: `sub, tid, trole, padmin, prj{pid: role}, exp, iat, jti`. When a user's project memberships change,
  their refresh token must be exchanged for a new access token (at most `JWT_TTL_MINUTES`, default 60 min, of
  staleness).
* Platform admins manage tenants but have **no implicit access to project data** (privacy by default). Project
  endpoints return 404 to them unless they are explicitly made project members.
* Deleting a user keeps history: "who" columns (`created_by`, `decided_by`, `assigned_to`, …) become NULL
  (migration `0009`), memberships and refresh tokens cascade, and the audit log keeps the actor id as text.
* Bootstrap: on first start, core-api creates the platform admin from `ADMIN_EMAIL`/`ADMIN_PASSWORD`, plus, when
  `SEED_DEMO=true` (code default `false`, compose default `true`), a demo tenant `demo` with projects `checkout`
  (pre_payment), `post-payment` (post_payment), `returns` (returns) and `promo` (promo), with template rulesets and
  demo users `analyst@demo.local` / `approver@demo.local` (password = `DEMO_USER_PASSWORD`).
* Seeding scripts can add more tenants for experiments (`deploy/seed.sh research <dataset>` creates `exp-sparkov`,
  `exp-paysim`, `exp-saml-d`), each with `sim-admin@<tenant>.local`, `analyst@<tenant>.local` and
  `approver@<tenant>.local`. `deploy/demo/wipe-tenant.sh <slug>` deletes one tenant and all of its data. The login
  page can show demo hints per tenant (`DEMO_LOGIN_HINTS` → `NUXT_PUBLIC_DEMO_ACCOUNTS`). See
  [`../guides/experiments.md`](../guides/experiments.md).
