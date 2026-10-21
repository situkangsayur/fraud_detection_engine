# Feature catalog — feature set v1

> Document status: verified against commit 07ee765 on 2026-10-13

Features are computed by **core-api** at ingest time (pipeline step 6, architecture.md §3) and stored in
`event_features.features` (JSONB, `feature_set_version = 1`). Raw aggregates come from one SQL round trip
(`core-api/src/adapters/features_sql.rs`); the arithmetic is in `core-api/src/domain/features.rs`. The `graph_*`
features are added in step 7a from graph-service metrics and stored with the rest (also for `load_only` events). The
same values are exposed to rules as `features.*`, consumed by ml-service for training and prediction, and shown in the
event detail page. ml-service must tolerate missing keys (impute) and ignore unknown keys. Adding a feature is
additive; changing a feature's meaning bumps `feature_set_version`. The list is defined in Rust
`contracts::catalog` and mirrored in `ml_service/features/catalog.py`.

## 1. Canonical event fields (`events` columns → `event.*`)

`event_type` (`transaction | login | account_change | promo_redemption | payout | registration | refund | <custom>`),
`external_id`, `occurred_at`, `channel` (`web|mobile_app|api|branch|atm|other`), `status`, `amount`, `currency`,
`merchant_id`, `merchant_category`, `payment_method` (`card|bank_transfer|ewallet|va|cod|paylater|other`),
`instrument_fingerprint`, `card_bin`, `card_last4`, `issuer_country`, `recipient_fingerprint` (payout/transfer
beneficiary), `device_id`, `ip_address`, `user_agent`, `geo_country`, `geo_city`, `latitude`, `longitude`,
`promo_code`, `discount_amount`, `cashback_amount`, `ref_transaction_id`, `shipping_address`, `billing_address`,
`account_change_type` (`password|email|phone|address|device|beneficiary|2fa` — for `account_change`),
`login_success` (bool — for `login`), `api_client_id` (for system-breach detection).

A mapping may also target raw `card_number` / `account_number`; normalisation hashes them into
`instrument_fingerprint` / `recipient_fingerprint` and they are never stored (data-sources.md §3).

## 2. Features (`features.*`)

Conventions (from the SQL): every window ends at the event's `occurred_at` and only looks at events of the same
project with `occurred_at` ≤ it. "cust" means grouped by customer. Customer counts and sums **exclude** the current
event; "distinct customers per instrument/device/IP/API client/promo code" **include** it (so `1` means "only this
customer"). Absent aggregates become 0; numbers are rounded to 4 decimals.

| Name | Type | Definition | Main typology |
|---|---|---|---|
| `amount` | number | event amount (0 if null) | all |
| `log_amount` | number | ln(1 + max(amount, 0)) | all |
| `hour_of_day` | int | local hour of occurred_at in the **project timezone** (`projects.timezone`, default Asia/Jakarta) | all |
| `day_of_week` | int | 0=Mon … 6=Sun (project timezone) | all |
| `is_night` | 0/1 | local hour in [0,5] | ATO, breach |
| `account_age_days` | number | fractional days since customer.registered_at, else since the customer's first earlier event (0 if none) | promo abuse, ATO |
| `cust_cnt_1h` | int | cust events (any type) in last 1h | carding, breach |
| `cust_cnt_24h` | int | cust events (any type) in last 24h | all |
| `cust_txn_sum_24h` | number | cust `transaction` amount sum 24h | ATO, bank ATO |
| `cust_txn_avg_30d` | number | cust avg `transaction` amount 30d | all |
| `amount_zscore_30d` | number | (amount − avg_30d)/std_30d (sample std of cust `transaction` amounts 30d); 0 if < 5 samples, std = 0 or amount null | ATO, bank ATO |
| `cust_distinct_devices_24h` | int | distinct device_id of cust events 24h | ATO |
| `cust_distinct_ips_24h` | int | distinct ip_address of cust events 24h | ATO, breach |
| `cust_distinct_instruments_7d` | int | distinct instrument_fingerprint of cust events 7d | carding |
| `instrument_distinct_customers_30d` | int | customers using this instrument 30d (incl. current) | carding |
| `device_distinct_customers_30d` | int | customers using this device 30d (incl. current) | promo abuse, carding |
| `ip_distinct_customers_24h` | int | customers from this IP 24h (incl. current) | breach, promo abuse |
| `is_new_device` | 0/1 | device_id present and never seen for this customer before (0 if no device_id) | ATO |
| `is_new_instrument` | 0/1 | instrument present and never seen for this customer before | carding, ATO |
| `is_new_recipient` | 0/1 | recipient present and never seen for this customer before | bank ATO, mule |
| `geo_country_mismatch` | 0/1 | geo_country ≠ customer's most frequent geo_country of earlier events (90d); 0 if either missing | ATO |
| `bin_country_mismatch` | 0/1 | issuer_country ≠ geo_country (both present) | carding |
| `shipping_billing_mismatch` | 0/1 | normalised shipping ≠ billing address (both present; `pii::normalize_address`, e.g. `jl.` → `jalan`) | carding, ATO |
| `secs_since_last_login` | number | seconds since previous successful `login` (−1 if none) | ATO |
| `secs_since_account_change` | number | seconds since last `account_change` of any type (−1 if none) | ATO, bank ATO |
| `recent_credential_change_24h` | 0/1 | earlier `account_change` of type password/email/phone/2fa in 24h | ATO |
| `failed_logins_24h` | int | cust `login` events with login_success=false 24h | ATO |
| `promo_cnt_cust_30d` | int | cust `promo_redemption` count 30d | promo abuse |
| `promo_cnt_device_30d` | int | `promo_redemption` events from same device 30d (all customers, excl. current) | promo abuse |
| `promo_distinct_customers_same_code_device_7d` | int | distinct customers with events carrying the same promo_code on the same device 7d (incl. current; any event type) | promo abuse |
| `discount_ratio` | number | discount_amount / (max(amount, 0) + discount_amount), 0 if no discount | promo abuse |
| `api_client_cnt_5m` | int | events from the same api_client_id in 5m (all customers, excl. current) | breach |
| `api_client_distinct_customers_5m` | int | distinct customers from same api_client_id in 5m (incl. current) | breach |
| `graph_distance_to_fraud` | number | customer→customer hops to the nearest **other** fraud customer within the project's `graph_config.max_depth` (default 3) over its `link_kinds`; 99 if none | all |
| `graph_fraud_neighbors_2` | int | fraud customers within 2 hops | all |
| `graph_component_size` | int | customers reachable at any depth incl. self, capped at 1000 nodes / 25 layers | promo abuse, mule |
| `graph_shared_entity_count` | int | the customer's entities (allowed kinds, excluding supernodes) linked to ≥ 1 other customer | promo abuse |
| `event_type` | category | event type | — |
| `channel` | category | channel, `"other"` if missing | — |
| `payment_method` | category | payment method, `"other"` if missing | — |

`graph_*` keys are **absent** (not 0) when graph-service was degraded or the graph links step failed, so ml-service
imputes them instead of seeing fake zeros.

## 3. ML outputs (`ml.*`) and graph metrics (`graph.*`)

See rule-dsl.md §5. They are produced in pipeline step 7 and are not stored in `event_features`, because they depend
on model versions. They are stored on the decision: `decisions.ml` (fraud probability, anomaly score, cluster id,
cluster fraud rate, supervised model version), `decisions.graph` (all graph metrics) and the per-engine 0–100 scores in
`decisions.engine_scores`.

## 4. ML encoding (ml-service `FeaturePipeline`)

Fitted on the training rows and persisted with each model (`pipeline.joblib`):

* Selection: `ml_config.features.include` (default `["*"]` = all of §2) minus `exclude`, plus
  `extra_source_fields` (`source.*` paths from the raw payload; typed at fit time: numeric if ≥ 90 % parse as
  float, otherwise categorical).
* Numeric: median imputation (training medians) → standardisation → clip to ±10σ.
* Categorical (stored as strings): top 20 training categories + `__other__`, one-hot. Unknown or missing values go
  to `__other__` only if `__other__` occurred in training; otherwise they encode as all-zeros, so an untrained column
  never moves the score.
