//! core-api / rule-service ⇄ graph-service (api-contract.md §C).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// Entity kinds that link customers (`graph.entities.kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    Email,
    Phone,
    Device,
    Ip,
    Card,
    BankAccount,
    Address,
    RefTransaction,
    ApiClient,
}

impl LinkKind {
    pub const ALL: [LinkKind; 9] = [
        Self::Email,
        Self::Phone,
        Self::Device,
        Self::Ip,
        Self::Card,
        Self::BankAccount,
        Self::Address,
        Self::RefTransaction,
        Self::ApiClient,
    ];

    /// Default link kinds when a project does not configure them (`ip`/`api_client` excluded:
    /// shared NAT/API clients create noisy links; opt-in only).
    pub const DEFAULT: [LinkKind; 7] = [
        Self::Email,
        Self::Phone,
        Self::Device,
        Self::Card,
        Self::BankAccount,
        Self::Address,
        Self::RefTransaction,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Email => "email",
            Self::Phone => "phone",
            Self::Device => "device",
            Self::Ip => "ip",
            Self::Card => "card",
            Self::BankAccount => "bank_account",
            Self::Address => "address",
            Self::RefTransaction => "ref_transaction",
            Self::ApiClient => "api_client",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct GraphCustomer {
    pub id: Uuid,
    pub external_id: String,
    /// `fraud` | `legit` | `unknown`
    pub risk_label: String,
    /// Normalised email (already normalised by core-api).
    #[serde(default)]
    pub email: Option<String>,
    /// E.164 phone.
    #[serde(default)]
    pub phone: Option<String>,
}

/// Linkable attributes of one event. Card/account values are **fingerprints**, never raw numbers.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct GraphEventLinks {
    pub id: Uuid,
    pub occurred_at: DateTime<Utc>,
    #[serde(default)]
    pub device_id: Option<String>,
    #[serde(default)]
    pub ip_address: Option<String>,
    #[serde(default)]
    pub instrument_fingerprint: Option<String>,
    /// For display: `bin` + `last4` of the instrument (masked value).
    #[serde(default)]
    pub card_bin: Option<String>,
    #[serde(default)]
    pub card_last4: Option<String>,
    #[serde(default)]
    pub recipient_fingerprint: Option<String>,
    #[serde(default)]
    pub shipping_address: Option<String>,
    #[serde(default)]
    pub billing_address: Option<String>,
    #[serde(default)]
    pub ref_transaction_id: Option<String>,
    #[serde(default)]
    pub api_client_id: Option<String>,
}

/// `POST /v1/projects/{pid}/links` — idempotent entity resolution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct GraphLinksRequest {
    pub customer: GraphCustomer,
    pub event: GraphEventLinks,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct GraphLinksResponse {
    pub entity_ids: Vec<i64>,
    pub new_links: u32,
    pub similarity_links_created: u32,
}

/// `POST /v1/projects/{pid}/metrics` — all pipeline metrics for one customer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct GraphMetricsRequest {
    pub customer_id: Uuid,
    /// Defaults to the project's `graph_config.link_kinds`.
    #[serde(default)]
    pub link_kinds: Option<Vec<LinkKind>>,
    #[serde(default)]
    pub include_similar: Option<bool>,
    #[serde(default)]
    pub max_depth: Option<u8>,
}

/// Graph metrics exposed to rules as `graph.*` and stored in `decisions.graph`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct GraphMetrics {
    /// Customer→customer hops to the nearest fraud customer; `None` when none within max depth
    /// (rules treat it as +∞; the feature `graph_distance_to_fraud` uses 99).
    pub distance_to_fraud: Option<u32>,
    pub fraud_neighbors_1: u32,
    pub fraud_neighbors_2: u32,
    pub component_size: u32,
    pub shared_entity_count: u32,
    pub degree: u32,
    /// Fraud rate of the customer's Louvain community; `None` when not computed.
    #[serde(default)]
    pub community_fraud_rate: Option<f64>,
    /// Fraud customers sharing an entity directly, by kind (for reason codes like `GRAPH_SHARED_CARD`).
    #[serde(default)]
    pub shared_with_fraud_kinds: Vec<LinkKind>,
}

/// Graph-rule metric names (rule-dsl §6.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum GraphMetric {
    DistanceToFraud,
    FraudNeighbors,
    SharedEntityCount,
    ComponentSize,
    Degree,
    CommunityFraudRate,
}

/// `POST /v1/projects/{pid}/metric` — one metric with rule-specific parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct GraphMetricRequest {
    pub customer_id: Uuid,
    pub metric: GraphMetric,
    pub link_kinds: Vec<LinkKind>,
    pub include_similar: bool,
    pub max_depth: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct GraphMetricResponse {
    /// `None` = +∞ for `distance_to_fraud`, or "not available" for `community_fraud_rate` (→ trapped).
    pub value: Option<f64>,
}

/// `PUT /v1/projects/{pid}/customers/{cid}/label`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct CustomerLabelUpdate {
    /// `fraud` | `legit` | `unknown`
    pub risk_label: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_kind_roundtrip() {
        for k in LinkKind::ALL {
            assert_eq!(LinkKind::parse(k.as_str()), Some(k));
        }
        assert_eq!(
            serde_json::to_value(LinkKind::BankAccount).ok(),
            Some(serde_json::json!("bank_account"))
        );
    }
}
