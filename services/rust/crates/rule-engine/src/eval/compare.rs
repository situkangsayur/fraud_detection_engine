//! Operator semantics for `left op right` (rule-dsl §4).
//!
//! Every comparison returns `Ok(bool)` or a [`Trap`]. Null operands trap (except for `is_null`/`is_not_null`);
//! the caller decides whether a *missing* operand becomes `no_match` (`missing_as_no_match`).

use std::cmp::Ordering;

use strsim::{jaro_winkler, normalized_levenshtein};

use super::Trap;
use crate::cache;
use crate::model::{LeafOptions, Op, SimilarityMethod};
use crate::value::Value;

const DEFAULT_SIMILARITY_THRESHOLD: f64 = 0.85;

/// Applies `op` to already-evaluated operands. `left_label`/`right_label` describe the operands in trap reasons.
pub fn compare(
    left: &Value,
    op: Op,
    right: Option<&Value>,
    options: &LeafOptions,
    left_label: &str,
    right_label: &str,
) -> Result<bool, Trap> {
    match op {
        Op::IsNull => return Ok(left.is_null()),
        Op::IsNotNull => return Ok(!left.is_null()),
        _ => {}
    }
    if left.is_null() {
        return Err(Trap::missing(format!("null_operand: {left_label}")));
    }
    let right = match right {
        Some(r) if !r.is_null() => r,
        Some(_) => return Err(Trap::missing(format!("null_operand: {right_label}"))),
        None => return Err(Trap::error(format!("missing_right_operand for '{op}'"))),
    };
    let ci = options.case_insensitive;
    match op {
        Op::Eq => equals(left, right, ci),
        Op::Ne => equals(left, right, ci).map(|b| !b),
        Op::Gt => order(left, right).map(|o| o == Ordering::Greater),
        Op::Gte => order(left, right).map(|o| o != Ordering::Less),
        Op::Lt => order(left, right).map(|o| o == Ordering::Less),
        Op::Lte => order(left, right).map(|o| o != Ordering::Greater),
        Op::Between => {
            let Value::Array(bounds) = right else {
                return Err(Trap::error("type_mismatch: between expects [lo, hi]"));
            };
            let [lo, hi] = bounds.as_slice() else {
                return Err(Trap::error("type_mismatch: between expects exactly two bounds"));
            };
            Ok(order(left, lo)? != Ordering::Less && order(left, hi)? != Ordering::Greater)
        }
        Op::In => membership(left, right, ci),
        Op::NotIn => membership(left, right, ci).map(|b| !b),
        Op::Contains => contains(left, right, ci),
        Op::NotContains => contains(left, right, ci).map(|b| !b),
        Op::StartsWith => {
            let (l, r) = texts(left, right, ci, "starts_with")?;
            Ok(l.starts_with(&r))
        }
        Op::EndsWith => {
            let (l, r) = texts(left, right, ci, "ends_with")?;
            Ok(l.ends_with(&r))
        }
        Op::Regex => {
            let text = left
                .as_text()
                .ok_or_else(|| type_mismatch("regex", left, right))?;
            let Value::String(pattern) = right else {
                return Err(type_mismatch("regex", left, right));
            };
            let pattern = if ci {
                format!("(?i){pattern}")
            } else {
                pattern.clone()
            };
            let re = cache::regex(&pattern).map_err(|e| Trap::error(format!("invalid_regex: {e}")))?;
            Ok(re.is_match(&text))
        }
        Op::Similar => {
            let (l, r) = texts(left, right, ci, "similar")?;
            let threshold = options.threshold.unwrap_or(DEFAULT_SIMILARITY_THRESHOLD);
            let score = match options.method.unwrap_or_default() {
                SimilarityMethod::JaroWinkler => jaro_winkler(&l, &r),
                SimilarityMethod::LevenshteinRatio => normalized_levenshtein(&l, &r),
            };
            Ok(score >= threshold)
        }
        Op::IsNull | Op::IsNotNull => Ok(false), // handled above
    }
}

fn type_mismatch(op: &str, left: &Value, right: &Value) -> Trap {
    Trap::error(format!(
        "type_mismatch: {} {op} {}",
        left.type_name(),
        right.type_name()
    ))
}

fn fold(s: &str, ci: bool) -> String {
    if ci {
        s.to_lowercase()
    } else {
        s.to_string()
    }
}

fn texts(left: &Value, right: &Value, ci: bool, op: &str) -> Result<(String, String), Trap> {
    match (left.as_text(), right.as_text()) {
        (Some(l), Some(r)) => Ok((fold(&l, ci), fold(&r, ci))),
        _ => Err(type_mismatch(op, left, right)),
    }
}

/// Equality: numeric when both sides coerce to numbers, date-time when both parse as RFC 3339, otherwise
/// strings/bools/arrays by value. Incomparable types trap.
fn equals(left: &Value, right: &Value, ci: bool) -> Result<bool, Trap> {
    match (left, right) {
        (Value::Bool(a), Value::Bool(b)) => Ok(a == b),
        (Value::Array(_), Value::Array(_)) | (Value::Object(_), Value::Object(_)) => Ok(left == right),
        _ => {
            if let (Some(a), Some(b)) = (left.as_number(), right.as_number()) {
                return Ok(a == b);
            }
            if let (Value::String(a), Value::String(b)) = (left, right) {
                if let (Some(x), Some(y)) = (left.as_datetime(), right.as_datetime()) {
                    return Ok(x == y);
                }
                return Ok(fold(a, ci) == fold(b, ci));
            }
            Err(type_mismatch("eq", left, right))
        }
    }
}

/// Ordering for `gt/gte/lt/lte/between`: numbers or RFC 3339 date-times.
fn order(left: &Value, right: &Value) -> Result<Ordering, Trap> {
    if let (Some(a), Some(b)) = (left.as_number(), right.as_number()) {
        return a
            .partial_cmp(&b)
            .ok_or_else(|| Trap::error("type_mismatch: NaN comparison"));
    }
    if let (Some(a), Some(b)) = (left.as_datetime(), right.as_datetime()) {
        return Ok(a.cmp(&b));
    }
    Err(type_mismatch("order", left, right))
}

/// `in`: the left scalar equals one element of the right array; an array left matches if any element does.
fn membership(left: &Value, right: &Value, ci: bool) -> Result<bool, Trap> {
    let Value::Array(set) = right else {
        return Err(Trap::error(format!(
            "type_mismatch: in expects an array, got {}",
            right.type_name()
        )));
    };
    let lefts: Vec<&Value> = match left {
        Value::Array(items) => items.iter().collect(),
        other => vec![other],
    };
    for l in lefts {
        for candidate in set {
            // Mismatched element types are simply "not equal" inside a set.
            if equals(l, candidate, ci).unwrap_or(false) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// `contains`: substring for strings, element membership for arrays.
fn contains(left: &Value, right: &Value, ci: bool) -> Result<bool, Trap> {
    match left {
        Value::Array(items) => Ok(items.iter().any(|item| equals(item, right, ci).unwrap_or(false))),
        _ => {
            let (l, r) = texts(left, right, ci, "contains")?;
            Ok(l.contains(&r))
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use serde_json::json;

    fn v(j: serde_json::Value) -> Value {
        Value::from_json(&j)
    }

    fn cmp(l: serde_json::Value, op: Op, r: serde_json::Value) -> Result<bool, Trap> {
        compare(&v(l), op, Some(&v(r)), &LeafOptions::default(), "l", "r")
    }

    fn cmp_opts(l: serde_json::Value, op: Op, r: serde_json::Value, o: LeafOptions) -> Result<bool, Trap> {
        compare(&v(l), op, Some(&v(r)), &o, "l", "r")
    }

    #[test]
    fn equality_and_ordering() {
        assert_eq!(cmp(json!(5), Op::Eq, json!(5.0)), Ok(true));
        assert_eq!(cmp(json!("5"), Op::Eq, json!(5)), Ok(true));
        assert_eq!(cmp(json!("ID"), Op::Eq, json!("id")), Ok(false));
        assert_eq!(cmp(json!("ID"), Op::Ne, json!("SG")), Ok(true));
        assert_eq!(cmp(json!(true), Op::Eq, json!(true)), Ok(true));
        assert!(cmp(json!("abc"), Op::Eq, json!(5)).is_err());
        assert_eq!(cmp(json!(6), Op::Gt, json!(5)), Ok(true));
        assert_eq!(cmp(json!(5), Op::Gte, json!(5)), Ok(true));
        assert_eq!(cmp(json!(4), Op::Lt, json!(5)), Ok(true));
        assert_eq!(cmp(json!(5), Op::Lte, json!(4)), Ok(false));
        assert_eq!(cmp(json!("1500000.00"), Op::Gt, json!(1_000_000)), Ok(true));
        assert!(cmp(json!("abc"), Op::Gt, json!(5)).is_err());
        assert_eq!(
            cmp(
                json!("2026-01-02T00:00:00Z"),
                Op::Gt,
                json!("2026-01-01T23:00:00+07:00")
            ),
            Ok(true)
        );
        assert_eq!(
            cmp(
                json!("2026-01-01T00:00:00Z"),
                Op::Eq,
                json!("2026-01-01T07:00:00+07:00")
            ),
            Ok(true)
        );
    }

    #[test]
    fn case_insensitive_option() {
        let o = LeafOptions {
            case_insensitive: true,
            ..Default::default()
        };
        assert_eq!(cmp_opts(json!("ID"), Op::Eq, json!("id"), o.clone()), Ok(true));
        assert_eq!(
            cmp_opts(json!("Hello World"), Op::Contains, json!("WORLD"), o.clone()),
            Ok(true)
        );
        assert_eq!(
            cmp_opts(json!("ABC"), Op::Regex, json!("^abc$"), o.clone()),
            Ok(true)
        );
        assert_eq!(cmp_opts(json!("ab"), Op::In, json!(["AB", "cd"]), o), Ok(true));
    }

    #[test]
    fn between_in_contains() {
        assert_eq!(cmp(json!(5), Op::Between, json!([1, 5])), Ok(true));
        assert_eq!(cmp(json!(6), Op::Between, json!([1, 5])), Ok(false));
        assert!(cmp(json!(6), Op::Between, json!([1])).is_err());
        assert!(cmp(json!(6), Op::Between, json!(1)).is_err());
        assert_eq!(cmp(json!("ID"), Op::In, json!(["SG", "ID"])), Ok(true));
        assert_eq!(cmp(json!(3), Op::In, json!(["3", 4])), Ok(true));
        assert_eq!(cmp(json!("MY"), Op::NotIn, json!(["SG", "ID"])), Ok(true));
        assert_eq!(cmp(json!(["a", "b"]), Op::In, json!(["b"])), Ok(true));
        assert!(cmp(json!("a"), Op::In, json!("a")).is_err());
        assert_eq!(cmp(json!("voucher-50"), Op::Contains, json!("50")), Ok(true));
        assert_eq!(cmp(json!(["x", "y"]), Op::Contains, json!("y")), Ok(true));
        assert_eq!(cmp(json!("abc"), Op::NotContains, json!("z")), Ok(true));
        assert_eq!(cmp(json!("411111"), Op::StartsWith, json!("4111")), Ok(true));
        assert_eq!(cmp(json!(411_111), Op::StartsWith, json!("4111")), Ok(true));
        assert_eq!(cmp(json!("gmail.com"), Op::EndsWith, json!(".com")), Ok(true));
    }

    #[test]
    fn regex_and_similar() {
        assert_eq!(cmp(json!("+6281234"), Op::Regex, json!(r"^\+62")), Ok(true));
        assert!(cmp(json!("x"), Op::Regex, json!("("))
            .unwrap_err()
            .reason
            .starts_with("invalid_regex"));
        assert_eq!(
            cmp(
                json!("Jl. Sudirman No 5"),
                Op::Similar,
                json!("Jl Sudirman No. 5")
            ),
            Ok(true)
        );
        assert_eq!(cmp(json!("Jakarta"), Op::Similar, json!("Surabaya")), Ok(false));
        let lev = LeafOptions {
            method: Some(SimilarityMethod::LevenshteinRatio),
            threshold: Some(0.8),
            ..Default::default()
        };
        assert_eq!(
            cmp_opts(json!("081234567890"), Op::Similar, json!("081234567891"), lev),
            Ok(true)
        );
    }

    #[test]
    fn nulls() {
        let o = LeafOptions::default();
        assert_eq!(compare(&Value::Null, Op::IsNull, None, &o, "l", "r"), Ok(true));
        assert_eq!(
            compare(&Value::Number(1.0), Op::IsNotNull, None, &o, "l", "r"),
            Ok(true)
        );
        let t = compare(
            &Value::Null,
            Op::Gt,
            Some(&Value::Number(1.0)),
            &o,
            "event.amount",
            "r",
        )
        .unwrap_err();
        assert!(t.missing);
        assert_eq!(t.reason, "null_operand: event.amount");
        let t = compare(
            &Value::Number(1.0),
            Op::Gt,
            Some(&Value::Null),
            &o,
            "l",
            "event.limit",
        )
        .unwrap_err();
        assert!(t.missing);
        assert!(compare(&Value::Number(1.0), Op::Gt, None, &o, "l", "r").is_err());
    }
}
