//! Ports: the only way the engine reaches data outside the evaluation context.
//!
//! `rule-service` implements [`DataProvider`] with SQL against `core.events` (velocity/composite), SQL against
//! `rules.reference_*` (reference lists) and HTTP to graph-service (graph metrics). Tests implement it in memory.
//!
//! The queries are **fully resolved** by the engine: group-by values are already read from the current event,
//! history-filter right-hand sides are already evaluated to constants, and field names are history column names
//! (`amount`, `customer_id`, `source.order.total`). An adapter only has to translate them into SQL — with the
//! field names checked against the project's field catalog and every value bound as a parameter.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use thiserror::Error;

use crate::model::{AggFn, GraphMetric, Op};

/// Error returned by a data provider; the rule evaluates to `trapped` with this reason.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProviderError {
    /// The provider gave up because of its time budget.
    #[error("timeout")]
    Timeout,
    /// The query referenced something the adapter cannot serve (unknown/unsafe field, …).
    #[error("invalid_query: {0}")]
    InvalidQuery(String),
    /// Any other failure (database, network…).
    #[error("provider_error: {0}")]
    Other(String),
}

/// The sliding window of a velocity query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryWindow {
    /// Events with `occurred_at` in `(anchor − seconds, anchor]`.
    Duration { seconds: i64 },
    /// The last `n` events up to the anchor (inclusive).
    LastN { n: u32 },
}

/// One `group_by` constraint: history rows where `field` equals the current event's `value`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GroupKey {
    pub field: String,
    pub value: serde_json::Value,
}

/// Comparison in a compiled history filter.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HistPredicate {
    /// History field (`promo_code`, `source.order.channel`).
    pub field: String,
    /// One of the operators allowed in history filters ([`Op::allowed_in_history_filter`]).
    pub op: Op,
    /// Constant right-hand side (array for `in`/`not_in`/`between`; `Null` for `is_null`/`is_not_null`).
    pub value: serde_json::Value,
}

/// A compiled history filter (a boolean tree of predicates, SQL-compilable).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HistFilter {
    And(Vec<HistFilter>),
    Or(Vec<HistFilter>),
    Not(Box<HistFilter>),
    Pred(HistPredicate),
}

/// Which series the engine needs besides the aggregate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SeriesRequest {
    /// Only the aggregate value and the sample count.
    None,
    /// Per-event values of `aggregate.field` (numeric), oldest → newest, for z-score / gaussian / percentile.
    Values,
    /// Bucketed series ending with the bucket that contains the anchor. Buckets are aligned so that the last
    /// bucket is `(anchor − bucket_seconds, anchor]`, the previous one ends where it starts, and so on back to
    /// the window start. `func` is applied per bucket. Empty buckets must be present with value `0` for
    /// `count`/`sum`/`distinct_count` and may be omitted for other functions.
    Buckets { bucket_seconds: i64, func: AggFn },
}

/// A fully-resolved velocity query (velocity and composite rules).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VelocityQuery {
    /// Current event id; excluded from history unless `include_current`.
    pub event_id: Option<String>,
    /// End of the window (the current event's `occurred_at`).
    pub anchor: DateTime<Utc>,
    /// History event types (never empty: defaults to the current event type).
    pub history_event_types: Vec<String>,
    /// AND of equality constraints; never empty.
    pub group_by: Vec<GroupKey>,
    pub window: QueryWindow,
    pub aggregate_fn: AggFn,
    /// Aggregated field; `None` only for `count`.
    pub aggregate_field: Option<String>,
    /// Percentile in (0,1) for `percentile`.
    pub percentile: Option<f64>,
    pub include_current: bool,
    /// Composite history filter (None for plain velocity rules).
    pub filter: Option<HistFilter>,
    pub series: SeriesRequest,
}

/// One point of a bucketed series.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BucketPoint {
    pub start: DateTime<Utc>,
    pub value: f64,
}

/// Result of a velocity query.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct VelocityData {
    /// The aggregate over the whole window; `None` when undefined (e.g. `avg` of no rows).
    pub aggregate: Option<f64>,
    /// Number of history rows that matched (after filter and include_current).
    pub samples: u64,
    /// Filled when `SeriesRequest::Values` was requested.
    pub values: Vec<f64>,
    /// Filled when `SeriesRequest::Buckets` was requested (oldest → newest, last = current bucket).
    pub buckets: Vec<BucketPoint>,
}

/// Result of a reference-list lookup.
#[derive(Debug, Clone, PartialEq)]
pub enum RefLookup {
    /// No list with that name in the project nor tenant-wide.
    UnknownList,
    /// The list exists but has no entry for the key.
    NotFound,
    /// Entry found; `valid` is false when outside `valid_from`/`valid_until` (treated as not found).
    Found {
        attributes: serde_json::Value,
        valid: bool,
    },
}

/// A graph metric request (graph rules).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GraphMetricQuery {
    pub customer_id: String,
    pub metric: GraphMetric,
    /// Empty = the project's default link kinds.
    pub link_kinds: Vec<String>,
    pub include_similar: bool,
    pub max_depth: u8,
}

/// Data access port implemented by the infrastructure (rule-service) and by test doubles.
///
/// Implementations must be cheap to call concurrently (`Send + Sync`); the engine evaluates independent rules
/// in parallel.
#[async_trait]
pub trait DataProvider: Send + Sync {
    /// Aggregates history for velocity/composite rules.
    async fn velocity(&self, query: &VelocityQuery) -> Result<VelocityData, ProviderError>;

    /// Looks up `key` in the reference list `list` (project list first, then tenant-wide).
    async fn reference_lookup(&self, list: &str, key: &str) -> Result<RefLookup, ProviderError>;

    /// Computes a graph metric. `Ok(None)` means "not available" (no fraud within depth for
    /// `distance_to_fraud` → +∞; no community for `community_fraud_rate` → trapped).
    async fn graph_metric(&self, query: &GraphMetricQuery) -> Result<Option<f64>, ProviderError>;
}

/// Abstract timer so the pure engine can enforce per-rule time budgets without depending on a runtime.
/// rule-service implements it with `tokio::time::sleep`.
#[async_trait]
pub trait Timer: Send + Sync {
    async fn sleep(&self, duration: std::time::Duration);
}
