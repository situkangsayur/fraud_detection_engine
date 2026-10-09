//! Dynamic values produced by operands, with the DSL's coercion rules.
//!
//! Context data comes from heterogeneous sources (canonical columns, raw source payloads, ML outputs), so
//! numbers may arrive as JSON strings (e.g. decimals serialised as `"150000.00"`) and timestamps as RFC 3339
//! strings. Coercion is centralised here so every operator behaves the same way.

use chrono::{DateTime, Utc};
use serde_json::Number;

/// A dynamically typed value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    Object(serde_json::Map<String, serde_json::Value>),
}

impl Value {
    /// Converts from JSON.
    pub fn from_json(json: &serde_json::Value) -> Value {
        match json {
            serde_json::Value::Null => Value::Null,
            serde_json::Value::Bool(b) => Value::Bool(*b),
            serde_json::Value::Number(n) => n.as_f64().map_or(Value::Null, Value::Number),
            serde_json::Value::String(s) => Value::String(s.clone()),
            serde_json::Value::Array(items) => Value::Array(items.iter().map(Value::from_json).collect()),
            serde_json::Value::Object(map) => Value::Object(map.clone()),
        }
    }

    /// Converts to JSON. Non-finite numbers become the strings `"Infinity"`, `"-Infinity"` or `"NaN"` so that
    /// traces stay valid JSON.
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            Value::Null => serde_json::Value::Null,
            Value::Bool(b) => serde_json::Value::Bool(*b),
            Value::Number(n) => number_to_json(*n),
            Value::String(s) => serde_json::Value::String(s.clone()),
            Value::Array(items) => serde_json::Value::Array(items.iter().map(Value::to_json).collect()),
            Value::Object(map) => serde_json::Value::Object(map.clone()),
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        }
    }

    /// Number coercion for comparisons: numbers, and strings that parse as numbers. Bools are *not* numbers
    /// in comparisons (use [`Value::as_formula_number`] for formulas).
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            Value::String(s) => parse_number(s),
            _ => None,
        }
    }

    /// Number coercion for formula arguments: like [`Value::as_number`] plus `true → 1`, `false → 0`.
    pub fn as_formula_number(&self) -> Option<f64> {
        match self {
            Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            other => other.as_number(),
        }
    }

    /// RFC 3339 date-time coercion (strings only).
    pub fn as_datetime(&self) -> Option<DateTime<Utc>> {
        match self {
            Value::String(s) => DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|d| d.with_timezone(&Utc)),
            _ => None,
        }
    }

    /// Scalar rendered as text: strings as-is, numbers without trailing `.0` for integers, bools as
    /// `true`/`false`. Used for reference-list keys, regex and string operators.
    pub fn as_text(&self) -> Option<String> {
        match self {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(format_number(*n)),
            Value::Bool(b) => Some(b.to_string()),
            _ => None,
        }
    }
}

fn parse_number(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    t.parse::<f64>().ok().filter(|n| n.is_finite())
}

/// Formats a number compactly: `5` instead of `5.0`, full precision otherwise.
pub fn format_number(n: f64) -> String {
    if n.fract() == 0.0 && n.is_finite() && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// Converts an `f64` into JSON; integers stay integers, non-finite values become strings.
pub fn number_to_json(n: f64) -> serde_json::Value {
    if n.is_nan() {
        return serde_json::Value::String("NaN".into());
    }
    if n.is_infinite() {
        return serde_json::Value::String(if n > 0.0 { "Infinity" } else { "-Infinity" }.into());
    }
    if n.fract() == 0.0 && n.abs() < 9e15 {
        return serde_json::Value::Number(Number::from(n as i64));
    }
    Number::from_f64(n).map_or(serde_json::Value::Null, serde_json::Value::Number)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn coercions() {
        assert_eq!(Value::from_json(&json!("150000.50")).as_number(), Some(150_000.5));
        assert_eq!(Value::from_json(&json!(" 7 ")).as_number(), Some(7.0));
        assert_eq!(Value::from_json(&json!("abc")).as_number(), None);
        assert_eq!(Value::Bool(true).as_number(), None);
        assert_eq!(Value::Bool(true).as_formula_number(), Some(1.0));
        assert!(Value::from_json(&json!("2026-01-01T00:00:00Z"))
            .as_datetime()
            .is_some());
        assert_eq!(Value::Number(5.0).as_text().as_deref(), Some("5"));
        assert_eq!(Value::Number(5.25).as_text().as_deref(), Some("5.25"));
    }

    #[test]
    fn non_finite_json() {
        assert_eq!(number_to_json(f64::INFINITY), json!("Infinity"));
        assert_eq!(number_to_json(4.0), json!(4));
        assert_eq!(number_to_json(0.5), json!(0.5));
    }
}
