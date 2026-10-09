//! Feature set v1 assembly (feature-catalog.md §2).
//!
//! The SQL adapter (`adapters::features_sql`) fetches raw aggregates in one round trip. This
//! module turns them plus the current event and customer into the documented feature map. The
//! split keeps the arithmetic (z-score, time zone, mismatches, ratios) unit-testable without a
//! database.
//!
//! Conventions: counts/sums exclude the current event; "distinct customers per instrument/device/
//! IP/API client" include the current event (so `1` means "only this customer").

use chrono::{DateTime, Datelike, Timelike, Utc};
use chrono_tz::Tz;
use contracts::events::CanonicalEventIn;
use contracts::graph::GraphMetrics;
use platform::pii;
use serde_json::{json, Map, Value};

/// Raw aggregates from SQL (all optional so a partial row never panics).
#[derive(Debug, Clone, Default, PartialEq, sqlx::FromRow)]
pub struct RawAggregates {
    pub cust_cnt_1h: Option<i64>,
    pub cust_cnt_24h: Option<i64>,
    pub cust_txn_sum_24h: Option<f64>,
    pub cust_txn_avg_30d: Option<f64>,
    pub cust_txn_std_30d: Option<f64>,
    pub cust_txn_n_30d: Option<i64>,
    pub cust_distinct_devices_24h: Option<i64>,
    pub cust_distinct_ips_24h: Option<i64>,
    pub cust_distinct_instruments_7d: Option<i64>,
    pub failed_logins_24h: Option<i64>,
    pub recent_credential_change_24h: Option<bool>,
    pub promo_cnt_cust_30d: Option<i64>,
    pub instrument_distinct_customers_30d: Option<i64>,
    pub device_distinct_customers_30d: Option<i64>,
    pub ip_distinct_customers_24h: Option<i64>,
    pub promo_cnt_device_30d: Option<i64>,
    pub promo_distinct_customers_same_code_device_7d: Option<i64>,
    pub api_client_cnt_5m: Option<i64>,
    pub api_client_distinct_customers_5m: Option<i64>,
    pub seen_device_before: Option<bool>,
    pub seen_instrument_before: Option<bool>,
    pub seen_recipient_before: Option<bool>,
    pub last_login_at: Option<DateTime<Utc>>,
    pub last_account_change_at: Option<DateTime<Utc>>,
    pub first_event_at: Option<DateTime<Utc>>,
    pub usual_geo_country: Option<String>,
}

/// Customer attributes needed by features.
#[derive(Debug, Clone, Default)]
pub struct CustomerFacts {
    pub registered_at: Option<DateTime<Utc>>,
}

fn i(v: Option<i64>) -> Value {
    json!(v.unwrap_or(0))
}

fn b(v: bool) -> Value {
    json!(i32::from(v))
}

fn r4(x: f64) -> f64 {
    (x * 10_000.0).round() / 10_000.0
}

/// Builds the v1 feature map (without graph features; see [`add_graph_features`]).
pub fn assemble(
    ev: &CanonicalEventIn,
    raw: &RawAggregates,
    cust: &CustomerFacts,
    tz: Tz,
) -> Map<String, Value> {
    let mut f = Map::new();
    let amount = ev.amount.unwrap_or(0.0);
    let local = ev.occurred_at.with_timezone(&tz);
    let hour = local.hour();

    f.insert("amount".into(), json!(amount));
    f.insert("log_amount".into(), json!(r4(amount.max(0.0).ln_1p())));
    f.insert("hour_of_day".into(), json!(hour));
    f.insert(
        "day_of_week".into(),
        json!(local.weekday().num_days_from_monday()),
    );
    f.insert("is_night".into(), b(hour <= 5));

    let since = cust
        .registered_at
        .or(raw.first_event_at)
        .unwrap_or(ev.occurred_at);
    let age_days = (ev.occurred_at - since).num_seconds().max(0) as f64 / 86_400.0;
    f.insert("account_age_days".into(), json!(r4(age_days)));

    f.insert("cust_cnt_1h".into(), i(raw.cust_cnt_1h));
    f.insert("cust_cnt_24h".into(), i(raw.cust_cnt_24h));
    f.insert(
        "cust_txn_sum_24h".into(),
        json!(r4(raw.cust_txn_sum_24h.unwrap_or(0.0))),
    );
    let avg = raw.cust_txn_avg_30d.unwrap_or(0.0);
    f.insert("cust_txn_avg_30d".into(), json!(r4(avg)));
    let z = match (raw.cust_txn_n_30d, raw.cust_txn_std_30d) {
        (Some(n), Some(std)) if n >= 5 && std > 0.0 && ev.amount.is_some() => (amount - avg) / std,
        _ => 0.0,
    };
    f.insert("amount_zscore_30d".into(), json!(r4(z)));
    f.insert(
        "cust_distinct_devices_24h".into(),
        i(raw.cust_distinct_devices_24h),
    );
    f.insert("cust_distinct_ips_24h".into(), i(raw.cust_distinct_ips_24h));
    f.insert(
        "cust_distinct_instruments_7d".into(),
        i(raw.cust_distinct_instruments_7d),
    );
    f.insert(
        "instrument_distinct_customers_30d".into(),
        i(raw.instrument_distinct_customers_30d),
    );
    f.insert(
        "device_distinct_customers_30d".into(),
        i(raw.device_distinct_customers_30d),
    );
    f.insert(
        "ip_distinct_customers_24h".into(),
        i(raw.ip_distinct_customers_24h),
    );
    f.insert(
        "is_new_device".into(),
        b(ev.device_id.is_some() && !raw.seen_device_before.unwrap_or(false)),
    );
    f.insert(
        "is_new_instrument".into(),
        b(ev.instrument_fingerprint.is_some() && !raw.seen_instrument_before.unwrap_or(false)),
    );
    f.insert(
        "is_new_recipient".into(),
        b(ev.recipient_fingerprint.is_some() && !raw.seen_recipient_before.unwrap_or(false)),
    );
    let geo_mismatch = match (&ev.geo_country, &raw.usual_geo_country) {
        (Some(cur), Some(usual)) => cur != usual,
        _ => false,
    };
    f.insert("geo_country_mismatch".into(), b(geo_mismatch));
    let bin_mismatch = match (&ev.issuer_country, &ev.geo_country) {
        (Some(a), Some(b)) => a != b,
        _ => false,
    };
    f.insert("bin_country_mismatch".into(), b(bin_mismatch));
    let ship_mismatch = match (&ev.shipping_address, &ev.billing_address) {
        (Some(s), Some(bl)) => pii::normalize_address(s) != pii::normalize_address(bl),
        _ => false,
    };
    f.insert("shipping_billing_mismatch".into(), b(ship_mismatch));
    let secs_since = |t: Option<DateTime<Utc>>| match t {
        Some(t) => json!((ev.occurred_at - t).num_seconds().max(0)),
        None => json!(-1),
    };
    f.insert("secs_since_last_login".into(), secs_since(raw.last_login_at));
    f.insert(
        "secs_since_account_change".into(),
        secs_since(raw.last_account_change_at),
    );
    f.insert(
        "recent_credential_change_24h".into(),
        b(raw.recent_credential_change_24h.unwrap_or(false)),
    );
    f.insert("failed_logins_24h".into(), i(raw.failed_logins_24h));
    f.insert("promo_cnt_cust_30d".into(), i(raw.promo_cnt_cust_30d));
    f.insert("promo_cnt_device_30d".into(), i(raw.promo_cnt_device_30d));
    f.insert(
        "promo_distinct_customers_same_code_device_7d".into(),
        i(raw.promo_distinct_customers_same_code_device_7d),
    );
    let discount = ev.discount_amount.unwrap_or(0.0).max(0.0);
    let ratio = if discount > 0.0 && amount + discount > 0.0 {
        discount / (amount.max(0.0) + discount)
    } else {
        0.0
    };
    f.insert("discount_ratio".into(), json!(r4(ratio)));
    f.insert("api_client_cnt_5m".into(), i(raw.api_client_cnt_5m));
    f.insert(
        "api_client_distinct_customers_5m".into(),
        i(raw.api_client_distinct_customers_5m),
    );
    f.insert("event_type".into(), json!(ev.event_type));
    f.insert(
        "channel".into(),
        json!(ev.channel.clone().unwrap_or_else(|| "other".into())),
    );
    f.insert(
        "payment_method".into(),
        json!(ev.payment_method.clone().unwrap_or_else(|| "other".into())),
    );
    f
}

/// Adds `graph_*` features from graph metrics (absent when graph-service was degraded, so
/// ml-service imputes them instead of seeing fake zeros).
pub fn add_graph_features(f: &mut Map<String, Value>, g: Option<&GraphMetrics>) {
    if let Some(g) = g {
        f.insert(
            "graph_distance_to_fraud".into(),
            json!(g.distance_to_fraud.unwrap_or(99)),
        );
        f.insert("graph_fraud_neighbors_2".into(), json!(g.fraud_neighbors_2));
        f.insert("graph_component_size".into(), json!(g.component_size));
        f.insert("graph_shared_entity_count".into(), json!(g.shared_entity_count));
    }
}

/// `graph.*` rule context: distance +∞ is represented as JSON `null` (rule-engine treats null
/// distance as +∞ for `distance_to_fraud` graph metrics; for field comparisons it traps/no-matches).
pub fn graph_context(g: Option<&GraphMetrics>) -> Value {
    match g {
        Some(g) => serde_json::to_value(g).unwrap_or(Value::Null),
        None => json!({}),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use contracts::events::CustomerIn;

    fn ev() -> CanonicalEventIn {
        CanonicalEventIn {
            external_id: "T".into(),
            event_type: "transaction".into(),
            occurred_at: Utc.with_ymd_and_hms(2026, 9, 22, 20, 30, 0).unwrap(), // 03:30 WIB
            customer: CustomerIn::default(),
            amount: Some(1_000_000.0),
            discount_amount: Some(250_000.0),
            device_id: Some("dev".into()),
            issuer_country: Some("US".into()),
            geo_country: Some("ID".into()),
            shipping_address: Some("Jl. Sudirman No. 1".into()),
            billing_address: Some("jalan sudirman nomor 1".into()),
            ..Default::default()
        }
    }

    #[test]
    fn assembles_documented_features() {
        let raw = RawAggregates {
            cust_txn_avg_30d: Some(200_000.0),
            cust_txn_std_30d: Some(100_000.0),
            cust_txn_n_30d: Some(10),
            seen_device_before: Some(false),
            usual_geo_country: Some("SG".into()),
            last_login_at: Some(Utc.with_ymd_and_hms(2026, 9, 22, 20, 0, 0).unwrap()),
            ..Default::default()
        };
        let cust = CustomerFacts {
            registered_at: Some(Utc.with_ymd_and_hms(2026, 9, 12, 20, 30, 0).unwrap()),
        };
        let f = assemble(&ev(), &raw, &cust, chrono_tz::Asia::Jakarta);
        assert_eq!(f["hour_of_day"], json!(3));
        assert_eq!(f["is_night"], json!(1));
        assert_eq!(f["day_of_week"], json!(2)); // Wednesday 23 Sept 2026 local
        assert_eq!(f["account_age_days"], json!(10.0));
        assert_eq!(f["amount_zscore_30d"], json!(8.0));
        assert_eq!(f["is_new_device"], json!(1));
        assert_eq!(f["is_new_instrument"], json!(0));
        assert_eq!(f["bin_country_mismatch"], json!(1));
        assert_eq!(f["geo_country_mismatch"], json!(1));
        assert_eq!(f["shipping_billing_mismatch"], json!(0));
        assert_eq!(f["secs_since_last_login"], json!(1800));
        assert_eq!(f["secs_since_account_change"], json!(-1));
        assert_eq!(f["discount_ratio"], json!(0.2));
        assert_eq!(f["channel"], json!("other"));
        // every documented v1 feature except graph_* is present
        for name in contracts::catalog::feature_names().filter(|n| !n.starts_with("graph_")) {
            assert!(f.contains_key(name), "missing {name}");
        }
    }

    #[test]
    fn zscore_needs_five_samples() {
        let raw = RawAggregates {
            cust_txn_avg_30d: Some(1.0),
            cust_txn_std_30d: Some(1.0),
            cust_txn_n_30d: Some(4),
            ..Default::default()
        };
        let f = assemble(&ev(), &raw, &CustomerFacts::default(), chrono_tz::UTC);
        assert_eq!(f["amount_zscore_30d"], json!(0.0));
        assert_eq!(f["account_age_days"], json!(0.0));
    }

    #[test]
    fn graph_features() {
        let mut f = Map::new();
        add_graph_features(&mut f, None);
        assert!(f.is_empty());
        add_graph_features(
            &mut f,
            Some(&GraphMetrics {
                distance_to_fraud: None,
                component_size: 3,
                ..Default::default()
            }),
        );
        assert_eq!(f["graph_distance_to_fraud"], json!(99));
        assert_eq!(f["graph_component_size"], json!(3));
    }
}
