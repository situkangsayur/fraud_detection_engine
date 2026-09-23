//! Evaluation context (rule-dsl §5) and path resolution.
//!
//! The context is one JSON document `{event, source, customer, features, ml, graph}` plus a few typed
//! identifiers the engine needs to build data-provider queries (current event id, customer id, event type and
//! time). Paths use dots and `[n]` indexes: `source.order.items[0].sku`.

use chrono::{DateTime, Utc};
use serde_json::Value as Json;

/// Result of resolving a path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Resolved<'a> {
    /// The path does not exist.
    Missing,
    /// The path exists and holds `null`.
    Null,
    /// The path holds a non-null value.
    Value(&'a Json),
}

/// Everything a rule can read about the current event.
#[derive(Debug, Clone)]
pub struct EvalContext {
    /// `{event, source, customer, features, ml, graph}`.
    pub data: Json,
    /// Current event id (excluded from history unless `include_current`).
    pub event_id: Option<String>,
    /// Current customer id (graph queries).
    pub customer_id: Option<String>,
    /// Current event type (default history event type, rule applicability).
    pub event_type: String,
    /// Event time: the end of every sliding window.
    pub occurred_at: DateTime<Utc>,
}

impl EvalContext {
    pub fn new(data: Json, event_type: impl Into<String>, occurred_at: DateTime<Utc>) -> Self {
        Self {
            data,
            event_id: None,
            customer_id: None,
            event_type: event_type.into(),
            occurred_at,
        }
    }

    pub fn with_event_id(mut self, id: impl Into<String>) -> Self {
        self.event_id = Some(id.into());
        self
    }

    pub fn with_customer_id(mut self, id: impl Into<String>) -> Self {
        self.customer_id = Some(id.into());
        self
    }

    /// Resolves a context path such as `event.amount`.
    pub fn resolve(&self, path: &str) -> Resolved<'_> {
        resolve_path(&self.data, path)
    }
}

/// Maps a history field name (as used in `group_by`, `aggregate.field`, `hist` operands) to the context path of
/// the same field on the current event: `amount` → `event.amount`, `source.order.total` → itself.
pub fn history_field_context_path(field: &str) -> String {
    if field.starts_with("source.") {
        field.to_string()
    } else {
        format!("event.{field}")
    }
}

/// Resolves `a.b[0].c` inside a JSON document.
pub fn resolve_path<'a>(root: &'a Json, path: &str) -> Resolved<'a> {
    let Some(segments) = parse_path(path) else {
        return Resolved::Missing;
    };
    let mut current = root;
    for segment in &segments {
        let next = match segment {
            Segment::Key(key) => current.as_object().and_then(|m| m.get(*key)),
            Segment::Index(i) => current.as_array().and_then(|a| a.get(*i)),
        };
        match next {
            Some(v) => current = v,
            None => return Resolved::Missing,
        }
    }
    if current.is_null() {
        Resolved::Null
    } else {
        Resolved::Value(current)
    }
}

#[derive(Debug, PartialEq)]
enum Segment<'a> {
    Key(&'a str),
    Index(usize),
}

/// Splits a path into key/index segments; `None` for syntactically invalid paths.
fn parse_path(path: &str) -> Option<Vec<Segment<'_>>> {
    let mut out = Vec::new();
    for part in path.split('.') {
        let (key, mut rest) = match part.find('[') {
            Some(i) => (&part[..i], &part[i..]),
            None => (part, ""),
        };
        if key.is_empty() {
            return None;
        }
        out.push(Segment::Key(key));
        while !rest.is_empty() {
            let close = rest.find(']')?;
            if !rest.starts_with('[') {
                return None;
            }
            let index: usize = rest[1..close].parse().ok()?;
            out.push(Segment::Index(index));
            rest = &rest[close + 1..];
        }
    }
    Some(out)
}

/// Whether a path is syntactically valid (`a.b[0].c`).
pub fn is_valid_path(path: &str) -> bool {
    parse_path(path).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resolves_nested_paths() {
        let doc =
            json!({"event": {"amount": 5, "x": null}, "source": {"items": [{"sku": "A"}, {"sku": "B"}]}});
        assert_eq!(resolve_path(&doc, "event.amount"), Resolved::Value(&json!(5)));
        assert_eq!(resolve_path(&doc, "event.x"), Resolved::Null);
        assert_eq!(resolve_path(&doc, "event.y"), Resolved::Missing);
        assert_eq!(
            resolve_path(&doc, "source.items[1].sku"),
            Resolved::Value(&json!("B"))
        );
        assert_eq!(resolve_path(&doc, "source.items[5].sku"), Resolved::Missing);
        assert_eq!(resolve_path(&doc, "event..amount"), Resolved::Missing);
        assert!(!is_valid_path("a[x]"));
        assert!(is_valid_path("a.b[0][1].c"));
    }

    #[test]
    fn history_paths() {
        assert_eq!(history_field_context_path("amount"), "event.amount");
        assert_eq!(
            history_field_context_path("source.order.total"),
            "source.order.total"
        );
    }
}
