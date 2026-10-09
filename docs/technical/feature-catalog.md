# Feature catalog — feature set v1

Features are computed by **core-api** at ingest time (pipeline step 5) and stored in `event_features.features` (JSONB,
`feature_set_version = 1`). The same values are exposed to rules as `features.*`, consumed by ml-service for training
and prediction, and shown in the event detail page. ml-service must tolerate missing keys (impute) and ignore unknown
keys. Adding a feature is additive; changing a feature's meaning bumps `feature_set_version`.

## 1. Canonical event fields (`events` columns → `event.*`)

`event_type` (`transaction | login | account_change | promo_redemption | payout | registration | refund | <custom>`),
`external_id`, `occurred_at`, `channel` (`web|mobile_app|api|branch|atm|other`), `status`, `amount`, `currency`,
`merchant_id`, `merchant_category`, `payment_method` (`card|bank_transfer|ewallet|va|cod|paylater|other`),
`instrument_fingerprint`, `card_bin`, `card_last4`, `issuer_country`, `recipient_fingerprint` (payout/transfer
beneficiary), `device_id`, `ip_address`, `user_agent`, `geo_country`, `geo_city`, `latitude`, `longitude`,
`promo_code`, `discount_amount`, `cashback_amount`, `ref_transaction_id`, `shipping_address`, `billing_address`,
`account_change_type` (`password|email|phone|address|device|beneficiary|2fa` — for `account_change`),
`login_success` (bool — for `login`), `api_client_id` (for system-breach detection).

## 2. Features (`features.*`)

All counts and sums exclude the current event unless stated otherwise. "cust" means grouped by customer.

| Name | Type | Definition | Main typology |
|---|---|---|---|
| `amount` | number | event amount (0 if null) | all |
| `log_amount` | number | ln(1 + amount) | all |
| `hour_of_day` | int | local hour (Asia/Jakarta) of occurred_at | all |
| `day_of_week` | int | 0=Mon … 6=Sun | all |
| `is_night` | 0/1 | hour in [0,5] | ATO, breach |
| `account_age_days` | number | days since customer.registered_at (or first event) | promo abuse, ATO |
| `cust_cnt_1h` | int | cust events in last 1h | carding, breach |
| `cust_cnt_24h` | int | cust events in last 24h | all |
| `cust_txn_sum_24h` | number | cust transaction amount sum 24h | ATO, bank ATO |
| `cust_txn_avg_30d` | number | cust avg transaction amount 30d | all |
| `amount_zscore_30d` | number | (amount − avg_30d)/std_30d, 0 if <5 samples or std=0 | ATO, bank ATO |
| `cust_distinct_devices_24h` | int | distinct device_id 24h | ATO |
| `cust_distinct_ips_24h` | int | distinct ip 24h | ATO, breach |
| `cust_distinct_instruments_7d` | int | distinct instrument_fingerprint 7d | carding |
| `instrument_distinct_customers_30d` | int | customers using this instrument 30d | carding |
| `device_distinct_customers_30d` | int | customers using this device 30d | promo abuse, carding |
| `ip_distinct_customers_24h` | int | customers from this IP 24h | breach, promo abuse |
| `is_new_device` | 0/1 | device never seen for this customer before | ATO |
| `is_new_instrument` | 0/1 | instrument never seen for this customer before | carding, ATO |
| `is_new_recipient` | 0/1 | recipient never seen for this customer before | bank ATO, mule |
| `geo_country_mismatch` | 0/1 | geo_country ≠ customer's most frequent geo_country (90d) | ATO |
| `bin_country_mismatch` | 0/1 | issuer_country ≠ geo_country (both present) | carding |
| `shipping_billing_mismatch` | 0/1 | normalised shipping ≠ billing address (both present) | carding, ATO |
| `secs_since_last_login` | number | seconds since previous successful login (−1 if none) | ATO |
| `secs_since_account_change` | number | seconds since last account_change of any type (−1 if none) | ATO, bank ATO |
| `recent_credential_change_24h` | 0/1 | password/email/phone/2fa change in 24h | ATO |
| `failed_logins_24h` | int | login events with login_success=false 24h | ATO |
| `promo_cnt_cust_30d` | int | cust promo_redemption count 30d | promo abuse |
| `promo_cnt_device_30d` | int | promo redemptions from same device 30d (all customers) | promo abuse |
| `promo_distinct_customers_same_code_device_7d` | int | distinct customers redeeming the same promo_code on same device 7d | promo abuse |
| `discount_ratio` | number | discount_amount / (amount + discount_amount), 0 if none | promo abuse |
| `api_client_cnt_5m` | int | events from the same api_client_id in 5m (all customers) | breach |
| `api_client_distinct_customers_5m` | int | distinct customers from same api_client_id in 5m | breach |
| `graph_distance_to_fraud` | number | graph engine, default link kinds, depth 3; 99 if none | all |
| `graph_fraud_neighbors_2` | int | fraud customers within 2 hops | all |
| `graph_component_size` | int | bounded component size (cap 1000) | promo abuse, mule |
| `graph_shared_entity_count` | int | entities shared with other customers | promo abuse |
| `event_type` | category | one-hot in ML | — |
| `channel` | category | one-hot in ML | — |
| `payment_method` | category | one-hot in ML | — |

Categoricals are stored as strings. ml-service handles encoding (unknown categories map to an "other" bucket).

## 3. ML outputs (`ml.*`) and graph metrics (`graph.*`)

See rule-dsl.md §5. They are produced in pipeline step 6 and are not stored in `event_features`, because they depend on model
versions. They are stored in `decisions.engine_scores`.
