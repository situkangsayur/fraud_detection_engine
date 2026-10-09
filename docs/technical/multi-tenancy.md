# Multi-tenancy & projects

## 1. Model

```
platform
 └─ tenant (company)                     core.tenants
     ├─ users (tenant_admin | member)    core.app_users
     ├─ regulation/policy library        llm.regulations  (+ OpenSearch index reg-chunks-<tenant_id>)
     ├─ tenant-wide reference lists      rules.reference_lists (project_id IS NULL)
     └─ projects                         core.projects
         ├─ members + role               core.project_members (project_admin|approver|analyst|viewer)
         ├─ settings                     core.project_settings (thresholds, engine weights, graph, timeouts…)
         ├─ data sources + mappings      core.data_sources, core.data_source_mappings
         ├─ customers, events, features  core.customers, core.events, core.event_features
         ├─ decisions, cases, labels     core.decisions, core.cases, core.labels
         ├─ rules, rulesets, ref lists   rules.*
         ├─ graph data set               graph.entities, graph.entity_links, graph.entity_similarity
         ├─ ML config + models           core.projects.ml_config, ml.models, ml.* results
         └─ LLM config + attached regs   core.projects.llm_config, llm.project_regulations, llm.reports, llm.conversations
```

Each project is an isolated fraud "workspace": its own customers, graph and models. Two projects of the same
tenant never share events or graph links. **Sharing across projects is explicit:**
* tenant-wide reference lists (e.g. a company blacklist), usable from rules in any project of the tenant;
* the tenant regulation library, where each project attaches the documents it follows;
* labels do **not** propagate automatically. Cross-project fraud propagation (e.g. a customer confirmed as fraud in
  `returns` is flagged in `pre_payment`) is on the backlog as an explicit opt-in "linked projects" feature.

## 2. Project definition

`core.projects` columns: `id, tenant_id, slug (unique per tenant), name, description, stage, business_context`
(free text: what is protected, the business flow, and the decision point; this goes into LLM prompts),
`timezone` (default `Asia/Jakarta`), `currency` (default `IDR`), `status (active|archived)`, `ml_config`,
`llm_config`, `graph_config`, and audit columns.

```jsonc
// ml_config
{ "supervised":   { "algorithm": "mlp_backprop", "params": { "hidden_layers": [64, 32] } },
  "unsupervised": { "anomaly_algorithm": "isolation_forest", "anomaly_params": {},
                    "clustering_algorithm": "hdbscan", "clustering_params": { "min_cluster_size": 15 } },
  "features": { "include": ["*"], "exclude": [], "extra_source_fields": ["source.order.item_count"] } }
// llm_config
{ "chat_model": "qwen2.5:7b-instruct", "temperature": 0.1, "language": "id",
  "system_prompt_extra": "Fokus pada abuse voucher untuk flash sale." }
// graph_config
{ "link_kinds": ["email","phone","device","card","bank_account","address","ref_transaction"],
  "include_similar": true, "max_depth": 3, "supernode_degree_cap": 50, "similarity_threshold": 0.85 }
```

Algorithm names must exist in `ml.algorithms` (the plugin registry) and params must validate against the plugin's
`param_schema`. ml-service validates both, and core-api calls ml-service `POST /v1/algorithms/validate-config` on save.

**Stage templates:** creating a project with a `stage` optionally bootstraps typical rulesets, reference lists and
settings from rule-service templates (`POST /v1/projects/{pid}/bootstrap {template}`), e.g.:
* `pre_payment`: carding, ATO checkout, velocity, blacklist;
* `post_payment`: shipping mismatch, amount anomaly, mule payout;
* `returns`: serial returner, return-after-delivery window, linked accounts;
* `promo`: multi-account promo farming, device reuse;
* `account_security`: failed logins, new device + credential change.

## 3. Database isolation (defence in depth)

1. Every tenant-owned table has `tenant_id uuid NOT NULL`. Project-owned tables also have `project_id uuid NOT NULL`
   with a composite FK `(tenant_id, project_id) → core.projects(tenant_id, id)`, so a row can never point to another
   tenant's project.
2. **Row-Level Security** is enabled on every tenant table:
   `USING (tenant_id = core.current_tenant()) WITH CHECK (tenant_id = core.current_tenant())`.
   `core.current_tenant()` = `NULLIF(current_setting('app.tenant_id', true), '')::uuid`. An unset tenant matches **no
   rows** (fails closed).
3. Services connect as non-owner roles, so RLS always applies. The `migrator` role owns the schemas and is used only
   by the migrate job.
4. Every request/job runs in a transaction that begins with
   `SELECT set_config('app.tenant_id', $1, true)` (transaction-local, which is pool-safe). The Rust `platform` crate
   exposes `TenantTx::begin(&pool, tenant_id)`; the Python services use a `tenant_session(tenant_id)` context
   manager. **Queries still filter by `project_id` explicitly.** RLS is the safety net, not the primary filter.
5. Cross-tenant platform operations (tenant CRUD by platform admins) touch only `core.tenants` and `core.app_users`,
   which have no RLS. Webhook API-key resolution uses the `SECURITY DEFINER` function
   `core.resolve_source_key(prefix)`, which returns only `(tenant_id, project_id, data_source_id, api_key_hash, slug)`.

## 4. Vector DB isolation

One OpenSearch index per tenant: `reg-chunks-<tenant_id>`. Each chunk carries `regulation_id`, `code`, `version`,
`section`, `issuer`, `effective_date` and the embedding. Project-scoped retrieval adds a `terms` filter on the project's
attached `regulation_id`s (`llm.project_regulations`). Deleting a tenant deletes its index.

## 5. Identity

* `core.app_users.email` is globally unique. A user belongs to exactly one tenant (`tenant_id`), except platform
  admins (`tenant_id` NULL, `is_platform_admin`).
* JWT claims: `sub, tid, trole, padmin, prj{pid: role}, exp, iat, jti`. When a user's project memberships change,
  their refresh token must be exchanged for a new access token (at most 60 min of staleness, documented).
* Platform admins manage tenants but have **no implicit access to project data** (privacy by default). Project endpoints
  return 404 to them unless they are explicitly made project members.
* Bootstrap: on first start, core-api creates the platform admin from `ADMIN_EMAIL`/`ADMIN_PASSWORD`, plus, when
  `SEED_DEMO=true`, a demo tenant `demo` with projects `checkout` (pre_payment), `post-payment` (post_payment),
  `returns` (returns) and `promo` (promo), with template rulesets and demo users
  `analyst@demo.local` / `approver@demo.local` (password = `DEMO_USER_PASSWORD`).
