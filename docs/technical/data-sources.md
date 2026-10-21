# Pluggable data sources — specification v1

> Document status: verified against commit 07ee765 on 2026-10-13

Goal: a user can point the platform at **their own** transaction/event data (file, database table, or real-time
webhook) in **their own structure**, confirm an automatically suggested mapping in the UI, and immediately use every
field in rules, velocity, ML and graph, **without code changes and without reshaping their data**.

## 1. Concepts

| Concept | Table | Description |
|---|---|---|
| Data source | `core.data_sources` (per project) | A named origin of records: `webhook`, `file`, `postgres`, `mysql` (plus `internal`). Holds the connection config, `default_event_type`, ingest mode (`score`/`load_only`), `cursor_state`, `is_active` and the API key hash/prefix. |
| Inferred schema | `data_sources.inferred_schema` | Field list produced by ingest-service schema inference (written on every inspect). |
| Mapping | `core.data_source_mappings` | A versioned mapping JSON (`draft` → `active` → `archived`) that turns a source record into canonical event + customer (+ optional label). At most one version is `active` (unique index). |
| Field catalog | `core.field_catalog` + built-ins in code | Every addressable path (built-in canonical, feature, ml, graph — defined in Rust `contracts::catalog` — plus every discovered `source.*` path, stored per project) with type, PII flag and `velocity_enabled`. It drives rule validation, the rule builder dropdowns and ML feature selection. |
| Ingest job | `ingest.jobs` | A batch import run (stored upload or full SQL source) with progress counters. Uploads are tracked in `ingest.uploads` and expire after 24 h (`UPLOAD_TTL_HOURS`). |
| Ingest error | `core.ingest_errors` | Dead-letter rows that failed mapping/validation, with reason and raw record. |

## 2. Flows

### 2.1 Onboarding (UI "Data Sources" page)

1. The user creates a source (`kind`, name, event type hint) and uploads a sample file, gives a SQL connection plus
   table/query, or pastes sample JSON.
2. web → gateway → ingest-service `POST /api/v1/projects/{pid}/data-sources/{id}/inspect[?use_llm=true]` (analyst;
   multipart `file` ≤ `MAX_UPLOAD_MB` (200), JSON `{"sql": {table|query, limit}}` for postgres/mysql sources, or
   `{"records": [...]}`). Up to `INSPECT_SAMPLE_ROWS` (1 000) records are inspected. The response contains the
   **inferred schema**, **suggested mapping**, per-target `confidence`, `unmapped_fields`, `notes`, `llm_used` and 20
   **preview rows** (`PREVIEW_ROWS`); a file upload also returns `upload_id`, `file_format` and `row_estimate`.
3. The user adjusts the mapping in the mapping editor (source field → canonical field, transform picker) and clicks
   **Preview**. core-api `POST /api/v1/projects/{pid}/data-sources/{id}/mappings/preview` applies the mapping to the preview rows
   and returns the canonical events plus per-row errors.
4. **Save** creates mapping version n (`draft`). **Activate** (analyst or above) sets it `active`, archives the previous
   active version, and core-api registers the source fields (inferred schema + every referenced path) in
   `field_catalog` as `source.<path>` with `data_source_id`, type and PII flag.
5. Load data: for a webhook source, the client posts to `/api/v1/ingest/{slug}` (or `/api/v1/ingest/{slug}/batch`)
   with the source's API key. For file and SQL sources, the user starts an ingest job
   `POST /api/v1/projects/{pid}/data-sources/{id}/jobs {upload_id?, mode?}` (`mode = score | load_only`, default =
   the source's mode; an active mapping is required). ingest-service sorts records by the mapped `occurred_at` when
   the file has ≤ `SORT_MAX_ROWS` (2 000 000) rows (larger files stream in file order), and sends batches of
   `BATCH_SIZE` (500) raw records to core-api `POST /v1/internal/projects/{pid}/sources/{source_id}/batch`, retrying
   5xx/429/network errors with exponential backoff (`CORE_MAX_RETRIES`, 5). Jobs are listed per source, read via
   `GET …/ingest-jobs/{job_id}` and cancelled cooperatively via `POST …/ingest-jobs/{job_id}/cancel`.

### 2.2 Runtime

Core-api is the **only** place mappings are applied (the same Rust code serves webhook, batch and preview).
For each record:

1. Apply the active mapping. Required canonical fields: `event_type` (from the mapping, else the source's
   `default_event_type`), `external_id`, `occurred_at`, `customer_external_id`. If any is missing or a transform error
   occurs, the row goes to `ingest_errors` (errors are collected per field) and processing continues with the next
   record.
2. Remove `drop_fields`, every source path feeding `hash_pan`/`hash_account`/`pan_bin`/`pan_last4`, and raw
   `card_number`/`account_number` sources from the stored payload, then store the rest as `events.payload` (the
   `source.*` context).
3. Unknown keys not yet in `field_catalog` are auto-registered (`velocity_enabled=false`, type inferred from the value).
4. Continue with the scoring pipeline (architecture §3). In `load_only` mode, ML, rules, combination and decision are
   skipped, but graph links, features (including `graph_*`) are still built and stored, which is required for
   training and backtests.
5. If the mapping has a `label` section and the label column is non-null, a `labels` row is created
   (`source='dataset'`), so labelled datasets (e.g. `is_fraud` columns) can train the supervised model immediately.
   `fraud_type` is lower-cased and must be a known typology, otherwise `other`.

## 3. Mapping JSON

```jsonc
{
  "event_type": { "from": "trx_type", "value_map": { "PURCHASE": "transaction", "LOGIN": "login",
                                                    "VOUCHER": "promo_redemption" }, "default": "transaction" },
  "event": {
    "external_id":            { "from": "trx_id" },
    "occurred_at":            { "from": "created", "transform": [{ "fn": "parse_datetime", "format": "%d/%m/%Y %H:%M:%S", "timezone": "Asia/Jakarta" }] },
    "customer_external_id":   { "from": "user.id", "transform": [{ "fn": "to_string" }] },
    "amount":                 { "from": "total", "transform": [{ "fn": "to_number" }, { "fn": "scale", "factor": 0.01 }] },
    "currency":               { "const": "IDR" },
    "instrument_fingerprint": { "from": "card_number", "transform": [{ "fn": "hash_pan" }] },
    "card_bin":               { "from": "card_number", "transform": [{ "fn": "pan_bin" }] },
    "card_last4":             { "from": "card_number", "transform": [{ "fn": "pan_last4" }] },
    "shipping_address":       { "from": ["ship.street", "ship.city"], "transform": [{ "fn": "concat", "sep": ", " }] }
  },
  "customer": {
    "full_name":  { "from": "user.name" },
    "email":      { "from": "user.email" },
    "phone":      { "from": "user.hp", "transform": [{ "fn": "normalize_phone", "default_country": "ID" }] },
    "registered_at": { "from": "user.join_date", "transform": [{ "fn": "parse_datetime", "format": "%Y-%m-%d" }] },
    "attributes": { "monthly_income": { "from": "user.income", "transform": [{ "fn": "to_number" }] } }
  },
  "label": {                                       // optional — for labelled datasets
    "from": "is_fraud", "fraud_values": [1, "1", "true", "Y"],
    "fraud_type": { "from": "fraud_category", "default": "other" }
  },
  "drop_fields": ["cvv", "card_number"]
}
```

* `event` keys must be canonical event fields (feature-catalog.md §1) or raw `card_number` / `account_number`
  (hashed by normalisation, never stored). `customer` accepts `full_name`, `email`, `phone`, `registered_at`,
  `kyc_level`, `segment` and free-form `attributes.*`. Unknown keys and unknown transform names are rejected at save
  time (`deny_unknown_fields`).
* A `from` value is a dotted path into the source record (`a.b[0].c`) or an array of paths (for `concat` / `coalesce`).
* `const` sets a fixed value. `default` is used when the source value is missing or null. A field spec may also carry
  a `value_map` (as `event_type` above).
* `transform` is an ordered pipeline (a null value passes through unchanged except for `concat`/`coalesce`):

| fn | params | result |
|---|---|---|
| `to_number`, `to_string`, `to_bool` | `locale` (`to_number` only: `id`, `en`, absent = auto-detect) | type coercion (`"1.500.000,50"` with `locale: "id"`; `to_bool` also accepts `ya/tidak`, `success/failed`, `berhasil/gagal`) |
| `parse_datetime` | `format` (strftime) or `"rfc3339"` / `"unix_s"` / `"unix_ms"`, absent = auto-detect; `timezone` (IANA, default = project timezone) | UTC timestamp |
| `lowercase`, `uppercase`, `trim` | — | |
| `scale` | `factor` | x × factor |
| `value_map` | `map`, `default` | lookup |
| `regex_extract` | `pattern`, `group` (default 1, falls back to whole match) | |
| `concat` | `sep` | joins multiple `from` values |
| `coalesce` | — | first non-null of multiple `from` values |
| `hash_pan`, `hash_account` | — | `HMAC-SHA256(tenant_pepper, digits_only)` hex, where `tenant_pepper = HMAC-SHA256(PII_PEPPER, tenant_id)`; error if < 6 digits |
| `pan_bin`, `pan_last4` | `length` (bin: 6 or 8, default 6) | |
| `normalize_phone` | `default_country` (default `ID`; ISO code or calling-code digits) | E.164 |
| `normalize_email` | — | lower-case, Gmail dot/plus removal |

Canonical event fields are listed in `feature-catalog.md` §1 and in the `events` table. Anything not mapped stays
available as `source.<path>`.

## 4. Schema inference (ingest-service)

* Formats: CSV/TSV (delimiter sniffing over `, ; \t |`, encoding detection via `charset-normalizer`), JSON array (or
  an object with a single array, e.g. `{"data": [...]}`), JSON Lines (`.jsonl`/`.ndjson`), Parquet, Excel (`.xlsx`),
  SQL table or query (Postgres/MySQL via SQLAlchemy, see §6), and posted sample JSON. Tabular cells stay strings (keeps
  leading zeros); dotted headers (`user.id`) are un-flattened into nested objects so mapping paths match JSON sources.
* Nested JSON is flattened into dotted paths (arrays: `[0]` sample plus a `[]` marker; max depth 8).
* Per field: `path`, `inferred_type` (`integer|number|string|bool|datetime|array|object`), `datetime_format` guess
  (ISO/`rfc3339`, day-first `dd/mm/yyyy` variants before `mm/dd/yyyy`, `unix_s`/`unix_ms` for integer columns with a
  time-like name), `null_ratio`, `distinct_ratio`, up to 5 most common `sample_values` (PII-masked), and `pii` hint by
  ≥ 80 % of samples: `pan` (Luhn, 13–19 digits), `email`, `phone` (Indonesian mobile or E.164), `account_number`
  (name hint such as `rekening|account|norek|iban` + digits), `name` (name hint such as `name|nama|holder`). Digit
  strings with a leading `0`/`+` or longer than 15 characters stay strings.
* Mapping suggestion (`app/mapping/suggest.py`): the bilingual EN + ID synonym dictionary is a data file,
  `app/mapping/synonyms.json` (editable without code changes; e.g. `jumlah|nominal|total|amount → amount`,
  `tgl|tanggal|waktu|created_at|timestamp → occurred_at`, `id_user|user_id|nasabah_id|customer → customer_external_id`,
  `no_hp|msisdn|phone → customer.phone`, `no_kartu|pan|card_number → instrument_fingerprint`,
  `rekening_tujuan|beneficiary_account → recipient_fingerprint`). Score per (source field, canonical target) =
  name similarity (exact, fuzzy via rapidfuzz, token containment) × type compatibility × parent context (fields under
  `user.*`, `pengguna.*`, `nasabah.*`, … favour `customer.*` targets), raised by value patterns (PAN/email ≥ 0.95,
  phone/account/name ≥ 0.8, IPv4 → `ip_address` ≥ 0.9). Assignment is greedy (each target and source once) above
  0.5 confidence. Generated transforms: PAN → `instrument_fingerprint` (`hash_pan`) + `card_bin` (`pan_bin`, 6) +
  `card_last4`; account number → `recipient_fingerprint` (`hash_account`); phone → `normalize_phone` (ID); email →
  `trim` + `lowercase`; datetimes → `parse_datetime` with the detected format (`Asia/Jakarta` for naive formats);
  amounts → `to_number` (`locale: "id"` when samples look Indonesian); ids → `to_string`; countries/currency →
  `trim` + `uppercase`. No currency column → `{"const": "IDR"}`. `event_type` comes from a low-cardinality column
  named like `type|jenis|trx_type|activity` with a `value_map` to canonical types, else `{"const": <default>}`.
  `drop_fields` = PAN/account columns, columns named like `cvv|pin|password|otp`, and every hashed source.
  Missing required fields are reported in `notes`.
* `use_llm=true` sends the still-unmapped fields to llm-service `POST /v1/mapping/suggest`; suggestions with
  confidence ≥ 0.6 for unassigned targets are merged (confidence × 0.9). Best-effort: any failure returns the
  rule-based result.
* Label detection: a boolean/0-1/yes-no column whose name matches `is_fraud|fraud|label|chargeback|penipuan|fraud_flag|
  target|class` becomes `label` (`fraud_values`: 1, "1", true, "Y", "yes", "ya", …); a column named like
  `fraud_type|fraud_category|jenis_fraud` becomes `label.fraud_type` (default `other`).

## 5. Connection secrets

SQL connection configs are stored in `data_sources.connection` **without passwords**. The password is referenced
by env var name (`"password_env": "SRC_ERP_DB_PASSWORD"`), which ingest-service resolves at runtime.
Webhook API keys are generated by core-api, shown once, and stored as an argon2 hash (`api_key_hash`) with a
lookup prefix (`api_key_prefix`).

## 6. SQL sources & pull connectors

Connection config (`data_sources.connection`): `{driver: postgres|mysql, host, port, database, user, password_env,
table | query, cursor_field, poll: {enabled, interval_seconds (60), batch_size (500)}}`. Sessions are forced
read-only (Postgres `default_transaction_read_only=on`, MySQL `SET SESSION TRANSACTION READ ONLY`); `table` must be
an identifier (`schema.table`) and `query` a single `SELECT`/`WITH` statement without write keywords. A job on a SQL
source reads the full relation (`SELECT * … ORDER BY <cursor_field>` when set) in streamed batches.

For active `postgres`/`mysql` sources with `connection.poll.enabled`, ingest-service (when `CONNECTOR_POLL_ENABLED`,
loop tick 5 s) discovers sources via the SECURITY DEFINER function `core.list_pollable_sources()` and polls each one
whose `interval_seconds` elapsed: `SELECT * FROM <relation> WHERE <cursor_field> > :last_cursor ORDER BY
<cursor_field> LIMIT :batch` (no `WHERE` on the first poll). It sends the rows with the source's mode and stores
`last_cursor`, `last_polled_at` and `last_error` in `data_sources.cursor_state`. The cursor advances only after
core-api accepted the batch, so delivery is at-least-once, and core-api dedups on `(source_id, external_id)`. The
cursor must be monotonic and unique enough that a batch boundary never splits rows sharing one value.
