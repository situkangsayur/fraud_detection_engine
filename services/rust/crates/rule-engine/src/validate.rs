//! Save-time validation of rules and rulesets (rule-dsl §8).
//!
//! Two layers:
//! 1. **Shape** — serde deserialisation into the typed model with [`serde_path_to_error`], so a malformed field
//!    is reported with its JSON path (`definition.when.all[1].op`).
//! 2. **Semantics** — a walk over the typed model: field paths against the project [`FieldCatalog`], formula
//!    parsing and binding, operand positions (`hist`/`ref`), operator/operand compatibility, velocity fields,
//!    statistics prerequisites, score ranges.
//!
//! `rule-service` calls this on every create/update and the LLM service calls it (through the API) before any
//! proposal is stored.

use std::collections::{BTreeSet, HashSet};

use serde::Serialize;

use crate::cache;
use crate::context::{history_field_context_path, is_valid_path};
use crate::eval::is_compare_op;
use crate::model::{
    AggFn, Compare, CompositeRule, Condition, GraphRule, Leaf, Op, Operand, RefMode, ReferenceRule,
    RuleDefinition, RuleEnvelope, RulesetSpec, SimpleRule, Statistic, VelocitySpec, Window, LINK_KINDS,
    TYPOLOGIES,
};

/// Project field catalogue as seen by the validator. rule-service implements it from built-ins plus
/// `core.field_catalog`.
pub trait FieldCatalog {
    /// Whether a context path (`event.amount`, `features.cust_cnt_1h`, `ml.fraud_probability`) exists.
    /// `source.*` and `customer.attributes.*` paths are accepted by the validator without asking.
    fn is_known_path(&self, path: &str) -> bool;

    /// Whether a *history* field (`amount`, `customer_id`, `source.order.total`) can be used in velocity
    /// `group_by` / `aggregate.field` and composite `hist` operands.
    fn is_velocity_field(&self, field: &str) -> bool;
}

/// A simple in-memory catalogue (tests, playgrounds, bootstrap).
#[derive(Debug, Clone, Default)]
pub struct StaticCatalog {
    pub paths: HashSet<String>,
    pub velocity_fields: HashSet<String>,
    /// Accept every syntactically valid path/field.
    pub permissive: bool,
}

impl StaticCatalog {
    /// Accepts everything (useful for the formula playground and unit tests).
    pub fn permissive() -> Self {
        Self {
            permissive: true,
            ..Default::default()
        }
    }

    pub fn with_paths<I: IntoIterator<Item = S>, S: Into<String>>(mut self, paths: I) -> Self {
        self.paths.extend(paths.into_iter().map(Into::into));
        self
    }

    pub fn with_velocity_fields<I: IntoIterator<Item = S>, S: Into<String>>(mut self, fields: I) -> Self {
        self.velocity_fields.extend(fields.into_iter().map(Into::into));
        self
    }
}

impl FieldCatalog for StaticCatalog {
    fn is_known_path(&self, path: &str) -> bool {
        self.permissive || self.paths.contains(path)
    }

    fn is_velocity_field(&self, field: &str) -> bool {
        self.permissive || self.velocity_fields.contains(field)
    }
}

/// One validation problem, located by a JSON path in the submitted document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidationError {
    pub path: String,
    pub message: String,
}

/// Validation result (response body of `POST /rules/validate`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct ValidationReport {
    pub valid: bool,
    pub errors: Vec<ValidationError>,
    pub referenced_fields: Vec<String>,
    pub referenced_lists: Vec<String>,
}

/// Validates a rule envelope given as JSON. Returns the report and, when the shape is valid, the parsed rule
/// (which may still have semantic errors — check `report.valid`).
pub fn validate_rule_json(
    json: &serde_json::Value,
    catalog: &dyn FieldCatalog,
) -> (ValidationReport, Option<RuleEnvelope>) {
    match serde_path_to_error::deserialize::<_, RuleEnvelope>(json) {
        Ok(rule) => (validate_rule(&rule, catalog), Some(rule)),
        Err(err) => {
            let path = err.path().to_string();
            let report = ValidationReport {
                valid: false,
                errors: vec![ValidationError {
                    path: if path == "." { String::new() } else { path },
                    message: err.inner().to_string(),
                }],
                ..Default::default()
            };
            (report, None)
        }
    }
}

/// Validates an already-parsed rule.
pub fn validate_rule(rule: &RuleEnvelope, catalog: &dyn FieldCatalog) -> ValidationReport {
    let mut v = Validator {
        catalog,
        errors: Vec::new(),
        fields: BTreeSet::new(),
        lists: BTreeSet::new(),
    };
    v.envelope(rule);
    ValidationReport {
        valid: v.errors.is_empty(),
        errors: v.errors,
        referenced_fields: v.fields.into_iter().collect(),
        referenced_lists: v.lists.into_iter().collect(),
    }
}

/// Validates a ruleset definition (membership is validated by rule-service against stored rules).
pub fn validate_ruleset(spec: &RulesetSpec) -> ValidationReport {
    let mut errors = Vec::new();
    let mut err = |path: String, message: String| errors.push(ValidationError { path, message });
    if !valid_code(&spec.code) {
        err("code".into(), "code must match ^[A-Z0-9-]{3,40}$".into());
    }
    if spec.name.trim().is_empty() {
        err("name".into(), "name must not be empty".into());
    }
    if !(0.0..=100.0).contains(&spec.max_score) {
        err("max_score".into(), "max_score must be within 0..100".into());
    }
    for (i, t) in spec.typologies.iter().enumerate() {
        if !TYPOLOGIES.contains(&t.as_str()) {
            err(format!("typologies[{i}]"), format!("unknown typology '{t}'"));
        }
    }
    let mut seen = HashSet::new();
    for (i, member) in spec.rules.iter().enumerate() {
        if !(0.0..=10.0).contains(&member.weight) {
            err(format!("rules[{i}].weight"), "weight must be within 0..10".into());
        }
        if !seen.insert(member.rule_id.as_str()) {
            err(
                format!("rules[{i}].rule_id"),
                format!("rule '{}' is listed twice", member.rule_id),
            );
        }
    }
    ValidationReport {
        valid: errors.is_empty(),
        errors,
        ..Default::default()
    }
}

fn valid_code(code: &str) -> bool {
    (3..=40).contains(&code.len())
        && code
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '-')
}

fn valid_list_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    (2..=63).contains(&bytes.len())
        && bytes
            .first()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && bytes
            .iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_')
}

const CONTEXT_ROOTS: [&str; 6] = ["event", "source", "customer", "features", "ml", "graph"];

/// Where an operand appears, which decides whether `ref` / `hist` are allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Position {
    Normal,
    /// Inside `reference.attribute_condition`: `ref` allowed.
    Attribute,
    /// Left side of a `composite.history_filter` leaf: must be `hist`.
    HistLeft,
}

struct Validator<'a> {
    catalog: &'a dyn FieldCatalog,
    errors: Vec<ValidationError>,
    fields: BTreeSet<String>,
    lists: BTreeSet<String>,
}

impl Validator<'_> {
    fn err(&mut self, path: impl Into<String>, message: impl Into<String>) {
        self.errors.push(ValidationError {
            path: path.into(),
            message: message.into(),
        });
    }

    fn envelope(&mut self, rule: &RuleEnvelope) {
        if !valid_code(&rule.code) {
            self.err("code", "code must match ^[A-Z0-9-]{3,40}$");
        }
        if rule.name.trim().is_empty() {
            self.err("name", "name must not be empty");
        }
        if rule.kind != rule.definition.kind() {
            self.err(
                "kind",
                format!(
                    "kind '{}' does not match definition.kind '{}'",
                    rule.kind,
                    rule.definition.kind()
                ),
            );
        }
        if !(0.0..=100.0).contains(&rule.risk_score) {
            self.err("risk_score", "risk_score must be within 0..100");
        }
        if !(0.0..=100.0).contains(&rule.trapped_score) {
            self.err("trapped_score", "trapped_score must be within 0..100");
        }
        for (i, t) in rule.typologies.iter().enumerate() {
            if !TYPOLOGIES.contains(&t.as_str()) {
                self.err(
                    format!("typologies[{i}]"),
                    format!("unknown typology '{t}' (allowed: {})", TYPOLOGIES.join(", ")),
                );
            }
        }
        for (i, t) in rule.event_types.iter().enumerate() {
            if t.trim().is_empty() {
                self.err(format!("event_types[{i}]"), "event type must not be empty");
            }
        }
        match &rule.definition {
            RuleDefinition::Simple(s) => self.simple(s),
            RuleDefinition::Velocity(v) => self.velocity(v, "definition"),
            RuleDefinition::Composite(c) => self.composite(c),
            RuleDefinition::Reference(r) => self.reference(r),
            RuleDefinition::Graph(g) => self.graph(g),
        }
    }

    fn simple(&mut self, rule: &SimpleRule) {
        self.condition(&rule.when, "definition.when", Position::Normal);
    }

    // ----- operands -------------------------------------------------------------------------------------------

    fn field_path(&mut self, path: &str, at: &str) {
        if !is_valid_path(path) {
            self.err(at, format!("invalid path syntax '{path}'"));
            return;
        }
        let root = path.split(['.', '[']).next().unwrap_or_default();
        if !CONTEXT_ROOTS.contains(&root) {
            self.err(
                at,
                format!(
                    "path '{path}' must start with one of: {}",
                    CONTEXT_ROOTS.join(", ")
                ),
            );
            return;
        }
        let dynamic = path.starts_with("source.") || path.starts_with("customer.attributes.");
        if !dynamic && !self.catalog.is_known_path(path) {
            self.err(
                at,
                format!("unknown field '{path}' (not in the project field catalog)"),
            );
        }
        self.fields.insert(path.to_string());
    }

    fn history_field(&mut self, field: &str, at: &str) {
        if !is_valid_path(field) {
            self.err(at, format!("invalid field syntax '{field}'"));
            return;
        }
        if !self.catalog.is_velocity_field(field) {
            self.err(
                at,
                format!("field '{field}' is not velocity-enabled in the project field catalog"),
            );
        }
        self.fields.insert(history_field_context_path(field));
    }

    fn operand(&mut self, operand: &Operand, at: &str, position: Position) {
        match operand {
            Operand::Const { .. } => {}
            Operand::Field { path } => self.field_path(path, &format!("{at}.path")),
            Operand::Ref { path } => {
                if position != Position::Attribute {
                    self.err(
                        at,
                        "`ref` operands are only allowed inside reference.attribute_condition",
                    );
                } else if !is_valid_path(path) {
                    self.err(format!("{at}.path"), format!("invalid path syntax '{path}'"));
                }
            }
            Operand::Hist { path } => {
                if position != Position::HistLeft {
                    self.err(
                        at,
                        "`hist` operands are only allowed on the left side of composite.history_filter",
                    );
                } else {
                    self.history_field(path, &format!("{at}.path"));
                }
            }
            Operand::Formula { expr, args } => {
                match cache::formula(expr) {
                    Ok(formula) => {
                        let names = args.keys().cloned().collect();
                        if let Err(e) = formula.check_bindings(&names) {
                            self.err(
                                format!("{at}.expr"),
                                format!("{} (position {})", e.message, e.position),
                            );
                        }
                    }
                    Err(e) => self.err(
                        format!("{at}.expr"),
                        format!("{} (position {})", e.message, e.position),
                    ),
                }
                let inner = if position == Position::Attribute {
                    Position::Attribute
                } else {
                    Position::Normal
                };
                for (name, arg) in args {
                    self.operand(arg, &format!("{at}.args.{name}"), inner);
                }
            }
        }
    }

    // ----- conditions -----------------------------------------------------------------------------------------

    fn condition(&mut self, condition: &Condition, at: &str, position: Position) {
        match condition {
            Condition::Leaf(leaf) => self.leaf(leaf, at, position),
            Condition::All(items) | Condition::Any(items) => {
                let key = if matches!(condition, Condition::All(_)) {
                    "all"
                } else {
                    "any"
                };
                if items.is_empty() {
                    self.err(
                        format!("{at}.{key}"),
                        format!("`{key}` must contain at least one condition"),
                    );
                }
                for (i, c) in items.iter().enumerate() {
                    self.condition(c, &format!("{at}.{key}[{i}]"), position);
                }
            }
            Condition::Not(inner) => self.condition(inner, &format!("{at}.not"), position),
            Condition::AtLeast(a) => {
                if position == Position::HistLeft {
                    self.err(
                        format!("{at}.at_least"),
                        "`at_least` is not supported in history_filter",
                    );
                }
                if a.n == 0 || a.n > a.of.len() {
                    self.err(
                        format!("{at}.at_least.n"),
                        format!("n must be within 1..{}", a.of.len()),
                    );
                }
                for (i, c) in a.of.iter().enumerate() {
                    self.condition(c, &format!("{at}.at_least.of[{i}]"), position);
                }
            }
        }
    }

    fn leaf(&mut self, leaf: &Leaf, at: &str, position: Position) {
        let history = position == Position::HistLeft;
        if history {
            if !matches!(leaf.left, Operand::Hist { .. }) {
                self.err(
                    format!("{at}.left"),
                    "the left side of a history_filter condition must be a `hist` operand",
                );
            } else {
                self.operand(&leaf.left, &format!("{at}.left"), Position::HistLeft);
            }
            if !leaf.op.allowed_in_history_filter() {
                self.err(
                    format!("{at}.op"),
                    format!("operator '{}' is not allowed in history_filter", leaf.op),
                );
            }
        } else {
            self.operand(&leaf.left, &format!("{at}.left"), position);
        }
        let right_position = if history { Position::Normal } else { position };
        match (&leaf.right, leaf.op.is_unary()) {
            (Some(_), true) => self.err(
                format!("{at}.right"),
                format!("'{}' takes no right operand", leaf.op),
            ),
            (None, false) => self.err(
                format!("{at}.right"),
                format!("'{}' needs a right operand", leaf.op),
            ),
            (Some(right), false) => {
                if history && matches!(right, Operand::Hist { .. } | Operand::Ref { .. }) {
                    self.err(
                        format!("{at}.right"),
                        "history_filter right side must be const, field or formula",
                    );
                } else {
                    self.operand(right, &format!("{at}.right"), right_position);
                }
                self.const_shape(leaf.op, right, &format!("{at}.right"));
            }
            (None, true) => {}
        }
        if let Some(w) = leaf.weight {
            if !(w.is_finite() && w >= 0.0) {
                self.err(format!("{at}.weight"), "weight must be a finite number >= 0");
            }
        }
        if let Some(t) = leaf.options.threshold {
            if !(0.0..=1.0).contains(&t) {
                self.err(format!("{at}.options.threshold"), "threshold must be within 0..1");
            }
        }
    }

    /// Checks constant right operands against operator expectations.
    fn const_shape(&mut self, op: Op, right: &Operand, at: &str) {
        let Operand::Const { value } = right else { return };
        match op {
            Op::Between => {
                let ok = value.as_array().is_some_and(|a| a.len() == 2);
                if !ok {
                    self.err(format!("{at}.value"), "between expects an array [lo, hi]");
                }
            }
            Op::In | Op::NotIn => {
                if !value.is_array() {
                    self.err(format!("{at}.value"), format!("'{op}' expects an array"));
                }
            }
            Op::Regex => match value.as_str() {
                Some(pattern) => {
                    if let Err(e) = cache::regex(pattern) {
                        self.err(format!("{at}.value"), format!("invalid regex: {e}"));
                    }
                }
                None => self.err(format!("{at}.value"), "regex expects a string pattern"),
            },
            Op::StartsWith | Op::EndsWith | Op::Similar | Op::NotContains
                if !(value.is_string() || value.is_number()) =>
            {
                self.err(format!("{at}.value"), format!("'{op}' expects a string"));
            }
            _ => {}
        }
    }

    // ----- kinds ----------------------------------------------------------------------------------------------

    fn compare(&mut self, compare: &Compare, at: &str) {
        if !is_compare_op(compare.op) {
            self.err(
                format!("{at}.op"),
                format!(
                    "'{}' is not allowed in compare (use eq, ne, gt, gte, lt, lte, between, in, not_in)",
                    compare.op
                ),
            );
        }
        self.operand(&compare.right, &format!("{at}.right"), Position::Normal);
        self.const_shape(compare.op, &compare.right, &format!("{at}.right"));
    }

    fn velocity(&mut self, spec: &VelocitySpec, at: &str) {
        if spec.group_by.is_empty() {
            self.err(
                format!("{at}.group_by"),
                "group_by must contain at least one field",
            );
        }
        for (i, field) in spec.group_by.iter().enumerate() {
            self.history_field(field, &format!("{at}.group_by[{i}]"));
        }
        for (i, t) in spec.history_event_types.iter().enumerate() {
            if t.trim().is_empty() {
                self.err(
                    format!("{at}.history_event_types[{i}]"),
                    "event type must not be empty",
                );
            }
        }
        let window_seconds = match &spec.window {
            Window::Duration { duration } => Some(duration.seconds()),
            Window::LastN { last_n } => {
                if !(1..=10_000).contains(last_n) {
                    self.err(format!("{at}.window.last_n"), "last_n must be within 1..10000");
                }
                None
            }
        };
        let agg = &spec.aggregate;
        match (&agg.field, agg.func) {
            (None, AggFn::Count) => {}
            (None, f) => self.err(
                format!("{at}.aggregate.field"),
                format!("aggregate '{}' needs a field", f.as_str()),
            ),
            (Some(field), _) => self.history_field(field, &format!("{at}.aggregate.field")),
        }
        match (agg.func, agg.p) {
            (AggFn::Percentile, Some(p)) if p > 0.0 && p < 1.0 => {}
            (AggFn::Percentile, _) => {
                self.err(format!("{at}.aggregate.p"), "percentile needs p within (0, 1)")
            }
            (_, Some(_)) => self.err(
                format!("{at}.aggregate.p"),
                "p is only allowed with fn = percentile",
            ),
            _ => {}
        }
        if let Some(stat) = &spec.statistic {
            let sat = format!("{at}.statistic");
            match stat {
                Statistic::Zscore { of }
                | Statistic::GaussianTail { of, .. }
                | Statistic::PercentileRank { of } => {
                    if agg.field.is_none() {
                        self.err(
                            format!("{at}.aggregate.field"),
                            format!(
                                "statistic '{}' needs aggregate.field (the history values)",
                                stat.name()
                            ),
                        );
                    }
                    if let Some(of) = of {
                        self.operand(of, &format!("{sat}.of"), Position::Normal);
                    }
                }
                Statistic::LinearTrend { bucket, .. } | Statistic::PoissonTail { bucket } => {
                    match window_seconds {
                        None => self.err(
                            format!("{at}.window"),
                            format!("statistic '{}' needs a duration window", stat.name()),
                        ),
                        Some(w) => {
                            if bucket.seconds() >= w {
                                self.err(format!("{sat}.bucket"), "bucket must be shorter than the window");
                            } else if w / bucket.seconds() > 10_000 {
                                self.err(
                                    format!("{sat}.bucket"),
                                    "window / bucket must not exceed 10000 buckets",
                                );
                            }
                        }
                    }
                }
            }
        }
        if let Some(m) = spec.min_samples {
            if m > 100_000 {
                self.err(format!("{at}.min_samples"), "min_samples must be <= 100000");
            }
        }
        self.compare(&spec.compare, &format!("{at}.compare"));
    }

    fn composite(&mut self, rule: &CompositeRule) {
        if let Some(gate) = &rule.gate {
            self.condition(gate, "definition.gate", Position::Normal);
        }
        self.condition(
            &rule.history_filter,
            "definition.history_filter",
            Position::HistLeft,
        );
        self.velocity(&rule.velocity, "definition.velocity");
    }

    fn reference(&mut self, rule: &ReferenceRule) {
        if !valid_list_name(&rule.list) {
            self.err(
                "definition.list",
                "list name must match ^[a-z0-9][a-z0-9_]{1,62}$",
            );
        }
        self.lists.insert(rule.list.clone());
        self.operand(&rule.key, "definition.key", Position::Normal);
        match (rule.mode, &rule.attribute_condition) {
            (RefMode::Attribute, Some(c)) => {
                self.condition(c, "definition.attribute_condition", Position::Attribute)
            }
            (RefMode::Attribute, None) => self.err(
                "definition.attribute_condition",
                "mode 'attribute' needs attribute_condition",
            ),
            (_, Some(_)) => self.err(
                "definition.attribute_condition",
                "attribute_condition is only allowed with mode 'attribute'",
            ),
            (_, None) => {}
        }
    }

    fn graph(&mut self, rule: &GraphRule) {
        if !(1..=3).contains(&rule.max_depth) {
            self.err("definition.max_depth", "max_depth must be within 1..3");
        }
        for (i, kind) in rule.link_kinds.iter().enumerate() {
            if !LINK_KINDS.contains(&kind.as_str()) {
                self.err(
                    format!("definition.link_kinds[{i}]"),
                    format!("unknown link kind '{kind}' (allowed: {})", LINK_KINDS.join(", ")),
                );
            }
        }
        self.compare(&rule.compare, "definition.compare");
    }
}
