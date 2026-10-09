//! Built-in field catalog (feature-catalog.md, rule-dsl.md §5).
//!
//! These paths exist in every project. Source-specific paths (`source.*`) are discovered per
//! project and stored in `core.field_catalog`; core-api merges both lists for the UI and for
//! rule validation.
//!
//! The catalog is a `static` slice (compiled into the binary) rather than a DB table so that the
//! feature code in core-api and the validator in rule-service can never disagree about what
//! exists: both link this crate.

use serde::Serialize;

/// Version of the feature set computed by core-api (`event_features.feature_set_version`).
pub const FEATURE_SET_VERSION: i32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Entity {
    Event,
    Source,
    Customer,
    Features,
    Ml,
    Graph,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DataType {
    Integer,
    Number,
    String,
    Bool,
    Datetime,
    Category,
    Object,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CatalogField {
    pub path: &'static str,
    pub entity: Entity,
    pub data_type: DataType,
    pub description: &'static str,
    /// Usable in velocity `group_by` / `aggregate.field` and composite `hist.*`.
    pub velocity_enabled: bool,
    pub pii: bool,
}

macro_rules! f {
    ($path:literal, $entity:ident, $dt:ident, $vel:literal, $desc:literal) => {
        CatalogField {
            path: $path,
            entity: Entity::$entity,
            data_type: DataType::$dt,
            description: $desc,
            velocity_enabled: $vel,
            pii: false,
        }
    };
    ($path:literal, $entity:ident, $dt:ident, $vel:literal, $desc:literal, pii) => {
        CatalogField {
            path: $path,
            entity: Entity::$entity,
            data_type: DataType::$dt,
            description: $desc,
            velocity_enabled: $vel,
            pii: true,
        }
    };
}

/// All built-in paths.
pub static BUILTIN_FIELDS: &[CatalogField] = &[
    // ---- event (canonical columns of core.events) ----
    f!("event.event_type", Event, String, true, "Event type (transaction, login, account_change, promo_redemption, payout, registration, refund, custom)"),
    f!("event.external_id", Event, String, false, "Id of the event in the source system"),
    f!("event.customer_id", Event, String, true, "Internal customer id (velocity grouping)"),
    f!("event.occurred_at", Event, Datetime, false, "When the event happened (UTC)"),
    f!("event.channel", Event, String, true, "web | mobile_app | api | branch | atm | other"),
    f!("event.status", Event, String, true, "Status in the source system"),
    f!("event.amount", Event, Number, true, "Amount in project currency"),
    f!("event.currency", Event, String, true, "ISO 4217 currency"),
    f!("event.merchant_id", Event, String, true, "Merchant / seller id"),
    f!("event.merchant_category", Event, String, true, "Merchant category (MCC or custom)"),
    f!("event.payment_method", Event, String, true, "card | bank_transfer | ewallet | va | cod | paylater | other"),
    f!("event.instrument_fingerprint", Event, String, true, "Tenant-peppered HMAC of card/account number", pii),
    f!("event.card_bin", Event, String, true, "Card BIN (first 6-8 digits)"),
    f!("event.card_last4", Event, String, false, "Card last 4 digits"),
    f!("event.issuer_country", Event, String, true, "Card issuer country (ISO 3166-1 alpha-2)"),
    f!("event.recipient_fingerprint", Event, String, true, "Tenant-peppered HMAC of beneficiary account", pii),
    f!("event.device_id", Event, String, true, "Device fingerprint id", pii),
    f!("event.ip_address", Event, String, true, "Client IP address", pii),
    f!("event.user_agent", Event, String, false, "User agent string"),
    f!("event.geo_country", Event, String, true, "Country of the client (ISO 3166-1 alpha-2)"),
    f!("event.geo_city", Event, String, true, "City of the client"),
    f!("event.latitude", Event, Number, false, "Latitude"),
    f!("event.longitude", Event, Number, false, "Longitude"),
    f!("event.promo_code", Event, String, true, "Voucher / promo code"),
    f!("event.discount_amount", Event, Number, true, "Discount granted"),
    f!("event.cashback_amount", Event, Number, true, "Cashback granted"),
    f!("event.ref_transaction_id", Event, String, true, "Referenced transaction (refund/return/chargeback)"),
    f!("event.shipping_address", Event, String, true, "Shipping address", pii),
    f!("event.billing_address", Event, String, false, "Billing address", pii),
    f!("event.account_change_type", Event, String, true, "password | email | phone | address | device | beneficiary | 2fa"),
    f!("event.login_success", Event, Bool, true, "Login succeeded (login events)"),
    f!("event.api_client_id", Event, String, true, "API client / key id (system breach detection)"),
    // ---- customer ----
    f!("customer.external_id", Customer, String, false, "Customer id in the source system"),
    f!("customer.kyc_level", Customer, Integer, false, "KYC level"),
    f!("customer.segment", Customer, String, false, "Customer segment"),
    f!("customer.status", Customer, String, false, "active | suspended | closed"),
    f!("customer.registered_at", Customer, Datetime, false, "Registration time"),
    f!("customer.risk_label", Customer, String, false, "fraud | legit | unknown"),
    f!("customer.account_age_days", Customer, Number, false, "Days since registration"),
    // ---- features v1 ----
    f!("features.amount", Features, Number, false, "Event amount (0 if null)"),
    f!("features.log_amount", Features, Number, false, "ln(1 + amount)"),
    f!("features.hour_of_day", Features, Integer, false, "Local hour of occurred_at (project timezone)"),
    f!("features.day_of_week", Features, Integer, false, "0=Mon … 6=Sun"),
    f!("features.is_night", Features, Integer, false, "1 if hour in [0,5]"),
    f!("features.account_age_days", Features, Number, false, "Days since registration or first event"),
    f!("features.cust_cnt_1h", Features, Integer, false, "Customer events in last 1h"),
    f!("features.cust_cnt_24h", Features, Integer, false, "Customer events in last 24h"),
    f!("features.cust_txn_sum_24h", Features, Number, false, "Customer transaction amount sum 24h"),
    f!("features.cust_txn_avg_30d", Features, Number, false, "Customer average transaction amount 30d"),
    f!("features.amount_zscore_30d", Features, Number, false, "(amount - avg_30d) / std_30d"),
    f!("features.cust_distinct_devices_24h", Features, Integer, false, "Distinct devices 24h"),
    f!("features.cust_distinct_ips_24h", Features, Integer, false, "Distinct IPs 24h"),
    f!("features.cust_distinct_instruments_7d", Features, Integer, false, "Distinct instruments 7d"),
    f!("features.instrument_distinct_customers_30d", Features, Integer, false, "Customers using this instrument 30d"),
    f!("features.device_distinct_customers_30d", Features, Integer, false, "Customers using this device 30d"),
    f!("features.ip_distinct_customers_24h", Features, Integer, false, "Customers from this IP 24h"),
    f!("features.is_new_device", Features, Integer, false, "Device never seen for this customer"),
    f!("features.is_new_instrument", Features, Integer, false, "Instrument never seen for this customer"),
    f!("features.is_new_recipient", Features, Integer, false, "Recipient never seen for this customer"),
    f!("features.geo_country_mismatch", Features, Integer, false, "Country differs from usual (90d)"),
    f!("features.bin_country_mismatch", Features, Integer, false, "Issuer country differs from geo country"),
    f!("features.shipping_billing_mismatch", Features, Integer, false, "Shipping address differs from billing"),
    f!("features.secs_since_last_login", Features, Number, false, "Seconds since previous successful login (-1 none)"),
    f!("features.secs_since_account_change", Features, Number, false, "Seconds since last account change (-1 none)"),
    f!("features.recent_credential_change_24h", Features, Integer, false, "Password/email/phone/2fa change in 24h"),
    f!("features.failed_logins_24h", Features, Integer, false, "Failed logins 24h"),
    f!("features.promo_cnt_cust_30d", Features, Integer, false, "Customer promo redemptions 30d"),
    f!("features.promo_cnt_device_30d", Features, Integer, false, "Promo redemptions from this device 30d"),
    f!("features.promo_distinct_customers_same_code_device_7d", Features, Integer, false, "Customers redeeming same code on same device 7d"),
    f!("features.discount_ratio", Features, Number, false, "discount / (amount + discount)"),
    f!("features.api_client_cnt_5m", Features, Integer, false, "Events from same API client 5m"),
    f!("features.api_client_distinct_customers_5m", Features, Integer, false, "Customers from same API client 5m"),
    f!("features.graph_distance_to_fraud", Features, Number, false, "Graph hops to nearest fraud customer (99 = none)"),
    f!("features.graph_fraud_neighbors_2", Features, Integer, false, "Fraud customers within 2 hops"),
    f!("features.graph_component_size", Features, Integer, false, "Connected component size (capped 1000)"),
    f!("features.graph_shared_entity_count", Features, Integer, false, "Entities shared with other customers"),
    f!("features.event_type", Features, Category, false, "Event type (categorical)"),
    f!("features.channel", Features, Category, false, "Channel (categorical)"),
    f!("features.payment_method", Features, Category, false, "Payment method (categorical)"),
    // ---- ml ----
    f!("ml.fraud_probability", Ml, Number, false, "Supervised model P(fraud) 0..1"),
    f!("ml.anomaly_score", Ml, Number, false, "Unsupervised anomaly score 0..1"),
    f!("ml.cluster_id", Ml, Integer, false, "Cluster id (-1 = noise)"),
    f!("ml.cluster_fraud_rate", Ml, Number, false, "Labelled fraud rate of the cluster"),
    f!("ml.model_version", Ml, Integer, false, "Active supervised model version"),
    // ---- graph ----
    f!("graph.distance_to_fraud", Graph, Number, false, "Hops to nearest fraud customer (+inf when none)"),
    f!("graph.fraud_neighbors_1", Graph, Integer, false, "Fraud customers at distance 1"),
    f!("graph.fraud_neighbors_2", Graph, Integer, false, "Fraud customers within 2 hops"),
    f!("graph.component_size", Graph, Integer, false, "Connected component size"),
    f!("graph.shared_entity_count", Graph, Integer, false, "Entities shared with other customers"),
    f!("graph.degree", Graph, Integer, false, "Distinct customers at distance 1"),
    f!("graph.community_fraud_rate", Graph, Number, false, "Fraud rate of the Louvain community"),
];

/// Feature names of feature set v1, in catalog order (without the `features.` prefix).
pub fn feature_names() -> impl Iterator<Item = &'static str> {
    BUILTIN_FIELDS
        .iter()
        .filter(|f| f.entity == Entity::Features)
        .filter_map(|f| f.path.strip_prefix("features."))
}

pub fn lookup(path: &str) -> Option<&'static CatalogField> {
    BUILTIN_FIELDS.iter().find(|f| f.path == path)
}

/// Paths whose children are open-ended and resolved at runtime (validated against
/// `core.field_catalog` for `source.*`, free-form for `customer.attributes.*`).
pub fn is_dynamic_path(path: &str) -> bool {
    path.starts_with("source.") || path.starts_with("customer.attributes.")
}

/// Maps a velocity field to its `core.events` SQL column.
///
/// Accepts both `event.device_id` and bare `device_id` (velocity definitions in rule-dsl.md use
/// bare names). Returns `None` for non-velocity fields; `source.*` paths are handled by the caller
/// via `payload #>> '{a,b}'` after checking `core.field_catalog.velocity_enabled`.
///
/// The returned string is a **compile-time constant**, so it is safe to splice into SQL.
pub fn velocity_column(path: &str) -> Option<&'static str> {
    let bare = path.strip_prefix("event.").unwrap_or(path);
    BUILTIN_FIELDS
        .iter()
        .filter(|f| f.entity == Entity::Event && f.velocity_enabled)
        .find(|f| f.path.strip_prefix("event.") == Some(bare))
        .and_then(|f| f.path.strip_prefix("event."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn paths_are_unique() {
        let mut seen = HashSet::new();
        for f in BUILTIN_FIELDS {
            assert!(seen.insert(f.path), "duplicate path {}", f.path);
        }
    }

    #[test]
    fn velocity_columns() {
        assert_eq!(velocity_column("event.device_id"), Some("device_id"));
        assert_eq!(
            velocity_column("instrument_fingerprint"),
            Some("instrument_fingerprint")
        );
        assert_eq!(velocity_column("customer_id"), Some("customer_id"));
        assert_eq!(velocity_column("event.card_last4"), None); // not velocity-enabled
        assert_eq!(velocity_column("event.amount; DROP TABLE x"), None);
        assert_eq!(velocity_column("features.cust_cnt_1h"), None);
    }

    #[test]
    fn feature_set_v1_contains_documented_features() {
        let names: Vec<_> = feature_names().collect();
        assert!(names.contains(&"amount_zscore_30d"));
        assert!(names.contains(&"promo_distinct_customers_same_code_device_7d"));
        assert_eq!(names.len(), 40);
    }

    #[test]
    fn dynamic_paths() {
        assert!(is_dynamic_path("source.order.total"));
        assert!(is_dynamic_path("customer.attributes.monthly_income"));
        assert!(!is_dynamic_path("event.amount"));
    }
}
