//! Condition trees with Kleene three-valued logic (rule-dsl §4) and compilation of composite history filters
//! into provider predicates (rule-dsl §6.3).

use serde_json::{json, Value as Json};

use super::{compare::compare, eval_operand, operand_label, Scope, Trap};
use crate::model::{Condition, Leaf, Operand};
use crate::ports::{HistFilter, HistPredicate};
use crate::value::Value;

/// Three-valued truth: true, false, unknown (= trapped).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tri {
    True,
    False,
    Unknown,
}

impl Tri {
    pub fn as_str(self) -> &'static str {
        match self {
            Tri::True => "match",
            Tri::False => "no_match",
            Tri::Unknown => "trapped",
        }
    }

    /// Kleene AND over a sequence.
    pub fn all(values: impl IntoIterator<Item = Tri>) -> Tri {
        let mut unknown = false;
        for v in values {
            match v {
                Tri::False => return Tri::False,
                Tri::Unknown => unknown = true,
                Tri::True => {}
            }
        }
        if unknown {
            Tri::Unknown
        } else {
            Tri::True
        }
    }

    /// Kleene OR over a sequence.
    pub fn any(values: impl IntoIterator<Item = Tri>) -> Tri {
        let mut unknown = false;
        for v in values {
            match v {
                Tri::True => return Tri::True,
                Tri::Unknown => unknown = true,
                Tri::False => {}
            }
        }
        if unknown {
            Tri::Unknown
        } else {
            Tri::False
        }
    }

    /// At least `n` true: decided true, decided false, or unknown while trapped members could tip it.
    pub fn at_least(n: usize, values: impl IntoIterator<Item = Tri>) -> Tri {
        let (mut t, mut u) = (0usize, 0usize);
        for v in values {
            match v {
                Tri::True => t += 1,
                Tri::Unknown => u += 1,
                Tri::False => {}
            }
        }
        if t >= n {
            Tri::True
        } else if t + u < n {
            Tri::False
        } else {
            Tri::Unknown
        }
    }
}

/// Kleene NOT: swaps true/false, unknown stays unknown.
impl std::ops::Not for Tri {
    type Output = Tri;

    fn not(self) -> Tri {
        match self {
            Tri::True => Tri::False,
            Tri::False => Tri::True,
            Tri::Unknown => Tri::Unknown,
        }
    }
}

/// Result of evaluating a condition tree.
#[derive(Debug, Clone)]
pub struct CondResult {
    pub tri: Tri,
    /// Σ weights of leaves that matched (after `not` inversion) — for `scoring: weighted`.
    pub matched_weight: f64,
    /// Σ weights of trapped leaves.
    pub trapped_weight: f64,
    /// Σ weights of all leaves.
    pub total_weight: f64,
    /// First trap encountered, when the result is unknown.
    pub trap: Option<Trap>,
    pub trace: Json,
}

/// Evaluates a condition tree in a scope. Every leaf is evaluated (no short-circuit) so traces and weighted
/// scoring see the whole tree; conditions never perform IO, so this is cheap.
pub fn eval_condition(condition: &Condition, scope: &Scope<'_>) -> CondResult {
    match condition {
        Condition::Leaf(leaf) => eval_leaf(leaf, scope),
        Condition::All(items) => group(items, scope, "all", Tri::all),
        Condition::Any(items) => group(items, scope, "any", Tri::any),
        Condition::AtLeast(at_least) => {
            let n = at_least.n;
            let mut result = group(&at_least.of, scope, "of", |v| Tri::at_least(n, v));
            result.trace = json!({ "at_least": { "n": n, "of": result.trace["of"].clone() }, "outcome": result.tri.as_str() });
            result
        }
        Condition::Not(inner) => {
            let r = eval_condition(inner, scope);
            let tri = !r.tri;
            CondResult {
                tri,
                matched_weight: (r.total_weight - r.matched_weight - r.trapped_weight).max(0.0),
                trapped_weight: r.trapped_weight,
                total_weight: r.total_weight,
                trap: if tri == Tri::Unknown { r.trap } else { None },
                trace: json!({ "not": r.trace, "outcome": tri.as_str() }),
            }
        }
    }
}

fn group(items: &[Condition], scope: &Scope<'_>, key: &str, combine: impl Fn(Vec<Tri>) -> Tri) -> CondResult {
    let results: Vec<CondResult> = items.iter().map(|c| eval_condition(c, scope)).collect();
    let tri = combine(results.iter().map(|r| r.tri).collect());
    let trap = if tri == Tri::Unknown {
        results.iter().find_map(|r| r.trap.clone())
    } else {
        None
    };
    CondResult {
        tri,
        matched_weight: results.iter().map(|r| r.matched_weight).sum(),
        trapped_weight: results.iter().map(|r| r.trapped_weight).sum(),
        total_weight: results.iter().map(|r| r.total_weight).sum(),
        trap,
        trace: json!({ key: results.into_iter().map(|r| r.trace).collect::<Vec<_>>(), "outcome": tri.as_str() }),
    }
}

fn eval_leaf(leaf: &Leaf, scope: &Scope<'_>) -> CondResult {
    let weight = leaf.weight();
    let left_label = operand_label(&leaf.left);
    let right_label = leaf.right.as_ref().map(operand_label).unwrap_or_default();
    let mut trace = json!({ "left": left_label, "op": leaf.op });
    if !right_label.is_empty() {
        trace["right"] = Json::String(right_label.clone());
    }
    let result: Result<bool, Trap> = (|| {
        let left = eval_operand(&leaf.left, scope)?;
        trace["left_value"] = left.to_json();
        let right = match &leaf.right {
            Some(r) => {
                let v = eval_operand(r, scope)?;
                trace["right_value"] = v.to_json();
                Some(v)
            }
            None => None,
        };
        compare(
            &left,
            leaf.op,
            right.as_ref(),
            &leaf.options,
            &left_label,
            &right_label,
        )
    })();
    let (tri, trap) = match result {
        Ok(true) => (Tri::True, None),
        Ok(false) => (Tri::False, None),
        Err(trap) if trap.missing && scope.missing_as_no_match => {
            trace["missing"] = Json::String(trap.reason.clone());
            (Tri::False, None)
        }
        Err(trap) => {
            trace["trapped_reason"] = Json::String(trap.reason.clone());
            (Tri::Unknown, Some(trap))
        }
    };
    trace["outcome"] = Json::String(tri.as_str().to_string());
    CondResult {
        tri,
        matched_weight: if tri == Tri::True { weight } else { 0.0 },
        trapped_weight: if tri == Tri::Unknown { weight } else { 0.0 },
        total_weight: weight,
        trap,
        trace,
    }
}

/// Compiles `composite.history_filter` into provider predicates. Left sides must be `hist` operands; right sides
/// are evaluated against the current event into constants.
pub fn compile_history_filter(condition: &Condition, scope: &Scope<'_>) -> Result<HistFilter, Trap> {
    match condition {
        Condition::All(items) => Ok(HistFilter::And(
            items
                .iter()
                .map(|c| compile_history_filter(c, scope))
                .collect::<Result<_, _>>()?,
        )),
        Condition::Any(items) => Ok(HistFilter::Or(
            items
                .iter()
                .map(|c| compile_history_filter(c, scope))
                .collect::<Result<_, _>>()?,
        )),
        Condition::Not(inner) => Ok(HistFilter::Not(Box::new(compile_history_filter(inner, scope)?))),
        Condition::AtLeast(_) => Err(Trap::error("invalid_history_filter: at_least is not supported")),
        Condition::Leaf(leaf) => compile_leaf(leaf, scope).map(HistFilter::Pred),
    }
}

fn compile_leaf(leaf: &Leaf, scope: &Scope<'_>) -> Result<HistPredicate, Trap> {
    let Operand::Hist { path } = &leaf.left else {
        return Err(Trap::error(format!(
            "invalid_history_filter: left side must be a hist operand, got {}",
            leaf.left.type_name()
        )));
    };
    if !leaf.op.allowed_in_history_filter() {
        return Err(Trap::error(format!(
            "invalid_history_filter: operator '{}' is not allowed",
            leaf.op
        )));
    }
    let value = if leaf.op.is_unary() {
        Json::Null
    } else {
        let right = leaf.right.as_ref().ok_or_else(|| {
            Trap::error(format!(
                "invalid_history_filter: '{}' needs a right operand",
                leaf.op
            ))
        })?;
        if matches!(right, Operand::Hist { .. } | Operand::Ref { .. }) {
            return Err(Trap::error(
                "invalid_history_filter: right side must be const, field or formula",
            ));
        }
        let value = eval_operand(right, scope)?;
        if value.is_null() {
            return Err(Trap::missing(format!("null_operand: {}", operand_label(right))));
        }
        let needs_array = matches!(
            leaf.op,
            crate::model::Op::In | crate::model::Op::NotIn | crate::model::Op::Between
        );
        if needs_array != matches!(value, Value::Array(_)) {
            return Err(Trap::error(format!(
                "type_mismatch: '{}' in history_filter got {}",
                leaf.op,
                value.type_name()
            )));
        }
        value.to_json()
    };
    Ok(HistPredicate {
        field: path.clone(),
        op: leaf.op,
        value,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const ALL: [Tri; 3] = [Tri::True, Tri::False, Tri::Unknown];

    #[test]
    fn kleene_truth_tables() {
        use Tri::*;
        // AND
        assert_eq!(Tri::all([True, True]), True);
        assert_eq!(Tri::all([True, Unknown]), Unknown);
        assert_eq!(Tri::all([False, Unknown]), False);
        assert_eq!(Tri::all([]), True);
        // OR
        assert_eq!(Tri::any([False, False]), False);
        assert_eq!(Tri::any([False, Unknown]), Unknown);
        assert_eq!(Tri::any([True, Unknown]), True);
        assert_eq!(Tri::any([]), False);
        // NOT
        assert_eq!(!True, False);
        assert_eq!(!Unknown, Unknown);
        // AT_LEAST
        assert_eq!(Tri::at_least(2, [True, True, False]), True);
        assert_eq!(Tri::at_least(2, [True, False, False]), False);
        assert_eq!(Tri::at_least(2, [True, Unknown, False]), Unknown);
        assert_eq!(Tri::at_least(0, []), True);
    }

    fn tri() -> impl Strategy<Value = Tri> {
        prop::sample::select(ALL.to_vec())
    }

    proptest! {
        #[test]
        fn de_morgan(a in tri(), b in tri()) {
            prop_assert_eq!(!Tri::all([a, b]), Tri::any([!a, !b]));
            prop_assert_eq!(!Tri::any([a, b]), Tri::all([!a, !b]));
        }

        #[test]
        fn at_least_generalises_and_or(values in prop::collection::vec(tri(), 1..6)) {
            prop_assert_eq!(Tri::at_least(1, values.clone()), Tri::any(values.clone()));
            prop_assert_eq!(Tri::at_least(values.len(), values.clone()), Tri::all(values));
        }

        #[test]
        fn double_negation(a in tri()) {
            prop_assert_eq!(!!a, a);
        }
    }
}
