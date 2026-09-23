# Pluggable data sources — specification v1

Goal: a user can point the platform at **their own** transaction/event data (file, database table, or real-time
webhook) in **their own structure**, confirm an automatically suggested mapping in the UI, and immediately use every
field in rules, velocity, ML and graph, **without code changes and without reshaping their data**.

## 1. Concepts

| Concept | Table | Description |
|---|---|---|
| Data source | `core.data_sources` (per project) | A named origin of records: `webhook`, `file`, `postgres`, `mysql`. Holds the connection config, ingest mode and API key hash. |
| Inferred schema | `data_sources.inferred_schema` | Field list produced by ingest-service schema inference. |
| Mapping | `core.data_source_mappings` | A versioned, immutable mapping JSON that turns a source record into canonical event + customer (+ optional label). Exactly one version is `active`. |
| Field catalog | `core.field_catalog` + built-ins in code | Every addressable path (built-in canonical, feature, ml, graph — defined in Rust `contracts::catalog` — plus every discovered `source.*` path, stored per project) with type, PII flag and `velocity_enabled`. It drives rule validation, the rule builder dropdowns and ML feature selection. |
| Ingest job | `ingest.jobs` | A batch import run (file or SQL) with progress counters. |
| Ingest error | `core.ingest_errors` | Dead-letter rows that failed mapping/validation, with reason and raw record. |

## 2. Flows

### 2.1 Onboarding (UI "Data Sources" page)

1. The user creates a source (`kind`, name, event type hint) and uploads a sample file, gives a SQL connection plus
   table/query, or pastes sample JSON.
2. web → gateway → ingest-service `POST /api/v1/projects/{pid}/data-sources/{id}/inspect` (file, SQL, or JSON). The response contains the **inferred schema**, **suggested mapping** (each with a
   confidence score) and 20 **preview rows**.
3. The user adjusts the mapping in the mapping editor (source field → canonical field, transform picker) and clicks
   **Preview**. core-api `POST /api/v1/projects/{pid}/data-sources/{id}/mappings/preview` applies the mapping to the preview rows
   and returns the canonical events plus per-row errors.
4. **Save** creates mapping version n (`draft`). **Activate** (analyst or above) sets it `active`, and core-api
   registers all source fields in `field_catalog` (`entity='source'`, `source_id`).
5. Load data: for a webhook source, the client posts to `/api/v1/ingest/{slug}` with the source's API key. For file and
   SQL sources, the user starts an ingest job (`mode = score | load_only`). ingest-service streams batches to core-api
   `POST /v1/internal/projects/{pid}/sources/{source_id}/batch`.

### 2.2 Runtime

Core-api is the **only** place mappings are applied (the same Rust code serves webhook, batch and preview).
For each record:

1. Apply the active mapping. Required canonical fields: `event_type`, `external_id`, `occurred_at`,
   `customer_external_id`. If any is missing or a transform error occurs, the row goes to `ingest_errors` and processing
   continues with the next record.
2. Remove `drop_fields` and hashed-PII source fields from the stored payload, then store the rest as `events.payload`
   (the `source.*` context).
3. Unknown keys not yet in `field_catalog` are auto-registered (`velocity_enabled=false`, type inferred from the value).
4. Continue with the scoring pipeline (architecture §3). In `load_only` mode, steps 6–8 are skipped: no ML, rules or
   decision, but features and graph links are still built, which is required for training and backtests.
5. If the mapping has a `label` section, a `labels` row is created (`source='dataset'`), so labelled datasets
   (e.g. `is_fraud` columns) can train the supervised model immediately.

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

* A `from` value is a dotted path into the source record (`a.b[0].c`) or an array of paths (for `concat` / `coalesce`).
* `const` sets a fixed value. `default` is used when the source value is missing or null.
* `transform` is an ordered pipeline:

| fn | params | result |
|---|---|---|
| `to_number`, `to_string`, `to_bool` | — | type coercion (`"1.500.000,50"` handled by `to_number` with `locale: "id"`) |
| `parse_datetime` | `format` (strftime) or `"rfc3339"` / `"unix_s"` / `"unix_ms"`; `timezone` | UTC timestamp |
| `lowercase`, `uppercase`, `trim` | — | |
| `scale` | `factor` | x × factor |
| `value_map` | `map`, `default` | lookup |
| `regex_extract` | `pattern`, `group` | |
| `concat` | `sep` | joins multiple `from` values |
| `coalesce` | — | first non-null of multiple `from` values |
| `hash_pan`, `hash_account` | — | `HMAC-SHA256(tenant_pepper, digits_only)` hex, where `tenant_pepper = HMAC-SHA256(PII_PEPPER, tenant_id)` |
| `pan_bin`, `pan_last4` | `length` (bin: 6 or 8) | |
| `normalize_phone` | `default_country` (`ID`) | E.164 |
| `normalize_email` | — | lower-case, Gmail dot/plus removal |

Canonical event fields are listed in `feature-catalog.md` §1 and in the `events` table. Anything not mapped stays
available as `source.<path>`.

## 4. Schema inference (ingest-service)

* Formats: CSV/TSV (delimiter sniffing, encoding detection), JSON array, JSON Lines, Parquet, Excel (`.xlsx`),
  SQL table or query (Postgres/MySQL via SQLAlchemy; read-only credentials recommended), and posted sample JSON.
* Nested JSON is flattened into dotted paths (arrays: `[0]` sample plus a `[]` marker).
* Per field: `path`, `inferred_type` (`integer|number|string|bool|datetime|array|object`), `datetime_format` guess,
  `null_ratio`, `distinct_ratio`, up to 5 `sample_values` (PII-masked), and `pii` hint (`pan` via Luhn,
  `email`, `phone`, `account_number`, `name`).
* Mapping suggestion: scores every (source field, canonical field) pair on name similarity against a bilingual synonym
  dictionary (EN + ID, e.g. `jumlah|nominal|total|amount → amount`, `tgl|tanggal|waktu|created_at|timestamp →
  occurred_at`, `id_user|user_id|nasabah_id|customer → customer_external_id`, `no_hp|msisdn|phone → phone`,
  `no_kartu|pan|card_number → instrument_fingerprint + card_bin + card_last4`), plus type compatibility and value
  patterns (Luhn, email regex, IP regex, ISO country). It picks the best field per canonical target above a 0.5
  confidence, and PII fields get hashing transforms automatically. `use_llm=true` asks llm-service for
  suggestions on the remaining low-confidence fields.
* Label detection: boolean/0-1 columns named like `is_fraud|fraud|label|chargeback|penipuan` are proposed as `label`.

## 5. Connection secrets

SQL connection configs are stored in `data_sources.connection` **without passwords**. The password is referenced
by env var name (`"password_env": "SRC_ERP_DB_PASSWORD"`), which ingest-service resolves at runtime.
Webhook API keys are generated by core-api, shown once, and stored as an argon2 hash (`api_key_hash`) with a
lookup prefix (`api_key_prefix`).

## 6. Pull connectors

For `postgres`/`mysql` sources with `connection.poll.enabled`, ingest-service polls
`SELECT … WHERE <cursor_field> > :last_cursor ORDER BY <cursor_field> LIMIT :batch` every `interval_seconds`
and keeps `data_sources.cursor_state` updated. Delivery is at-least-once, and core-api dedups on
`(source_id, external_id)`.
