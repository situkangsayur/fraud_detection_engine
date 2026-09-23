//! Rule evaluation with three-valued (Kleene) logic (rule-dsl §1, §4, §6).
//!
//! A rule evaluates to [`Outcome::Match`], [`Outcome::NoMatch`] or [`Outcome::Trapped`]. Trapping is not an
//! exception: it is a first-class result ("we could not decide"), carried through boolean groups with Kleene
//! semantics, logged in traces and optionally scored or sent to review by the ruleset layer.
//!
//! Dispatch over rule kinds is a `match` on [`RuleDefinition`] — each arm is a small function. This replaces the
//! virtual `evaluate()` of a Java class hierarchy while keeping all kinds visible in one place.

pub mod compare;
pub mod condition;
pub mod stats;

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{json, Value as Json};

use crate::cache;
use crate::context::{history_field_context_path, EvalContext, Resolved};
use crate::model::{
    AggFn, Compare, CompositeRule, GraphMetric, GraphRule, Op, Operand, RefMode, ReferenceRule,
    RuleDefinition, RuleEnvelope, Scoring, SimpleRule, Statistic, VelocitySpec, Window,
};
use crate::ports::{
    DataProvider, GraphMetricQuery, GroupKey, QueryWindow, RefLookup, SeriesRequest, VelocityData,
    VelocityQuery,
};
use crate::value::{number_to_json, Value};

use condition::{compile_history_filter, eval_condition, Tri};

// ---------------------------------------------------------------------------------------------------------------
// Outcome & traps
// ---------------------------------------------------------------------------------------------------------------

/// Result of evaluating a rule.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Match,
    NoMatch,
    /// Could not be evaluated; the string is the reason (`null_operand: event.amount`, `division_by_zero`, …).
    Trapped(String),
}

impl Outcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            Outcome::Match => "match",
            Outcome::NoMatch => "no_match",
            Outcome::Trapped(_) => "trapped",
        }
    }

    pub fn trapped_reason(&self) -> Option<&str> {
        match self {
            Outcome::Trapped(reason) => Some(reason),
            _ => None,
        }
    }
}

/// Why something could not be evaluated. `missing` marks traps caused by a missing/null *field* value, which
/// `missing_as_no_match` turns into `no_match`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trap {
    pub reason: String,
    pub missing: bool,
}

impl Trap {
    pub fn error(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            missing: false,
        }
    }

    pub fn missing(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            missing: true,
        }
    }
}

/// Full result of one rule evaluation.
#[derive(Debug, Clone, PartialEq)]
pub struct RuleEvaluation {
    pub outcome: Outcome,
    /// Share of `risk_score` earned on match: 1.0 except for `scoring: weighted` simple rules.
    pub fraction: f64,
    /// Kind-specific explanation (rule-dsl §7 `trace`).
    pub trace: Json,
}

impl RuleEvaluation {
    fn matched(fraction: f64, trace: Json) -> Self {
        Self {
            outcome: Outcome::Match,
            fraction,
            trace,
        }
    }

    fn no_match(trace: Json) -> Self {
        Self {
            outcome: Outcome::NoMatch,
            fraction: 0.0,
            trace,
        }
    }

    fn from_bool(hit: bool, trace: Json) -> Self {
        if hit {
            Self::matched(1.0, trace)
        } else {
            Self::no_match(trace)
        }
    }

    fn trapped(trap: &Trap, missing_as_no_match: bool, mut trace: Json) -> Self {
        if let Json::Object(map) = &mut trace {
            map.insert("trapped_reason".into(), Json::String(trap.reason.clone()));
        }
        if trap.missing && missing_as_no_match {
            return Self::no_match(trace);
        }
        Self {
            outcome: Outcome::Trapped(trap.reason.clone()),
            fraction: 0.0,
            trace,
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Operand evaluation
// ---------------------------------------------------------------------------------------------------------------

/// Where operands are evaluated: the context, plus reference-entry attributes inside `attribute_condition`.
#[derive(Debug, Clone, Copy)]
pub struct Scope<'a> {
    pub ctx: &'a EvalContext,
    pub refs: Option<&'a Json>,
    pub missing_as_no_match: bool,
}

impl<'a> Scope<'a> {
    pub fn new(ctx: &'a EvalContext, missing_as_no_match: bool) -> Self {
        Self {
            ctx,
            refs: None,
            missing_as_no_match,
        }
    }
}

/// Short human label of an operand for traces and trap reasons.
pub fn operand_label(operand: &Operand) -> String {
    match operand {
        Operand::Const { value } => value.to_string(),
        Operand::Field { path } => path.clone(),
        Operand::Formula { expr, .. } => format!("formula({expr})"),
        Operand::Ref { path } => format!("ref.{path}"),
        Operand::Hist { path } => format!("hist.{path}"),
    }
}

fn resolved_to_value(resolved: Resolved<'_>) -> Value {
    match resolved {
        Resolved::Value(json) => Value::from_json(json),
        Resolved::Missing | Resolved::Null => Value::Null,
    }
}

/// Evaluates an operand. Missing/null fields evaluate to [`Value::Null`] (comparisons decide what that means).
pub fn eval_operand(operand: &Operand, scope: &Scope<'_>) -> Result<Value, Trap> {
    match operand {
        Operand::Const { value } => Ok(Value::from_json(value)),
        Operand::Field { path } => Ok(resolved_to_value(scope.ctx.resolve(path))),
        Operand::Ref { path } => match scope.refs {
            Some(attributes) => Ok(resolved_to_value(crate::context::resolve_path(attributes, path))),
            None => Err(Trap::error(format!(
                "invalid_operand: ref.{path} is only allowed in attribute_condition"
            ))),
        },
        Operand::Hist { path } => Err(Trap::error(format!(
            "invalid_operand: hist.{path} is only allowed in history_filter"
        ))),
        Operand::Formula { expr, args } => eval_formula(expr, args, scope).map(Value::Number),
    }
}

fn eval_formula(expr: &str, args: &BTreeMap<String, Operand>, scope: &Scope<'_>) -> Result<f64, Trap> {
    let formula = cache::formula(expr).map_err(|e| Trap::error(format!("formula_error: {e}")))?;
    let names = args.keys().cloned().collect();
    formula
        .check_bindings(&names)
        .map_err(|e| Trap::error(format!("formula_error: {e}")))?;
    let mut vars = BTreeMap::new();
    for (name, arg) in args {
        let value = eval_operand(arg, scope)?;
        if value.is_null() {
            return Err(Trap::missing(format!(
                "null_operand: formula arg '{name}' ({})",
                operand_label(arg)
            )));
        }
        let number = value.as_formula_number().ok_or_else(|| {
            Trap::error(format!(
                "non_numeric_arg: formula arg '{name}' is {}",
                value.type_name()
            ))
        })?;
        vars.insert(name.clone(), number);
    }
    formula.eval(&vars).map_err(|e| Trap::error(e.to_string()))
}

/// Evaluates `value op right` for velocity/graph comparisons.
fn eval_compare(value: f64, compare: &Compare, scope: &Scope<'_>) -> Result<(bool, Json), Trap> {
    let right = eval_operand(&compare.right, scope)?;
    let hit = compare::compare(
        &Value::Number(value),
        compare.op,
        Some(&right),
        &Default::default(),
        "value",
        &operand_label(&compare.right),
    )?;
    Ok((hit, right.to_json()))
}

// ---------------------------------------------------------------------------------------------------------------
// Rule evaluation
// ---------------------------------------------------------------------------------------------------------------

/// Evaluates one rule against the context. Never fails: problems become [`Outcome::Trapped`].
pub async fn evaluate_rule(
    rule: &RuleEnvelope,
    ctx: &EvalContext,
    provider: &dyn DataProvider,
) -> RuleEvaluation {
    let scope = Scope::new(ctx, rule.missing_as_no_match);
    match &rule.definition {
        RuleDefinition::Simple(simple) => eval_simple(simple, &scope),
        RuleDefinition::Velocity(spec) => eval_velocity(spec, None, &scope, provider).await,
        RuleDefinition::Composite(composite) => eval_composite(composite, &scope, provider).await,
        RuleDefinition::Reference(reference) => eval_reference(reference, &scope, provider).await,
        RuleDefinition::Graph(graph) => eval_graph(graph, &scope, provider).await,
    }
}

fn eval_simple(rule: &SimpleRule, scope: &Scope<'_>) -> RuleEvaluation {
    let result = eval_condition(&rule.when, scope);
    let trace = json!({ "when": result.trace, "scoring": rule.scoring });
    match (result.tri, rule.scoring) {
        (Tri::Unknown, _) => {
            let trap = result.trap.unwrap_or_else(|| Trap::error("trapped"));
            RuleEvaluation::trapped(&trap, false, trace)
        }
        (Tri::True, Scoring::Binary) => RuleEvaluation::matched(1.0, trace),
        (Tri::False, Scoring::Binary) => RuleEvaluation::no_match(trace),
        (_, Scoring::Weighted) => {
            let fraction = if result.total_weight > 0.0 {
                result.matched_weight / result.total_weight
            } else {
                0.0
            };
            let mut trace = trace;
            if let Json::Object(map) = &mut trace {
                map.insert("fraction".into(), number_to_json(fraction));
            }
            if fraction > 0.0 {
                RuleEvaluation::matched(fraction, trace)
            } else {
                RuleEvaluation::no_match(trace)
            }
        }
    }
}

/// Reads the current event's value of a history field (`group_by`, default statistic input).
fn current_history_value(field: &str, ctx: &EvalContext) -> Option<Json> {
    match ctx.resolve(&history_field_context_path(field)) {
        Resolved::Value(v) => Some(v.clone()),
        Resolved::Missing | Resolved::Null => {
            if field == "customer_id" {
                ctx.customer_id.as_ref().map(|id| Json::String(id.clone()))
            } else {
                None
            }
        }
    }
}

fn series_request(spec: &VelocitySpec) -> SeriesRequest {
    match &spec.statistic {
        None => SeriesRequest::None,
        Some(
            Statistic::Zscore { .. } | Statistic::GaussianTail { .. } | Statistic::PercentileRank { .. },
        ) => SeriesRequest::Values,
        Some(Statistic::LinearTrend { bucket, .. }) => SeriesRequest::Buckets {
            bucket_seconds: bucket.seconds(),
            func: spec.aggregate.func,
        },
        Some(Statistic::PoissonTail { bucket }) => SeriesRequest::Buckets {
            bucket_seconds: bucket.seconds(),
            func: AggFn::Count,
        },
    }
}

/// Builds the fully-resolved provider query for a velocity spec.
pub fn build_velocity_query(
    spec: &VelocitySpec,
    filter: Option<crate::ports::HistFilter>,
    ctx: &EvalContext,
) -> Result<VelocityQuery, Trap> {
    let mut group_by = Vec::with_capacity(spec.group_by.len());
    for field in &spec.group_by {
        let value = current_history_value(field, ctx)
            .ok_or_else(|| Trap::missing(format!("null_group_by_value: {field}")))?;
        group_by.push(GroupKey {
            field: field.clone(),
            value,
        });
    }
    if group_by.is_empty() {
        return Err(Trap::error("invalid_rule: group_by must not be empty"));
    }
    let history_event_types = if spec.history_event_types.is_empty() {
        vec![ctx.event_type.clone()]
    } else {
        spec.history_event_types.clone()
    };
    let window = match &spec.window {
        Window::Duration { duration } => QueryWindow::Duration {
            seconds: duration.seconds(),
        },
        Window::LastN { last_n } => QueryWindow::LastN { n: *last_n },
    };
    Ok(VelocityQuery {
        event_id: ctx.event_id.clone(),
        anchor: ctx.occurred_at,
        history_event_types,
        group_by,
        window,
        aggregate_fn: spec.aggregate.func,
        aggregate_field: spec.aggregate.field.clone(),
        percentile: spec.aggregate.p,
        include_current: spec.include_current(),
        filter,
        series: series_request(spec),
    })
}

/// Value of a statistic's `of` operand, defaulting to the current event's `aggregate.field`.
fn statistic_input(of: Option<&Operand>, spec: &VelocitySpec, scope: &Scope<'_>) -> Result<f64, Trap> {
    let (value, label) = match of {
        Some(operand) => (eval_operand(operand, scope)?, operand_label(operand)),
        None => {
            let field = spec
                .aggregate
                .field
                .as_deref()
                .ok_or_else(|| Trap::error("invalid_rule: statistic needs aggregate.field or `of`"))?;
            let value = current_history_value(field, scope.ctx).map_or(Value::Null, |j| Value::from_json(&j));
            (value, history_field_context_path(field))
        }
    };
    if value.is_null() {
        return Err(Trap::missing(format!("null_operand: {label}")));
    }
    value
        .as_number()
        .ok_or_else(|| Trap::error(format!("type_mismatch: statistic input {label} is not numeric")))
}

/// Turns provider data into the value compared by the rule.
fn velocity_value(spec: &VelocitySpec, data: &VelocityData, scope: &Scope<'_>) -> Result<f64, Trap> {
    let min = spec.min_samples() as usize;
    match &spec.statistic {
        None => {
            if (data.samples as usize) < min {
                return Err(Trap::error(format!(
                    "insufficient_history: {} samples < {min} required",
                    data.samples
                )));
            }
            match (data.aggregate, spec.aggregate.func) {
                (Some(v), _) => Ok(v),
                (None, AggFn::Count | AggFn::Sum | AggFn::DistinctCount) => Ok(0.0),
                (None, _) => Err(Trap::error(
                    "insufficient_history: aggregate undefined over empty history",
                )),
            }
        }
        Some(Statistic::Zscore { of }) => {
            stats::zscore(statistic_input(of.as_ref(), spec, scope)?, &data.values, min)
        }
        Some(Statistic::GaussianTail { of, tail }) => stats::gaussian_tail(
            statistic_input(of.as_ref(), spec, scope)?,
            &data.values,
            *tail,
            min,
        ),
        Some(Statistic::PercentileRank { of }) => {
            stats::percentile_rank(statistic_input(of.as_ref(), spec, scope)?, &data.values, min)
        }
        Some(Statistic::LinearTrend { output, .. }) => stats::linear_trend(&data.buckets, *output, min),
        Some(Statistic::PoissonTail { .. }) => stats::poisson_tail(&data.buckets, min),
    }
}

async fn eval_velocity(
    spec: &VelocitySpec,
    filter: Option<crate::ports::HistFilter>,
    scope: &Scope<'_>,
    provider: &dyn DataProvider,
) -> RuleEvaluation {
    let mut trace = json!({
        "window": spec.window.label(),
        "aggregate": spec.aggregate.func.as_str(),
        "op": spec.compare.op,
    });
    if let Some(statistic) = &spec.statistic {
        trace["statistic"] = Json::String(statistic.name().to_string());
    }
    let result: Result<(bool, f64, Json, u64), Trap> = async {
        let query = build_velocity_query(spec, filter, scope.ctx)?;
        trace["group_by"] = Json::Object(
            query
                .group_by
                .iter()
                .map(|g| (g.field.clone(), g.value.clone()))
                .collect(),
        );
        let data = provider
            .velocity(&query)
            .await
            .map_err(|e| Trap::error(e.to_string()))?;
        let value = velocity_value(spec, &data, scope)?;
        let (hit, right) = eval_compare(value, &spec.compare, scope)?;
        Ok((hit, value, right, data.samples))
    }
    .await;
    match result {
        Ok((hit, value, right, samples)) => {
            trace["value"] = number_to_json(value);
            trace["right"] = right;
            trace["samples"] = json!(samples);
            RuleEvaluation::from_bool(hit, trace)
        }
        Err(trap) => RuleEvaluation::trapped(&trap, scope.missing_as_no_match, trace),
    }
}

async fn eval_composite(
    rule: &CompositeRule,
    scope: &Scope<'_>,
    provider: &dyn DataProvider,
) -> RuleEvaluation {
    let mut gate_trace = Json::Null;
    if let Some(gate) = &rule.gate {
        let gate_result = eval_condition(gate, scope);
        gate_trace = gate_result.trace.clone();
        match gate_result.tri {
            Tri::False => return RuleEvaluation::no_match(json!({ "gate": gate_trace })),
            Tri::Unknown => {
                let trap = gate_result.trap.unwrap_or_else(|| Trap::error("trapped"));
                return RuleEvaluation::trapped(&trap, false, json!({ "gate": gate_trace }));
            }
            Tri::True => {}
        }
    }
    let filter = match compile_history_filter(&rule.history_filter, scope) {
        Ok(filter) => filter,
        Err(trap) => {
            return RuleEvaluation::trapped(&trap, scope.missing_as_no_match, json!({ "gate": gate_trace }))
        }
    };
    let filter_json = serde_json::to_value(&filter).unwrap_or(Json::Null);
    let mut evaluation = eval_velocity(&rule.velocity, Some(filter), scope, provider).await;
    if let Json::Object(map) = &mut evaluation.trace {
        map.insert("gate".into(), gate_trace);
        map.insert("history_filter".into(), filter_json);
    }
    evaluation
}

async fn eval_reference(
    rule: &ReferenceRule,
    scope: &Scope<'_>,
    provider: &dyn DataProvider,
) -> RuleEvaluation {
    let mut trace = json!({ "list": rule.list, "mode": rule.mode });
    let key = match eval_operand(&rule.key, scope) {
        Ok(Value::Null) => {
            let trap = Trap::missing(format!("null_operand: {}", operand_label(&rule.key)));
            return RuleEvaluation::trapped(&trap, scope.missing_as_no_match, trace);
        }
        Ok(value) => match value.as_text() {
            Some(text) => text,
            None => {
                let trap = Trap::error(format!("type_mismatch: reference key is {}", value.type_name()));
                return RuleEvaluation::trapped(&trap, false, trace);
            }
        },
        Err(trap) => return RuleEvaluation::trapped(&trap, scope.missing_as_no_match, trace),
    };
    trace["key"] = Json::String(key.clone());
    let lookup = match provider.reference_lookup(&rule.list, &key).await {
        Ok(lookup) => lookup,
        Err(e) => return RuleEvaluation::trapped(&Trap::error(e.to_string()), false, trace),
    };
    let attributes = match lookup {
        RefLookup::UnknownList => {
            return RuleEvaluation::trapped(&Trap::error("unknown_reference_list"), false, trace);
        }
        RefLookup::Found {
            attributes,
            valid: true,
        } => Some(attributes),
        RefLookup::NotFound | RefLookup::Found { valid: false, .. } => None,
    };
    trace["found"] = Json::Bool(attributes.is_some());
    match (rule.mode, attributes) {
        (RefMode::Exists, found) => RuleEvaluation::from_bool(found.is_some(), trace),
        (RefMode::NotExists, found) => RuleEvaluation::from_bool(found.is_none(), trace),
        (RefMode::Attribute, None) => RuleEvaluation::no_match(trace),
        (RefMode::Attribute, Some(attributes)) => {
            let Some(condition) = &rule.attribute_condition else {
                let trap = Trap::error("invalid_rule: attribute mode needs attribute_condition");
                return RuleEvaluation::trapped(&trap, false, trace);
            };
            let ref_scope = Scope {
                refs: Some(&attributes),
                ..*scope
            };
            let result = eval_condition(condition, &ref_scope);
            trace["attribute_condition"] = result.trace;
            match result.tri {
                Tri::True => RuleEvaluation::matched(1.0, trace),
                Tri::False => RuleEvaluation::no_match(trace),
                Tri::Unknown => {
                    let trap = result.trap.unwrap_or_else(|| Trap::error("trapped"));
                    RuleEvaluation::trapped(&trap, false, trace)
                }
            }
        }
    }
}

async fn eval_graph(rule: &GraphRule, scope: &Scope<'_>, provider: &dyn DataProvider) -> RuleEvaluation {
    let mut trace =
        json!({ "metric": rule.metric.as_str(), "max_depth": rule.max_depth, "op": rule.compare.op });
    let customer_id =
        scope
            .ctx
            .customer_id
            .clone()
            .or_else(|| match scope.ctx.resolve("event.customer_id") {
                Resolved::Value(Json::String(id)) => Some(id.clone()),
                _ => None,
            });
    let Some(customer_id) = customer_id else {
        return RuleEvaluation::trapped(
            &Trap::missing("null_operand: customer_id"),
            scope.missing_as_no_match,
            trace,
        );
    };
    let query = GraphMetricQuery {
        customer_id,
        metric: rule.metric,
        link_kinds: rule.link_kinds.clone(),
        include_similar: rule.include_similar,
        max_depth: rule.max_depth,
    };
    let value = match provider.graph_metric(&query).await {
        Ok(Some(v)) => v,
        Ok(None) => match rule.metric {
            GraphMetric::DistanceToFraud => f64::INFINITY,
            GraphMetric::CommunityFraudRate => {
                return RuleEvaluation::trapped(&Trap::error("no_community"), false, trace);
            }
            _ => return RuleEvaluation::trapped(&Trap::error("graph_metric_unavailable"), false, trace),
        },
        Err(e) => return RuleEvaluation::trapped(&Trap::error(e.to_string()), false, trace),
    };
    trace["value"] = number_to_json(value);
    match eval_compare(value, &rule.compare, scope) {
        Ok((hit, right)) => {
            trace["right"] = right;
            RuleEvaluation::from_bool(hit, trace)
        }
        Err(trap) => RuleEvaluation::trapped(&trap, scope.missing_as_no_match, trace),
    }
}

/// Convenience used by tests and the "test rule" endpoint: `true` when the op is a comparison allowed in
/// `compare` blocks of velocity/graph rules.
pub fn is_compare_op(op: Op) -> bool {
    matches!(
        op,
        Op::Eq | Op::Ne | Op::Gt | Op::Gte | Op::Lt | Op::Lte | Op::Between | Op::In | Op::NotIn
    )
}
