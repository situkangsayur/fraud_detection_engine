//! JSON model of rules and rulesets (rule-dsl §2–§7).
//!
//! The model is a direct, typed image of the JSON contract. Unknown fields are rejected
//! (`deny_unknown_fields`) so malformed input — including LLM-generated rules — fails loudly instead of being
//! silently ignored. Semantic checks that serde cannot express (field paths, formula parsing, allowed operand
//! positions, …) live in [`crate::validate`].

use std::collections::BTreeMap;
use std::fmt;

use serde::de::{self, Deserializer, MapAccess, Visitor};
use serde::ser::{SerializeMap, Serializer};
use serde::{Deserialize, Serialize};

use crate::duration::DurationSpec;

/// Fraud typologies known to the platform (architecture §1).
pub const TYPOLOGIES: &[&str] = &[
    "carding",
    "account_takeover",
    "bank_account_takeover",
    "system_breach",
    "promo_abuse",
    "refund_abuse",
    "money_mule",
    "other",
];

/// Entity kinds that can link customers in the graph (rule-dsl §6.5).
pub const LINK_KINDS: &[&str] = &[
    "email",
    "phone",
    "device",
    "ip",
    "card",
    "bank_account",
    "address",
    "ref_transaction",
    "api_client",
];

// ---------------------------------------------------------------------------------------------------------------
// Envelope
// ---------------------------------------------------------------------------------------------------------------

/// The complete rule as stored per version and as exchanged over the API (rule-dsl §2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleEnvelope {
    pub code: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub kind: RuleKind,
    #[serde(default)]
    pub typologies: Vec<String>,
    #[serde(default)]
    pub event_types: Vec<String>,
    pub risk_score: f64,
    #[serde(default)]
    pub trapped_score: f64,
    #[serde(default)]
    pub action: Action,
    #[serde(default)]
    pub on_trapped: OnTrapped,
    #[serde(default)]
    pub missing_as_no_match: bool,
    pub definition: RuleDefinition,
}

impl RuleEnvelope {
    /// Whether the rule applies to an event type (`event_types` empty = all).
    pub fn applies_to(&self, event_type: &str) -> bool {
        self.event_types.is_empty() || self.event_types.iter().any(|t| t == event_type)
    }
}

/// Rule kind discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleKind {
    Simple,
    Velocity,
    Composite,
    Reference,
    Graph,
}

impl RuleKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RuleKind::Simple => "simple",
            RuleKind::Velocity => "velocity",
            RuleKind::Composite => "composite",
            RuleKind::Reference => "reference",
            RuleKind::Graph => "graph",
        }
    }
}

impl fmt::Display for RuleKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Effect of a matched rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    #[default]
    Score,
    ForceReview,
    ForceDecline,
    ForceApprove,
}

/// Effect of a trapped rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnTrapped {
    #[default]
    Ignore,
    Score,
    Review,
}

/// Kind-specific rule body, tagged by `"kind"` (rule-dsl §6).
///
/// A closed sum type: the compiler forces every consumer to handle every kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuleDefinition {
    Simple(SimpleRule),
    // Large bodies are boxed so the enum stays small (serde treats `Box<T>` exactly like `T`).
    Velocity(Box<VelocitySpec>),
    Composite(Box<CompositeRule>),
    Reference(ReferenceRule),
    Graph(GraphRule),
}

impl RuleDefinition {
    pub fn kind(&self) -> RuleKind {
        match self {
            RuleDefinition::Simple(_) => RuleKind::Simple,
            RuleDefinition::Velocity(_) => RuleKind::Velocity,
            RuleDefinition::Composite(_) => RuleKind::Composite,
            RuleDefinition::Reference(_) => RuleKind::Reference,
            RuleDefinition::Graph(_) => RuleKind::Graph,
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Operands
// ---------------------------------------------------------------------------------------------------------------

/// A value source inside a condition (rule-dsl §3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operand {
    /// A literal: number, string, bool, null or array.
    Const { value: serde_json::Value },
    /// A path into the evaluation context, e.g. `event.amount`.
    Field { path: String },
    /// A formula whose variables are bound to other operands.
    Formula {
        expr: String,
        #[serde(default)]
        args: BTreeMap<String, Operand>,
    },
    /// An attribute of the matched reference-list entry (only inside `reference.attribute_condition`).
    Ref { path: String },
    /// A field of a historical row (only on the left side of `composite.history_filter`).
    Hist { path: String },
}

impl Operand {
    pub fn constant(value: impl Into<serde_json::Value>) -> Self {
        Operand::Const { value: value.into() }
    }

    pub fn field(path: impl Into<String>) -> Self {
        Operand::Field { path: path.into() }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Operand::Const { .. } => "const",
            Operand::Field { .. } => "field",
            Operand::Formula { .. } => "formula",
            Operand::Ref { .. } => "ref",
            Operand::Hist { .. } => "hist",
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Conditions
// ---------------------------------------------------------------------------------------------------------------

/// Comparison operators (rule-dsl §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    Eq,
    Ne,
    Gt,
    Gte,
    Lt,
    Lte,
    Between,
    In,
    NotIn,
    Contains,
    NotContains,
    StartsWith,
    EndsWith,
    Regex,
    IsNull,
    IsNotNull,
    Similar,
}

impl Op {
    pub fn as_str(self) -> &'static str {
        match self {
            Op::Eq => "eq",
            Op::Ne => "ne",
            Op::Gt => "gt",
            Op::Gte => "gte",
            Op::Lt => "lt",
            Op::Lte => "lte",
            Op::Between => "between",
            Op::In => "in",
            Op::NotIn => "not_in",
            Op::Contains => "contains",
            Op::NotContains => "not_contains",
            Op::StartsWith => "starts_with",
            Op::EndsWith => "ends_with",
            Op::Regex => "regex",
            Op::IsNull => "is_null",
            Op::IsNotNull => "is_not_null",
            Op::Similar => "similar",
        }
    }

    /// Unary operators take no right operand.
    pub fn is_unary(self) -> bool {
        matches!(self, Op::IsNull | Op::IsNotNull)
    }

    /// Operators allowed in `composite.history_filter` (compiled to SQL).
    pub fn allowed_in_history_filter(self) -> bool {
        matches!(
            self,
            Op::Eq
                | Op::Ne
                | Op::Gt
                | Op::Gte
                | Op::Lt
                | Op::Lte
                | Op::Between
                | Op::In
                | Op::NotIn
                | Op::IsNull
                | Op::IsNotNull
                | Op::StartsWith
                | Op::Contains
        )
    }
}

impl fmt::Display for Op {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// String similarity method for the `similar` operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimilarityMethod {
    #[default]
    JaroWinkler,
    LevenshteinRatio,
}

/// Per-leaf options.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeafOptions {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub case_insensitive: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<SimilarityMethod>,
}

impl LeafOptions {
    pub fn is_default(&self) -> bool {
        self == &LeafOptions::default()
    }
}

/// A single comparison `left op right`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Leaf {
    pub left: Operand,
    pub op: Op,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right: Option<Operand>,
    /// Weight used by `scoring: "weighted"` (default 1.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<f64>,
    #[serde(default, skip_serializing_if = "LeafOptions::is_default")]
    pub options: LeafOptions,
}

impl Leaf {
    pub fn weight(&self) -> f64 {
        self.weight.unwrap_or(1.0)
    }
}

/// `at_least` group body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AtLeast {
    pub n: usize,
    pub of: Vec<Condition>,
}

/// A condition tree: a leaf or a boolean group (rule-dsl §4).
///
/// JSON shape: a leaf object (`{left, op, right}`) or a single-key group object (`{"all": [...]}`,
/// `{"any": [...]}`, `{"not": {...}}`, `{"at_least": {"n": 2, "of": [...]}}`).
#[derive(Debug, Clone, PartialEq)]
pub enum Condition {
    Leaf(Leaf),
    All(Vec<Condition>),
    Any(Vec<Condition>),
    Not(Box<Condition>),
    AtLeast(AtLeast),
}

impl Condition {
    /// Visits every leaf in the tree (depth first).
    pub fn leaves(&self) -> Vec<&Leaf> {
        let mut out = Vec::new();
        self.collect_leaves(&mut out);
        out
    }

    fn collect_leaves<'a>(&'a self, out: &mut Vec<&'a Leaf>) {
        match self {
            Condition::Leaf(l) => out.push(l),
            Condition::All(items) | Condition::Any(items) => items.iter().for_each(|c| c.collect_leaves(out)),
            Condition::AtLeast(a) => a.of.iter().for_each(|c| c.collect_leaves(out)),
            Condition::Not(c) => c.collect_leaves(out),
        }
    }
}

impl Serialize for Condition {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Condition::Leaf(leaf) => leaf.serialize(serializer),
            Condition::All(items) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("all", items)?;
                map.end()
            }
            Condition::Any(items) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("any", items)?;
                map.end()
            }
            Condition::Not(inner) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("not", inner)?;
                map.end()
            }
            Condition::AtLeast(at_least) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("at_least", at_least)?;
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for Condition {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(ConditionVisitor)
    }
}

struct ConditionVisitor;

/// Deserializes a buffered JSON value into `T`, keeping the inner path in the error message.
fn from_buffered<T: serde::de::DeserializeOwned, E: de::Error>(
    key: &str,
    value: serde_json::Value,
) -> Result<T, E> {
    serde_path_to_error::deserialize(value).map_err(|err| {
        let inner = err.path().to_string();
        if inner == "." || inner.is_empty() {
            E::custom(format!("{key}: {}", err.inner()))
        } else {
            E::custom(format!("{key}.{inner}: {}", err.inner()))
        }
    })
}

impl<'de> Visitor<'de> for ConditionVisitor {
    type Value = Condition;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a condition: a leaf {left, op, right} or a group {all|any|not|at_least}")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Condition, A::Error> {
        let mut entries = serde_json::Map::new();
        while let Some((key, value)) = map.next_entry::<String, serde_json::Value>()? {
            if entries.insert(key.clone(), value).is_some() {
                return Err(de::Error::custom(format!("duplicate key `{key}`")));
            }
        }
        const GROUPS: [&str; 4] = ["all", "any", "not", "at_least"];
        let group_keys: Vec<&String> = entries.keys().filter(|k| GROUPS.contains(&k.as_str())).collect();
        match group_keys.len() {
            0 => from_buffered::<Leaf, _>("leaf", serde_json::Value::Object(entries)).map(Condition::Leaf),
            1 if entries.len() == 1 => {
                let (key, value) = entries
                    .into_iter()
                    .next()
                    .ok_or_else(|| de::Error::custom("empty group"))?;
                match key.as_str() {
                    "all" => from_buffered(&key, value).map(Condition::All),
                    "any" => from_buffered(&key, value).map(Condition::Any),
                    "not" => from_buffered::<Condition, _>(&key, value).map(|c| Condition::Not(Box::new(c))),
                    _ => from_buffered(&key, value).map(Condition::AtLeast),
                }
            }
            _ => Err(de::Error::custom(format!(
                "a group condition must have exactly one key of all/any/not/at_least, found keys: {}",
                entries.keys().cloned().collect::<Vec<_>>().join(", ")
            ))),
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Kind bodies
// ---------------------------------------------------------------------------------------------------------------

/// Scoring mode of a simple rule (rule-dsl §6.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scoring {
    #[default]
    Binary,
    Weighted,
}

/// `simple` rule body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimpleRule {
    pub when: Condition,
    #[serde(default)]
    pub scoring: Scoring,
}

/// A comparison of a computed value against an operand.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Compare {
    pub op: Op,
    pub right: Operand,
}

/// History window: sliding duration or last N events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum Window {
    Duration { duration: DurationSpec },
    LastN { last_n: u32 },
}

impl Window {
    pub fn label(&self) -> String {
        match self {
            Window::Duration { duration } => duration.to_string(),
            Window::LastN { last_n } => format!("last_{last_n}"),
        }
    }
}

/// Aggregate function over history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AggFn {
    Count,
    Sum,
    Avg,
    Min,
    Max,
    DistinctCount,
    Stddev,
    Median,
    Percentile,
}

impl AggFn {
    pub fn as_str(self) -> &'static str {
        match self {
            AggFn::Count => "count",
            AggFn::Sum => "sum",
            AggFn::Avg => "avg",
            AggFn::Min => "min",
            AggFn::Max => "max",
            AggFn::DistinctCount => "distinct_count",
            AggFn::Stddev => "stddev",
            AggFn::Median => "median",
            AggFn::Percentile => "percentile",
        }
    }
}

/// `aggregate` of a velocity rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Aggregate {
    #[serde(rename = "fn")]
    pub func: AggFn,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    /// Percentile in (0, 1) for `fn = percentile`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p: Option<f64>,
}

/// Tail of a gaussian tail probability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tail {
    #[default]
    Upper,
    Lower,
    Two,
}

/// Output of the linear-trend statistic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrendOutput {
    Slope,
    Forecast,
    ResidualZ,
}

/// Statistical transformation of the history (rule-dsl §6.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "fn", rename_all = "snake_case", deny_unknown_fields)]
pub enum Statistic {
    Zscore {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        of: Option<Operand>,
    },
    GaussianTail {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        of: Option<Operand>,
        #[serde(default)]
        tail: Tail,
    },
    PercentileRank {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        of: Option<Operand>,
    },
    LinearTrend {
        bucket: DurationSpec,
        output: TrendOutput,
    },
    PoissonTail {
        bucket: DurationSpec,
    },
}

impl Statistic {
    pub fn name(&self) -> &'static str {
        match self {
            Statistic::Zscore { .. } => "zscore",
            Statistic::GaussianTail { .. } => "gaussian_tail",
            Statistic::PercentileRank { .. } => "percentile_rank",
            Statistic::LinearTrend { .. } => "linear_trend",
            Statistic::PoissonTail { .. } => "poisson_tail",
        }
    }

    /// Statistics computed over bucketed series (instead of per-event values).
    pub fn is_bucketed(&self) -> bool {
        matches!(
            self,
            Statistic::LinearTrend { .. } | Statistic::PoissonTail { .. }
        )
    }
}

/// `velocity` rule body; also the `velocity` part of a composite rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VelocitySpec {
    /// Event types of the history; empty = same type as the current event.
    #[serde(default)]
    pub history_event_types: Vec<String>,
    pub group_by: Vec<String>,
    pub window: Window,
    pub aggregate: Aggregate,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statistic: Option<Statistic>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_current: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_samples: Option<u32>,
    pub compare: Compare,
}

impl VelocitySpec {
    /// Effective `include_current` (rule-dsl §6.2: true for plain aggregates, false with a statistic).
    pub fn include_current(&self) -> bool {
        self.include_current.unwrap_or(self.statistic.is_none())
    }

    /// Effective `min_samples` (5 with a statistic, otherwise 0).
    pub fn min_samples(&self) -> u32 {
        self.min_samples
            .unwrap_or(if self.statistic.is_some() { 5 } else { 0 })
    }
}

/// `composite` rule body (rule-dsl §6.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositeRule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<Condition>,
    pub history_filter: Condition,
    pub velocity: VelocitySpec,
}

/// Reference-list matching mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefMode {
    Exists,
    NotExists,
    Attribute,
}

/// `reference` rule body (rule-dsl §6.4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceRule {
    pub list: String,
    pub key: Operand,
    pub mode: RefMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribute_condition: Option<Condition>,
}

/// Graph metrics available to graph rules (rule-dsl §6.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphMetric {
    DistanceToFraud,
    FraudNeighbors,
    SharedEntityCount,
    ComponentSize,
    Degree,
    CommunityFraudRate,
}

impl GraphMetric {
    pub fn as_str(self) -> &'static str {
        match self {
            GraphMetric::DistanceToFraud => "distance_to_fraud",
            GraphMetric::FraudNeighbors => "fraud_neighbors",
            GraphMetric::SharedEntityCount => "shared_entity_count",
            GraphMetric::ComponentSize => "component_size",
            GraphMetric::Degree => "degree",
            GraphMetric::CommunityFraudRate => "community_fraud_rate",
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_max_depth() -> u8 {
    3
}

/// `graph` rule body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphRule {
    pub metric: GraphMetric,
    /// Entity kinds that count as links; empty = the project's default link kinds.
    #[serde(default)]
    pub link_kinds: Vec<String>,
    #[serde(default = "default_true")]
    pub include_similar: bool,
    #[serde(default = "default_max_depth")]
    pub max_depth: u8,
    pub compare: Compare,
}

// ---------------------------------------------------------------------------------------------------------------
// Rulesets
// ---------------------------------------------------------------------------------------------------------------

/// How rule contributions combine into a ruleset score (rule-dsl §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Aggregation {
    Sum,
    Max,
    #[default]
    ProbabilisticOr,
    WeightedAverage,
}

fn default_weight() -> f64 {
    1.0
}

fn default_max_score() -> f64 {
    100.0
}

/// Membership of a rule in a ruleset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RulesetMember {
    pub rule_id: String,
    #[serde(default = "default_weight")]
    pub weight: f64,
    #[serde(default)]
    pub pinned_version: Option<i32>,
}

/// A ruleset as exchanged over the API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RulesetSpec {
    pub code: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub event_types: Vec<String>,
    #[serde(default)]
    pub typologies: Vec<String>,
    #[serde(default)]
    pub aggregation: Aggregation,
    #[serde(default = "default_max_score")]
    pub max_score: f64,
    #[serde(default)]
    pub rules: Vec<RulesetMember>,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use serde_json::json;

    #[test]
    fn condition_leaf_and_groups_round_trip() {
        let j = json!({"all": [
            {"left": {"type": "field", "path": "event.amount"}, "op": "gt", "right": {"type": "const", "value": 5}},
            {"not": {"left": {"type": "field", "path": "event.x"}, "op": "is_null"}},
            {"at_least": {"n": 1, "of": [{"any": []}]}}
        ]});
        let c: Condition = serde_json::from_value(j.clone()).unwrap();
        assert_eq!(serde_json::to_value(&c).unwrap(), j);
        assert_eq!(c.leaves().len(), 2);
    }

    #[test]
    fn condition_rejects_mixed_group_keys() {
        let err = serde_json::from_value::<Condition>(json!({"all": [], "any": []})).unwrap_err();
        assert!(err.to_string().contains("exactly one key"), "{err}");
    }

    #[test]
    fn leaf_rejects_unknown_fields_and_ops() {
        let err = serde_json::from_value::<Condition>(json!(
            {"left": {"type": "field", "path": "a"}, "op": "greater", "right": {"type": "const", "value": 1}}
        ))
        .unwrap_err();
        assert!(err.to_string().contains("greater"), "{err}");
        let err = serde_json::from_value::<Condition>(json!(
            {"left": {"type": "field", "path": "a"}, "op": "gt", "rigth": {"type": "const", "value": 1}}
        ))
        .unwrap_err();
        assert!(err.to_string().contains("rigth"), "{err}");
    }

    #[test]
    fn operand_rejects_unknown_fields() {
        assert!(serde_json::from_value::<Operand>(json!({"type": "field", "path": "a", "x": 1})).is_err());
        assert!(serde_json::from_value::<Operand>(json!({"type": "column", "path": "a"})).is_err());
    }

    #[test]
    fn nested_error_path_is_reported() {
        let err = serde_json::from_value::<Condition>(json!({"all": [
            {"left": {"type": "field", "path": "a"}, "op": "gt", "right": {"type": "const", "value": 1}},
            {"left": {"type": "field", "path": "a"}, "op": "bogus"}
        ]}))
        .unwrap_err();
        assert!(
            err.to_string().contains("all.[1]") || err.to_string().contains("all[1]"),
            "{err}"
        );
    }

    #[test]
    fn velocity_defaults() {
        let v: VelocitySpec = serde_json::from_value(json!({
            "group_by": ["customer_id"], "window": {"duration": "1h"},
            "aggregate": {"fn": "count"}, "compare": {"op": "gt", "right": {"type": "const", "value": 5}}
        }))
        .unwrap();
        assert!(v.include_current());
        assert_eq!(v.min_samples(), 0);
        let v2 = VelocitySpec {
            statistic: Some(Statistic::Zscore { of: None }),
            ..v
        };
        assert!(!v2.include_current());
        assert_eq!(v2.min_samples(), 5);
    }

    #[test]
    fn window_variants() {
        let w: Window = serde_json::from_value(json!({"last_n": 50})).unwrap();
        assert_eq!(w, Window::LastN { last_n: 50 });
        assert!(serde_json::from_value::<Window>(json!({"duration": "5q"})).is_err());
    }
}
