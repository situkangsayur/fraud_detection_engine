# ingest-service

Python 3.12 / FastAPI. Owns schema `ingest` (`jobs`, `uploads`). Specs: `docs/technical/data-sources.md`
§4–§6 and `docs/technical/api-contract.md` §F.

| Capability | Where |
|---|---|
| File readers: CSV/TSV (delimiter sniffing, charset detection), JSON array, JSONL, Parquet, XLSX; dotted headers un-flattened | `app/readers.py` |
| Schema inference: nested flattening, type + datetime-format guessing (ID `dd/mm/yyyy`, ISO, unix s/ms), null/distinct ratios, PII detection (Luhn PAN, email, phone, account no., name), masked samples | `app/inference/` |
| Mapping suggestion: bilingual EN/ID synonym **data file** (`app/mapping/synonyms.json`, editable without code changes), fuzzy names, type compatibility, value patterns, parent context (`user.*`, `pengguna.*`); emits mapping JSON in the data-sources §3 format with per-target confidence, PAN/account hashing, `drop_fields`, event-type `value_map` and `label` detection; optional LLM assist (`?use_llm=true`, via llm-service `/v1/mapping/suggest`, best-effort) | `app/mapping/` |
| Import jobs: stored upload or full SQL source. Records are sorted by the mapped `occurred_at` when the file has ≤ `SORT_MAX_ROWS` rows; batches of 500 go to core-api `POST /v1/internal/projects/{pid}/sources/{sid}/batch`, with exponential-backoff retries on 5xx/429/network errors and cooperative cancel | `app/jobs/runner.py`, `app/core_client.py` |
| Pull connectors (Postgres/MySQL): read-only sessions, `cursor_field` polling, at-least-once delivery, `cursor_state` persisted | `app/connectors/` |
| Upload expiry (24 h) cleanup loop | `app/connectors/scheduler.py` |

Auth: user JWT (claims `tid`, `trole`, `prj`) or `INTERNAL_API_TOKEN` + `X-Tenant-Id`. Every DB unit of work
runs in `tenant_session()` (`set_config('app.tenant_id', …, true)`), so Postgres RLS applies.

```bash
uv sync && uv run pytest && uv run ruff check . && uv run mypy app
uv run uvicorn app.main:app --port 8003
```

Env: `DATABASE_URL`, `CORE_API_URL`, `LLM_SERVICE_URL`, `INTERNAL_API_TOKEN`, `JWT_SECRET`, `UPLOAD_DIR`,
`MAX_UPLOAD_MB` (200), `BATCH_SIZE` (500), `SORT_MAX_ROWS` (2 000 000), `CONNECTOR_POLL_ENABLED`, `LOG_LEVEL`.

Memory note: a job keeps a whole file in memory only when it has to sort it (≤ `SORT_MAX_ROWS` rows).
Larger files stream in file order, so pre-sort them by time, or load history with `mode=load_only` first.
