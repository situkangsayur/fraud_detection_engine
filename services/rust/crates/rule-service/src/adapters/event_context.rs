//! Reconstructs the rule-dsl §5 evaluation context of stored events (rule tests and backtests).
//!
//! `event` = the `core.events` row as JSON (without internal columns), `source` = the stored payload,
//! `customer` = customer columns + `account_age_days` at event time, `features` = `core.event_features`,
//! `ml` / `graph` = what the decision recorded (empty objects when the event was never scored, e.g. `load_only`).
//! rule_service has SELECT grants on exactly these core tables (db/migrations/0007).

use chrono::{DateTime, Utc};
use contracts::scoring::EvaluationContext;
use platform::{AppError, AppResult, ProjectId};
use serde_json::{json, Value};
use sqlx::{FromRow, PgConnection};
use uuid::Uuid;

use crate::domain::backtest::Label;

#[derive(Debug, Clone, FromRow)]
pub struct StoredEvent {
    pub id: Uuid,
    pub event_type: String,
    pub occurred_at: DateTime<Utc>,
    pub customer_id: Uuid,
    pub event: Value,
    pub source: Value,
    pub customer: Value,
    pub features: Value,
    pub ml: Value,
    pub graph: Value,
    pub label: Option<String>,
}

impl StoredEvent {
    pub fn context(&self) -> EvaluationContext {
        EvaluationContext {
            event: self.event.clone(),
            source: self.source.clone(),
            customer: self.customer.clone(),
            features: self.features.clone(),
            ml: self.ml.clone(),
            graph: self.graph.clone(),
        }
    }

    pub fn label(&self) -> Option<Label> {
        Label::parse(self.label.as_deref())
    }
}

const SELECT: &str = "SELECT e.id, e.event_type, e.occurred_at, e.customer_id, \
    (to_jsonb(e) - 'payload' - 'tenant_id' - 'project_id' - 'data_source_id' - 'load_only' - 'needs_rescore' \
                 - 'received_at') AS event, \
    e.payload AS source, \
    jsonb_build_object('external_id', c.external_id, 'kyc_level', c.kyc_level, 'segment', c.segment, \
        'status', c.status, 'registered_at', c.registered_at, 'risk_label', c.risk_label, 'attributes', c.attributes, \
        'account_age_days', CASE WHEN c.registered_at IS NULL THEN NULL \
                                 ELSE extract(epoch FROM (e.occurred_at - c.registered_at)) / 86400.0 END) AS customer, \
    COALESCE(f.features, '{}'::jsonb) AS features, COALESCE(d.ml, '{}'::jsonb) AS ml, \
    COALESCE(d.graph, '{}'::jsonb) AS graph, l.label \
    FROM core.events e JOIN core.customers c ON c.id = e.customer_id \
    LEFT JOIN core.event_features f ON f.event_id = e.id \
    LEFT JOIN core.decisions d ON d.event_id = e.id \
    LEFT JOIN core.event_labels l ON l.event_id = e.id";

pub async fn load_event(
    conn: &mut PgConnection,
    project: ProjectId,
    event_id: Uuid,
) -> AppResult<StoredEvent> {
    sqlx::query_as::<_, StoredEvent>(&format!("{SELECT} WHERE e.project_id = $1 AND e.id = $2"))
        .bind(project.as_uuid())
        .bind(event_id)
        .fetch_optional(conn)
        .await?
        .ok_or_else(|| AppError::not_found("event not found"))
}

/// Most recent events in `[from, to)` of the given types (empty = all), newest first, up to `limit + 1` rows so
/// the caller can detect truncation.
pub async fn load_window(
    conn: &mut PgConnection,
    project: ProjectId,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    event_types: &[String],
    limit: i64,
) -> AppResult<Vec<StoredEvent>> {
    let types: Option<&[String]> = if event_types.is_empty() {
        None
    } else {
        Some(event_types)
    };
    Ok(sqlx::query_as::<_, StoredEvent>(&format!(
        "{SELECT} WHERE e.project_id = $1 AND e.occurred_at >= $2 AND e.occurred_at < $3 \
         AND ($4::text[] IS NULL OR e.event_type = ANY($4)) ORDER BY e.occurred_at DESC LIMIT $5"
    ))
    .bind(project.as_uuid())
    .bind(from)
    .bind(to)
    .bind(types)
    .bind(limit + 1)
    .fetch_all(conn)
    .await?)
}

/// Engine context JSON: the §5 object plus `event.customer_id` (history grouping) guaranteed present.
pub fn engine_data(ctx: &EvaluationContext, customer_id: Uuid) -> Value {
    let mut event = if ctx.event.is_object() {
        ctx.event.clone()
    } else {
        json!({})
    };
    if let Some(obj) = event.as_object_mut() {
        obj.entry("customer_id")
            .or_insert_with(|| json!(customer_id.to_string()));
    }
    let obj = |v: &Value| if v.is_object() { v.clone() } else { json!({}) };
    json!({
        "event": event,
        "source": obj(&ctx.source),
        "customer": obj(&ctx.customer),
        "features": obj(&ctx.features),
        "ml": obj(&ctx.ml),
        "graph": obj(&ctx.graph),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_data_injects_customer_id_and_normalises_missing_parts() {
        let ctx = EvaluationContext {
            event: json!({"amount": 5}),
            ..Default::default()
        };
        let id = Uuid::from_u128(7);
        let data = engine_data(&ctx, id);
        assert_eq!(data["event"]["customer_id"], json!(id.to_string()));
        assert_eq!(data["ml"], json!({}));
        assert_eq!(data["source"], json!({}));
        // An explicit customer_id wins.
        let ctx = EvaluationContext {
            event: json!({"customer_id": "x"}),
            ..Default::default()
        };
        assert_eq!(engine_data(&ctx, id)["event"]["customer_id"], json!("x"));
    }
}
