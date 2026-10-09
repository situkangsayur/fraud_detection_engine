//! `POST /api/v1/projects/{pid}/formulas/evaluate`: formula playground for the UI rule builder.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use platform::auth::{Caller, ProjectRole};
use platform::AppResult;
use rule_engine::formula::Formula;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use super::dto::project_scope;
use crate::state::AppState;

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct FormulaBody {
    pub expr: String,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub variables: HashMap<String, Value>,
}

#[derive(Debug, Serialize, PartialEq, utoipa::ToSchema)]
pub struct FormulaOut {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    pub trapped: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Header name/params when the formula declares `F(x, y) = …`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub params: Vec<String>,
}

/// Numeric coercion used by formulas (rule-dsl §3.1): numbers, booleans (1/0) and numeric strings.
fn to_number(v: &Value) -> Result<f64, String> {
    match v {
        Value::Number(n) => n.as_f64().ok_or_else(|| "not a finite number".to_string()),
        Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        Value::String(s) => s
            .trim()
            .parse::<f64>()
            .map_err(|_| format!("'{s}' is not numeric")),
        Value::Null => Err("null".to_string()),
        _ => Err("not a scalar".to_string()),
    }
}

/// Pure evaluation (unit-tested); `Err` is a parse error `(message, position)`.
pub fn evaluate_formula(body: &FormulaBody) -> Result<FormulaOut, (String, usize)> {
    let formula = Formula::parse(&body.expr).map_err(|e| (e.message, e.position))?;
    let names: BTreeSet<String> = body.variables.keys().cloned().collect();
    formula
        .check_bindings(&names)
        .map_err(|e| (e.message, e.position))?;
    let name = formula.name().map(str::to_string);
    let params = formula.params().map(<[String]>::to_vec).unwrap_or_default();
    let mut vars = BTreeMap::new();
    for (k, v) in &body.variables {
        match to_number(v) {
            Ok(x) => {
                vars.insert(k.clone(), x);
            }
            Err(why) => {
                return Ok(FormulaOut {
                    value: None,
                    trapped: true,
                    reason: Some(format!("argument {k} is {why}")),
                    name,
                    params,
                })
            }
        }
    }
    Ok(match formula.eval(&vars) {
        Ok(value) => FormulaOut {
            value: Some(value),
            trapped: false,
            reason: None,
            name,
            params,
        },
        Err(e) => FormulaOut {
            value: None,
            trapped: true,
            reason: Some(e.to_string()),
            name,
            params,
        },
    })
}

#[utoipa::path(post, path = "/api/v1/projects/{pid}/formulas/evaluate", params(("pid" = Uuid, Path)),
    request_body = FormulaBody,
    responses((status = 200, body = FormulaOut), (status = 422, description = "Parse error {message, position}")),
    tag = "formulas")]
pub async fn evaluate(
    State(_state): State<AppState>,
    caller: Caller,
    Path(pid): Path<Uuid>,
    Json(body): Json<FormulaBody>,
) -> AppResult<Response> {
    project_scope(&caller, pid, ProjectRole::Viewer).await?;
    Ok(match evaluate_formula(&body) {
        Ok(out) => Json(out).into_response(),
        Err((message, position)) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            [(axum::http::header::CONTENT_TYPE, "application/problem+json")],
            Json(json!({
                "type": "https://fraud-platform.local/problems/formula-parse-error",
                "title": "Unprocessable Entity",
                "status": 422,
                "detail": message,
                "message": message,
                "position": position,
            })),
        )
            .into_response(),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn body(expr: &str, vars: Value) -> FormulaBody {
        FormulaBody {
            expr: expr.into(),
            variables: serde_json::from_value(vars).unwrap(),
        }
    }

    #[test]
    fn spec_example() {
        let out = evaluate_formula(&body(
            "F(x,y,z) = 2x + 2^y / z^2",
            json!({"x": 10, "y": 3, "z": 2}),
        ))
        .unwrap();
        assert_eq!(out.value, Some(22.0));
        assert_eq!(out.name.as_deref(), Some("F"));
        assert_eq!(out.params, vec!["x", "y", "z"]);
    }

    #[test]
    fn traps_and_parse_errors() {
        let out = evaluate_formula(&body("a / b", json!({"a": 1, "b": 0}))).unwrap();
        assert!(out.trapped);
        let out = evaluate_formula(&body("a + 1", json!({"a": null}))).unwrap();
        assert!(out.trapped && out.reason.unwrap().contains("null"));
        let out = evaluate_formula(&body("a + 1", json!({"a": "2.5"}))).unwrap();
        assert_eq!(out.value, Some(3.5));
        let err = evaluate_formula(&body("2 * (a +", json!({"a": 1}))).unwrap_err();
        assert!(err.1 > 0);
        assert!(evaluate_formula(&body("F(x) = x + y", json!({"x": 1}))).is_err());
    }
}
