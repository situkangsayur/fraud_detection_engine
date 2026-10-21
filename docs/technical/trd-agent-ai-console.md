# TRD: Agent & AI Console (agent registry, multi-model, provider credentials)

Status: **design only, not implemented**. Written on 2026-10-21 against `revamp/rust-nuxt-stack` @ `07ee765`.
Product requirements: [`../business/prd-agent-ai-console.md`](../business/prd-agent-ai-console.md) (FR-n numbers below refer
to it). Builds on [`multi-agent.md`](multi-agent.md) (agent roster, orchestrator, budgets) and
[`llm-agentic.md`](llm-agentic.md) (today's single agent). Backlog: epic **E18** (this document) and **E16** (multi-agent).

## 0. Summary

Move LLM configuration from **process environment** to **tenant-scoped data** owned by llm-service:

* **Providers.** Ollama, Gemini, and any later OpenAI-compatible endpoint, each with optional encrypted credentials.
* **Models.** A curated catalog per provider, with capabilities.
* **Agent profiles.** Each agent gets a primary model, an ordered fallback chain, parameters, a tool subset and a kill switch.

The settings are resolved as **project → tenant → platform → env**. A `ChatClientRegistry` hands each call the client
for the resolved model, so different agents in one run can use different providers. The `.env` values stay the last
fallback, so an empty database behaves exactly like today.

## 1. Current state (verified in code)

| Area | Today | File |
|---|---|---|
| Provider selection | `Settings.llm_provider: "ollama"\|"gemini"`. `Container.build` creates **one** chat client (`GeminiClient` or `OllamaClient`) and shares it with chat, analysis, proposals and regulations | `config.py`, `container.py` |
| Embeddings | always Ollama `bge-m3` (`GeminiClient` delegates embeddings) | `clients/gemini.py` |
| Model per project | `core.projects.llm_config.chat_model` (jsonb, default `null`) overrides `settings.chat_model` in `ProjectContextLoader.load` | `project_context.py`, migration `0001` |
| Model per project (UI) | free text, only on `projects/new.vue`, **default `qwen2.5:7b-instruct`** (not installed on nvda11-gpu, which has `qwen3:8b`) | `web/app/pages/projects/new.vue:32` |
| `GeminiClient` model guard | the project model is used only if it starts with `gemini`, otherwise falls back to `GEMINI_MODEL` | `clients/gemini.py:92` |
| Secrets | `GEMINI_API_KEY` in `.env`; no secret is stored in the DB anywhere in the platform | — |
| Agent | one `run_agent` loop, 14 tools, `chat_max_tool_iterations`; no per-agent identity | `agent/loop.py`, `agent/tools.py` |
| Token usage | not recorded (Ollama returns `prompt_eval_count`/`eval_count`) | `clients/ollama.py` |
| Audit | services `INSERT` into `core.audit_log` directly (grant in `0007`) | `0007_rls_and_grants.sql` |

## 2. Design decisions

| # | Decision | Rationale |
|---|---|---|
| D1 | llm-service owns the new tables (schema `llm`) and endpoints; core-api is not involved except for JWT/roles | config and its consumers live in one service; same pattern as regulations |
| D2 | **Layered scopes**: `platform` (tenant_id NULL), `tenant`, `project`; the most specific non-null field wins **per field**, not per row | a project can override only the model while inheriting temperature |
| D3 | `env` is the implicit bottom layer | zero-config deployments and tests keep working; rollback = empty tables |
| D4 | One protocol `ChatClient` (`chat`, `structured_chat`, `stream`, `aclose`); `OllamaClient`/`GeminiClient` already match it; new `OpenAICompatClient` generalises `GeminiClient` | multi-provider without a framework (multi-agent.md §2) |
| D5 | Credentials: AES-256-GCM, 96-bit random nonce, key `LLM_SECRETS_KEY` (32 bytes base64, env), AAD = `provider_id`; `key_version` column for rotation | secrets never in plain text at rest; a dump without the env key is useless |
| D6 | Credentials are **write-only** over the API; responses carry `{"set": true, "last4": "abcd", "updated_at", "updated_by"}` | FR-3 |
| D7 | Config cache in llm-service with TTL 30 s **plus** `LISTEN/NOTIFY llm_config_changed` invalidation | FR T1 (≤ 60 s) without a restart; NOTIFY makes it near-instant |
| D8 | Prompt and tool-subset changes create a **pending version** that needs approval by a different user (maker–checker, same rule as `proposal::review`); model/params/kill switch apply immediately and are audited | PRD §5 |
| D9 | Tool subsets can only **narrow** the code-defined `AgentSpec.max_tools`; `create_rule_proposal` is never assignable | least privilege; multi-agent.md §6 |
| D10 | Embedding model stays env-only | changing it requires re-indexing OpenSearch (out of scope) |
| D11 | External providers (`kind != ollama` or a non-private URL) are blocked for a tenant until `tenant_ai_policy.allow_external = true` | UU PDP; PRD T5 |

## 3. Data model (migration `0013_llm_ai_console.sql`)

> multi-agent.md plans `0013_llm_agent_runs.sql`. Whichever lands first takes `0013`; the other becomes `0014`.

```sql
CREATE TABLE llm.ai_providers (
  id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  tenant_id     uuid NULL REFERENCES core.tenants(id),          -- NULL = platform provider
  name          text NOT NULL,                                  -- "nvda11-gpu ollama", "gemini (tenant)"
  kind          text NOT NULL CHECK (kind IN ('ollama','gemini','openai_compat')),
  base_url      text NOT NULL,
  is_external   boolean NOT NULL,                               -- computed on write (kind/url), drives D11
  enabled       boolean NOT NULL DEFAULT true,
  options       jsonb NOT NULL DEFAULT '{}',                    -- timeout_s, num_parallel, headers (non-secret)
  last_test     jsonb NULL,                                     -- {ok, at, version, latency_ms, error}
  created_by uuid, created_at timestamptz DEFAULT now(), updated_at timestamptz DEFAULT now(),
  UNIQUE (tenant_id, name)
);

CREATE TABLE llm.ai_credentials (
  provider_id   uuid PRIMARY KEY REFERENCES llm.ai_providers(id) ON DELETE CASCADE,
  tenant_id     uuid NULL,                                      -- copy for RLS
  ciphertext    bytea NOT NULL,
  nonce         bytea NOT NULL,
  key_version   smallint NOT NULL DEFAULT 1,
  last4         text NOT NULL,
  updated_by uuid, updated_at timestamptz DEFAULT now()
);

CREATE TABLE llm.ai_models (
  id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  provider_id   uuid NOT NULL REFERENCES llm.ai_providers(id) ON DELETE CASCADE,
  tenant_id     uuid NULL,
  model         text NOT NULL,                                  -- "qwen3:8b", "gemini-flash-latest"
  enabled       boolean NOT NULL DEFAULT false,                 -- discovered ≠ allowed (FR-5)
  capabilities  jsonb NOT NULL DEFAULT '{}',                    -- {tools, json_schema, thinking, ctx_max, vram_gb, embed}
  defaults      jsonb NOT NULL DEFAULT '{}',                    -- {num_ctx, temperature, think}
  UNIQUE (provider_id, model)
);

CREATE TABLE llm.agent_profiles (                               -- one row per (scope, agent)
  id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  tenant_id     uuid NULL,
  project_id    uuid NULL REFERENCES core.projects(id),
  agent         text NOT NULL,                                  -- registry key: chat, analysis, regulation, analytics,
                                                                -- pattern, designer, critic, reporter, router, '*'
  model_id      uuid NULL REFERENCES llm.ai_models(id),
  fallback_ids  uuid[] NOT NULL DEFAULT '{}',                   -- ordered (FR-9)
  params        jsonb NOT NULL DEFAULT '{}',                    -- temperature, think, num_ctx, max_iterations, language
  tools         text[] NULL,                                    -- NULL = inherit; else subset of AgentSpec.max_tools
  prompt_extra  text NULL,
  enabled       boolean NULL,                                   -- NULL = inherit (kill switch FR-13)
  version       int NOT NULL DEFAULT 1,
  status        text NOT NULL DEFAULT 'active' CHECK (status IN ('active','pending','superseded')),
  created_by uuid, approved_by uuid, created_at timestamptz DEFAULT now(),
  CHECK (project_id IS NULL OR tenant_id IS NOT NULL)
);
CREATE UNIQUE INDEX ux_agent_profile_active
  ON llm.agent_profiles (coalesce(tenant_id,'00000000-0000-0000-0000-000000000000'),
                         coalesce(project_id,'00000000-0000-0000-0000-000000000000'), agent)
  WHERE status = 'active';

CREATE TABLE llm.tenant_ai_policy (
  tenant_id       uuid PRIMARY KEY REFERENCES core.tenants(id),
  allow_external  boolean NOT NULL DEFAULT false,
  ack_by uuid, ack_at timestamptz,                              -- data-processing acknowledgement
  agents_enabled  boolean NOT NULL DEFAULT true,
  monthly_token_cap bigint NULL
);

CREATE TABLE llm.llm_calls (                                    -- usage (FR-16/17); partition by month later
  id bigserial PRIMARY KEY, at timestamptz NOT NULL DEFAULT now(),
  tenant_id uuid NOT NULL, project_id uuid NULL, agent text NOT NULL, run_id uuid NULL,
  provider_id uuid NULL, model text NOT NULL, fallback_from text NULL,
  prompt_tokens int, completion_tokens int, latency_ms int, outcome text NOT NULL   -- ok|error|timeout|denied
);
```

RLS follows the rest of the platform (`multi-tenancy.md` §3), with one difference: **rows with `tenant_id IS NULL` are
readable by every tenant session**, because platform providers and models are shared. Only the platform-admin path can
write them, through a dedicated `llm_service_admin` code path that runs without `app.tenant_id` and checks
`padmin` in the JWT. `ai_credentials` is never selected into API responses. Grants: `llm_service` gets
`SELECT/INSERT/UPDATE` on the new tables and `INSERT` on `llm_calls`. The web/BFF has no DB access.

The existing `core.projects.llm_config.chat_model` is migrated into a project-scope `agent_profiles` row for agent
`'*'`, but only when the model exists in `ai_models`. The field then becomes read-only (deprecated) and is removed after one release.

## 4. Resolution

```
resolve(tenant, project, agent) -> EffectiveAgentConfig
  layers = [project/agent, project/'*', tenant/agent, tenant/'*', platform/agent, platform/'*', env]
  for each field (model_id, fallback_ids, params.*, tools, prompt_extra, enabled):
      value = first non-null across layers
      source[field] = layer name               # shown in UI ("from: platform default")
  enabled = AND(global kill, tenant_ai_policy.agents_enabled, project agents_enabled, resolved enabled)
  drop models whose provider is disabled, not enabled, or external while allow_external = false
  tools   = resolved ∩ AgentSpec.max_tools  (create_rule_proposal always removed)
```

`EffectiveAgentConfig` is cached per `(tenant, project, agent)`. A cache entry is invalidated by `NOTIFY` (payload
`tenant_id`) or after its TTL expires. `GET .../agents/{agent}/effective` returns the resolved config together with `source`, for UI transparency.

## 5. Runtime changes in llm-service

| Change | Detail |
|---|---|
| `clients/base.py` | `ChatClient` Protocol + `ChatResult{content, tool_calls, usage{prompt,completion}, model}`; Ollama client reads `prompt_eval_count`/`eval_count` |
| `clients/openai_compat.py` | generalise `GeminiClient` (base_url, api key, model list via `GET /models`); `GeminiClient` becomes a thin subclass |
| `clients/registry.py` | `ChatClientRegistry.get(provider_id) -> ChatClient`; one pooled `httpx.AsyncClient` per provider, rebuilt when the provider row or credential changes; decrypts the key only in memory |
| `ai_config/` (new) | `repository.py` (SQL), `crypto.py` (AES-GCM, `cryptography` lib), `resolver.py` (§4), `cache.py` (TTL + LISTEN task) |
| `agent/registry.py` | `AgentSpec{name, purpose, max_tools, default_prompt, default_params}` (code), merged with `EffectiveAgentConfig` (data) |
| `run_agent(...)` | takes `agent_name`; the model client comes from the resolver. **Fallback:** on timeout, 5xx, connection error, or unsupported tool calling, try the next model in `fallback_ids` once per call; record `fallback_from` |
| `ProjectContextLoader` | drops `llm_config.chat_model` once migrated; keeps language/temperature as `'*'` params |
| Usage | every chat/structured call writes one `llm_calls` row (async, batched); Prometheus `llm_calls_total{provider,model,agent,outcome}`, `llm_tokens_total{direction}`, `llm_call_seconds` |
| GPU guard | a process-wide semaphore per **local** provider (`options.num_parallel`, default 1), so concurrent agents on one 8 GB GPU are serialised (multi-agent.md §8); cloud providers have no such limit |
| Budgets | `tenant_ai_policy.monthly_token_cap` checked before each call, using a sum over `llm_calls` cached for 60 s; on breach → `denied` |

Back-compat: if `llm.ai_providers` is empty, the registry synthesises one provider from env (`LLM_PROVIDER`,
`OLLAMA_URL`/`GEMINI_*`), which matches today's behaviour.

## 6. API (gateway → llm-service, prefix `/api/v1`)

| Method | Path | Auth | Notes |
|---|---|---|---|
| GET/POST | `/ai/providers` | PA | platform providers |
| GET/POST | `/tenants/{tid}/ai/providers` | TA | tenant providers; POST rejected for external kinds while `allow_external=false` |
| PATCH/DELETE | `/ai/providers/{id}` | PA or TA (owner) | `{name, base_url, enabled, options}` |
| PUT/DELETE | `/ai/providers/{id}/credential` | PA or TA (owner) | body `{api_key}`; response `{set, last4, updated_at, updated_by}`; never returns the key |
| POST | `/ai/providers/{id}/test` | PA or TA | `{ok, version, models[], latency_ms, chat_ok, error?}`; stores `last_test` |
| POST | `/ai/providers/{id}/models/refresh` | PA or TA | discover models (`/api/tags`, `/models`) → upsert `enabled=false` |
| GET/PATCH | `/ai/models[/{id}]` | read: any member · write: PA/TA owner | `{enabled, capabilities, defaults}` |
| GET/PUT | `/tenants/{tid}/ai/policy` | TA | `{allow_external, agents_enabled, monthly_token_cap}`; enabling external requires `{ack: true}` |
| GET | `/ai/agents` | any member | code registry (`AgentSpec`): names, purpose, max tools |
| GET/PUT | `/ai/agents/{agent}/profile` · `/tenants/{tid}/ai/agents/{agent}/profile` · `/projects/{pid}/ai/agents/{agent}/profile` | PA · TA · project_admin | PUT of model/params/enabled → active immediately; PUT changing `tools` or `prompt_extra` → `pending` version |
| POST | `/projects/{pid}/ai/agents/{agent}/profile/{version}/approve` (+ tenant/platform variants) | approver (≠ creator) | activates pending, supersedes old |
| GET | `/projects/{pid}/ai/agents/{agent}/effective` | viewer | resolved config + `source` per field (FR-14) |
| GET | `/tenants/{tid}/ai/usage` · `/ai/usage` | TA · PA | `?from&to&group_by=model\|agent\|project\|day` |

Every write inserts into `core.audit_log` (`subject_type` = `ai_provider|ai_credential|ai_model|agent_profile|ai_policy`,
before/after with the credential redacted to `last4`). Errors use RFC 7807. Lists use the standard page envelope.

## 7. UI (Nuxt)

| Route | Who | Content |
|---|---|---|
| `/admin/ai` (tabs Providers · Models · Agents · Usage) | platform admin | platform scope; provider form with *Test connection*; model table with enable toggle, capability chips, VRAM warning (> 7.5 GB on the 8 GB GPU) |
| `/tenant/ai` | tenant admin | external-provider policy with data-processing acknowledgement; tenant providers + write-only key field; tenant agent defaults; usage |
| `/p/[pid]/settings` → section **AI & agents** | project admin | per-agent model + fallback picker (only enabled + allowed models), params, kill switch; each field shows "Effective: … (from …)"; pending approvals |
| `/p/[pid]/llm/chat`, reports, Agent Runs | everyone | badge with the model actually used, plus a fallback marker |

`projects/new.vue`: replace the free-text `chat_model` with the same model picker (default = inherit), which removes
the hard-coded `qwen2.5:7b-instruct`. Components reuse `PagePanel`, `UModal`, `UTable` as in `admin/tenants.vue`.
i18n keys `ai.*` (id/en).

## 8. Security

* `LLM_SECRETS_KEY` is required whenever a credential row exists. Startup fails loudly if credentials exist but the key
  is missing or wrong (GCM tag check on one row).
* Rotation: set `LLM_SECRETS_KEY_NEXT`, run `python -m llm_service.ai_config.rotate` (re-encrypts, bumps
  `key_version`), swap env, restart.
* The key never appears in logs (structlog redaction processor for `api_key`, `authorization`), metrics, `last_test`, or
  errors returned to clients.
* SSRF: `base_url` of **tenant** providers must be `https` and must not resolve to private, loopback or link-local ranges.
  Platform providers may use private addresses (needed for `host.docker.internal:11434`).
* Prompt injection and data minimisation are unchanged (`<tool_result>` wrapping, masked fields).

## 9. Tests

| Level | Cases |
|---|---|
| Unit | resolver layering per field and `source`; tool-subset narrowing and write-tool removal; crypto round-trip, wrong key, tampered AAD; fallback order + `fallback_from`; external block when `allow_external=false`; SSRF validator |
| Integration (Postgres) | RLS: tenant A cannot see B's providers/credentials/profiles but sees platform rows; maker–checker on prompt/tools (self-approval 409); NOTIFY invalidates cache; audit rows redact keys |
| Contract | OpenAPI for all §6 routes; credential GET never contains the key |
| E2E (Playwright) | platform admin enables `qwen3:8b` → project chat uses it without restart; tenant enables Gemini (mocked server) → Critic uses Gemini, forced 500 → fallback badge |
| Mock providers | an Ollama mock and an OpenAI-compatible mock in the test compose profile; no live keys in CI |

## 10. Rollout

1. Migration + read path with env-synthesised provider (no behaviour change). Deploy.
2. Platform admin UI; seed the platform provider "nvda11-gpu ollama" (`http://host.docker.internal:11434`) and enable
   `qwen3:8b`. Then remove `OLLAMA_CHAT_MODEL` reliance from the docs.
3. Tenant/project UI and credentials; migrate `llm_config.chat_model`.
4. Agent registry + per-agent profiles, used by chat and analysis first, and then by the multi-agent flows (E16).
5. Usage page and token caps.

Ops: add `LLM_SECRETS_KEY` to `.env` in the deploy checkout (`~/own_project/deployment/fraud_detection_engine`) and to
`.env.example`. Back up the key outside the server, because losing it means re-entering every API key.

## 11. Open questions

| # | Question | Default if not decided |
|---|---|---|
| Q1 | Should tenant admins see platform-provider usage of their own tenant only, or the platform total? | own tenant only |
| Q2 | Should model enable/disable at **platform** scope also need maker–checker? | no (audit only) |
| Q3 | Add Anthropic Claude as a native client, or only via an OpenAI-compatible gateway? | OpenAI-compatible first |
| Q4 | Cost table for Gemini pricing: static config or admin-editable? | static config file, admin-editable later |
