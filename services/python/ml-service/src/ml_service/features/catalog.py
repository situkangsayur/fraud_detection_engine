"""Feature set v1 as defined in docs/technical/feature-catalog.md §2 (computed by core-api)."""

from __future__ import annotations

FEATURE_SET_VERSION = 1

NUMERIC_FEATURES: tuple[str, ...] = (
    "amount",
    "log_amount",
    "hour_of_day",
    "day_of_week",
    "is_night",
    "account_age_days",
    "cust_cnt_1h",
    "cust_cnt_24h",
    "cust_txn_sum_24h",
    "cust_txn_avg_30d",
    "amount_zscore_30d",
    "cust_distinct_devices_24h",
    "cust_distinct_ips_24h",
    "cust_distinct_instruments_7d",
    "instrument_distinct_customers_30d",
    "device_distinct_customers_30d",
    "ip_distinct_customers_24h",
    "is_new_device",
    "is_new_instrument",
    "is_new_recipient",
    "geo_country_mismatch",
    "bin_country_mismatch",
    "shipping_billing_mismatch",
    "secs_since_last_login",
    "secs_since_account_change",
    "recent_credential_change_24h",
    "failed_logins_24h",
    "promo_cnt_cust_30d",
    "promo_cnt_device_30d",
    "promo_distinct_customers_same_code_device_7d",
    "discount_ratio",
    "api_client_cnt_5m",
    "api_client_distinct_customers_5m",
    "graph_distance_to_fraud",
    "graph_fraud_neighbors_2",
    "graph_component_size",
    "graph_shared_entity_count",
)

CATEGORICAL_FEATURES: tuple[str, ...] = ("event_type", "channel", "payment_method")

ALL_FEATURES: tuple[str, ...] = NUMERIC_FEATURES + CATEGORICAL_FEATURES
