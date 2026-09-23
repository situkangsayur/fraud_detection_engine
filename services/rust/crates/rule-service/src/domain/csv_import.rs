//! CSV → reference-list entries (pure parsing and validation; the adapter persists in batches).
//!
//! Format: a header row; the first column is the key, the remaining columns become entry `attributes`. Values of
//! attribute columns declared as `number`/`integer`/`bool` in the list's `columns` are coerced; everything else
//! stays a string. Optional reserved columns `valid_from`, `valid_until` (RFC 3339) and `reason` map to entry
//! fields instead of attributes.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Map, Value};

pub const MAX_KEY_LEN: usize = 512;

/// A parsed entry ready to upsert.
#[derive(Debug, Clone, PartialEq)]
pub struct EntryRow {
    pub key: String,
    pub attributes: Value,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_until: Option<DateTime<Utc>>,
    pub reason: Option<String>,
}

/// A rejected CSV line (1-based line number including the header).
#[derive(Debug, Clone, PartialEq, serde::Serialize, utoipa::ToSchema)]
pub struct RowError {
    pub line: u64,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ColumnDef {
    name: String,
    #[serde(rename = "type", default)]
    data_type: String,
}

/// Column types from `reference_lists.columns` (`[{name, type}]`).
pub fn column_types(columns: &Value) -> HashMap<String, String> {
    serde_json::from_value::<Vec<ColumnDef>>(columns.clone())
        .unwrap_or_default()
        .into_iter()
        .map(|c| (c.name, c.data_type))
        .collect()
}

fn coerce(raw: &str, data_type: Option<&str>) -> Result<Value, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(Value::Null);
    }
    match data_type {
        Some("number") | Some("integer") => trimmed
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .ok_or_else(|| format!("'{trimmed}' is not a number")),
        Some("bool") | Some("boolean") => match trimmed.to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "y" => Ok(Value::Bool(true)),
            "false" | "0" | "no" | "n" => Ok(Value::Bool(false)),
            _ => Err(format!("'{trimmed}' is not a boolean")),
        },
        _ => Ok(Value::String(trimmed.to_string())),
    }
}

fn timestamp(raw: &str, column: &str) -> Result<Option<DateTime<Utc>>, String> {
    let t = raw.trim();
    if t.is_empty() {
        return Ok(None);
    }
    DateTime::parse_from_rfc3339(t)
        .map(|d| Some(d.with_timezone(&Utc)))
        .map_err(|_| format!("{column} '{t}' is not an RFC 3339 timestamp"))
}

/// Parses a whole CSV document. Returns accepted rows and per-line errors (bad lines never abort the import).
pub fn parse(data: &[u8], columns: &Value) -> Result<(Vec<EntryRow>, Vec<RowError>), String> {
    let types = column_types(columns);
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(false)
        .trim(csv::Trim::Headers)
        .from_reader(data);
    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| format!("cannot read CSV header: {e}"))?
        .iter()
        .map(|h| h.trim_start_matches('\u{feff}').to_string())
        .collect();
    if headers.is_empty() || headers[0].is_empty() {
        return Err("the first CSV column must be the key".into());
    }

    let mut rows = Vec::new();
    let mut errors = Vec::new();
    for (i, record) in reader.records().enumerate() {
        let line = i as u64 + 2;
        let record = match record {
            Ok(r) => r,
            Err(e) => {
                errors.push(RowError {
                    line,
                    message: e.to_string(),
                });
                continue;
            }
        };
        match parse_record(&headers, &record, &types) {
            Ok(row) => rows.push(row),
            Err(message) => errors.push(RowError { line, message }),
        }
    }
    Ok((rows, errors))
}

fn parse_record(
    headers: &[String],
    record: &csv::StringRecord,
    types: &HashMap<String, String>,
) -> Result<EntryRow, String> {
    let key = record.get(0).unwrap_or_default().trim().to_string();
    if key.is_empty() {
        return Err("empty key".into());
    }
    if key.len() > MAX_KEY_LEN {
        return Err(format!("key longer than {MAX_KEY_LEN} characters"));
    }
    let mut attributes = Map::new();
    let mut row = EntryRow {
        key,
        attributes: Value::Null,
        valid_from: None,
        valid_until: None,
        reason: None,
    };
    for (idx, header) in headers.iter().enumerate().skip(1) {
        let raw = record.get(idx).unwrap_or_default();
        match header.as_str() {
            "valid_from" => row.valid_from = timestamp(raw, header)?,
            "valid_until" => row.valid_until = timestamp(raw, header)?,
            "reason" => row.reason = Some(raw.trim().to_string()).filter(|s| !s.is_empty()),
            name => {
                let value = coerce(raw, types.get(name).map(String::as_str))
                    .map_err(|e| format!("column {name}: {e}"))?;
                if !value.is_null() {
                    attributes.insert(name.to_string(), value);
                }
            }
        }
    }
    if let (Some(from), Some(until)) = (row.valid_from, row.valid_until) {
        if until <= from {
            return Err("valid_until must be after valid_from".into());
        }
    }
    row.attributes = Value::Object(attributes);
    Ok(row)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn parses_keys_attributes_and_reserved_columns() {
        let csv = "\u{feff}key,max_amount,note,reason,valid_until\n\
                   M-1,5000000,\"big, trusted\",limit,2030-01-01T00:00:00Z\n\
                   M-2,,x,,\n";
        let cols = json!([{"name": "max_amount", "type": "number"}]);
        let (rows, errors) = parse(csv.as_bytes(), &cols).unwrap();
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].key, "M-1");
        assert_eq!(
            rows[0].attributes,
            json!({"max_amount": 5000000.0, "note": "big, trusted"})
        );
        assert_eq!(rows[0].reason.as_deref(), Some("limit"));
        assert!(rows[0].valid_until.is_some());
        assert_eq!(rows[1].attributes, json!({"note": "x"}));
    }

    #[test]
    fn bad_rows_are_reported_not_fatal() {
        let csv = "key,max_amount\n,1\nA,abc\nB,2\n";
        let cols = json!([{"name": "max_amount", "type": "number"}]);
        let (rows, errors) = parse(csv.as_bytes(), &cols).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(errors.len(), 2);
        assert_eq!(errors[0].line, 2);
        assert!(errors[1].message.contains("not a number"));
    }

    #[test]
    fn ragged_rows_are_errors() {
        let (rows, errors) = parse(b"key,a\nX,1,2\nY,3\n", &json!([])).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(errors.len(), 1);
    }
}
