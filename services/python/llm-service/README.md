# llm-service

The fraud platform's LLM assistant. It advises humans and **never activates anything**: the only write it performs
on other services is creating a *pending* rule proposal, which then goes through maker–checker approval in
rule-service and starts in shadow mode.

| Capability | Endpoint(s) |
|---|---|
| Regulation/policy library (tenant) | `GET/POST /api/v1/tenants/{tid}/regulations`, `GET/DELETE …/{id}` |
| Attach documents to a project | `GET/PUT /api/v1/projects/{pid}/llm/regulations` |
| Hybrid search (attached docs only) | `POST /api/v1/projects/{pid}/llm/regulations/search` |
| Analyst chat with tools + citations | `POST …/llm/chat`, `POST …/llm/chat/stream` (SSE), `GET …/llm/conversations[/{id}]` |
| Analyses → reports (+ proposals) | `POST …/llm/analysis/{rule-relevance,fraud-situation,regulation-impact,recommend-rules}`, `GET …/llm/reports[/{id}]` |
| Mapping suggestions for ingest-service | `POST /v1/mapping/suggest` (internal token) |
| Ops | `/health/live`, `/health/ready`, `/metrics`, `/openapi.json`, `/docs` |

Contract: `docs/technical/api-contract.md` §E. Tenancy: `docs/technical/multi-tenancy.md` §4.

## How it works

```
upload ─▶ sha256 dedupe ─▶ extract (pdf/docx/txt/md) ─▶ structure-aware chunking (BAB › Bagian › Paragraf › Pasal › ayat,
          Penjelasan, Lampiran; word windows as fallback) ─▶ embed (Ollama /api/embed, batched)
          ─▶ OpenSearch index reg-chunks-<tenant_id> (HNSW, lucene, cosine) ─▶ summary (LLM)
          └─ supersedes_id? ─▶ Pasal-level diff (difflib) ─▶ llm.regulation_changes + LLM change summary
                               ─▶ previous version = superseded, project attachments move to the new version

question ─▶ hybrid retrieval: kNN + BM25, fused with Reciprocal Rank Fusion, filtered to the project's attached docs
         ─▶ tool-calling loop (Ollama native tools, max N iterations, per-tool timeout) ─▶ answer + citations
```

Rule recommendations are produced as structured output: a JSON schema is passed to Ollama `format`, and the result
is re-validated with jsonschema. Every recommendation is checked with rule-service `POST …/rules/validate`. If it
fails, the validator errors go back to the LLM for up to `RULE_REPAIR_ATTEMPTS` (2) repairs. Only valid rules
become proposals (`POST …/proposals`); rule-service backtests each one on creation.

## Design choices

* **No LangChain.** The service talks to Ollama and OpenSearch through small explicit clients
  (`clients/ollama.py`, `clients/vector_store.py`) and a ~100-line agent loop (`agent/loop.py`).
  Every prompt, tool call and response is visible in code, which makes the audit trail and the prompt-injection
  defences reviewable. The dependency tree stays small, with no framework API churn, so tests only need `respx`
  and a scripted fake model instead of framework mocks. The cost is writing the tool loop and RRF ourselves; each
  is a few dozen lines and covered by tests.
* **Prompts are versioned files** in `src/llm_service/prompts/` (Jinja2, `StrictUndefined`). Each starts with
  `{#- prompt: <name>  version: <n> -#}`, and reports record the prompt version they used.
* **Prompt-injection defence:**
  - an explicit instruction hierarchy in `system_base.md.j2`;
  - regulation text and tool output only inside `<document>` / `<tool_result>` data blocks;
  - a tool allow-list with exactly one write tool (`create_rule_proposal`, pending only);
  - tool results truncated to 6000 chars;
  - mapping sample values treated as data.
* **Tenant isolation:**
  - every DB access goes through `tenant_session(tenant_id)` (`set_config('app.tenant_id', …, true)`, so RLS applies);
  - one OpenSearch index per tenant;
  - project retrieval filters on attached `regulation_id`s;
  - attaching another tenant's document id is rejected explicitly, because FK checks bypass RLS.
* **Degradation:** analyses fetch each data source defensively. An unavailable service is listed in
  `structured.data_gaps` instead of failing the report. If core-api is unreachable, the project context falls back
  to minimal defaults.
* **Background jobs** (indexing, analyses) run in an in-process runner with bounded concurrency. Durable queueing
  is on the backlog: after a restart, reports stuck in `running` must be re-triggered.

## Calls to other services (internal token + `X-Tenant-Id`, `X-Project-Id`, `X-Actor`, `X-Request-Id`)

| Service | Call |
|---|---|
| core-api | `GET /api/v1/projects/{pid}` (project context incl. `llm_config`) · `GET /api/v1/projects/{pid}/analytics/overview?from&to` · `GET …/analytics/drift` · `GET …/analytics/typologies` · `GET /v1/internal/projects/{pid}/field-catalog` (`{items:[{path,…}]}` or a list) |
| rule-service | `GET /api/v1/projects/{pid}/rules?page_size=200[&status&kind&typology&q]` · `GET …/rules/{id}` · `GET …/rules/performance?since_days` · `POST …/rules/validate` (body = envelope; 200 or 422 with `{valid, errors}`) · `POST …/rules/backtest` body `{"rule": envelope, "since_days": n}` · `POST …/proposals` body `{source:"llm", proposal_type, target_rule_id, definition, rationale, citations, evidence, report_id, llm_model}` → `{id, backtest?}` |
| graph-service | `GET /api/v1/projects/{pid}/graph/components?min_size&only_with_fraud=false` |
| ml-service | `GET /api/v1/projects/{pid}/ml/unsupervised/clusters` · `GET …/ml/graph-communities?min_size` (404 tolerated) |

## Configuration

| Env | Default | |
|---|---|---|
| `DATABASE_URL` | — | `postgresql+psycopg://llm_service:…@postgres:5432/fraud` |
| `INTERNAL_API_TOKEN`, `JWT_SECRET` | — | shared platform secrets |
| `CORE_API_URL`, `RULE_SERVICE_URL`, `GRAPH_SERVICE_URL`, `ML_SERVICE_URL` | compose names | |
| `OLLAMA_URL`, `OLLAMA_CHAT_MODEL`, `OLLAMA_EMBED_MODEL` | `qwen2.5:7b-instruct`, `bge-m3` | a project can override the chat model via `llm_config.chat_model` |
| `OPENSEARCH_URL` | `http://opensearch:9200` | |
| `REGULATION_DIR` | `/regulations` | uploaded originals (volume) |
| `CHAT_MAX_TOOL_ITERATIONS`, `TOOL_TIMEOUT_S`, `RULE_REPAIR_ATTEMPTS` | 6, 20, 2 | |

## Development

```bash
uv sync                                   # Python 3.12
uv run ruff check src tests && uv run ruff format --check src tests
uv run mypy                               # strict
uv run pytest                             # unit tests (fast, no infrastructure)
uv run pytest -m integration              # real Postgres 16 + migrations/RLS + OpenSearch 2.13 via docker
uv run uvicorn llm_service.main:app --reload --port 8002
```

Author: Hendri Karisma.
