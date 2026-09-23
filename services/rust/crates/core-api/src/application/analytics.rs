//! Analytics for the dashboard and for the LLM assistant (`overview`, `drift`, `typologies`).

use chrono::{DateTime, Duration, Utc};
use contracts::catalog::{DataType, Entity, BUILTIN_FIELDS};
use platform::auth::{Caller, ProjectRole};
use platform::db::TenantTx;
use platform::error::AppResult;
use platform::ProjectId;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::application::context::project_ctx;
use crate::domain::psi::{mean, psi, DriftStatus};
use crate::state::AppState;

#[derive(Debug, Clone, Default, Deserialize, utoipa::IntoParams)]
pub struct Range {
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
}

impl Range {
    fn resolve(&self, default_days: i64) -> (DateTime<Utc>, DateTime<Utc>) {
        let to = self.to.unwrap_or_else(Utc::now);
        let from = self.from.unwrap_or(to - Duration::days(default_days));
        (from, to)
    }
}

pub async fn overview(st: &AppState, caller: &Caller, project: ProjectId, r: &Range) -> AppResult<Value> {
    let tenant = caller.require_project_role(project, ProjectRole::Viewer).await?;
    let pctx = project_ctx(st, tenant, project).await?;
    let tz = pctx.timezone.name().to_string();
    let (from, to) = r.resolve(30);
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let (v,): (Value,) = sqlx::query_as(
        r#"
        WITH ev AS (
            SELECT e.id, e.event_type, e.occurred_at FROM core.events e
            WHERE e.project_id = $1 AND e.occurred_at >= $2 AND e.occurred_at < $3
        ), dec AS (
            SELECT d.decision, d.final_score, d.engine_scores, d.degraded, ev.occurred_at
            FROM core.decisions d JOIN ev ON ev.id = d.event_id
        )
        SELECT jsonb_build_object(
            'from', $2::timestamptz, 'to', $3::timestamptz,
            'totals', (SELECT jsonb_build_object(
                'events', (SELECT count(*) FROM ev),
                'decisions', count(*),
                'approve', count(*) FILTER (WHERE decision = 'approve'),
                'review', count(*) FILTER (WHERE decision = 'review'),
                'decline', count(*) FILTER (WHERE decision = 'decline')) FROM dec),
            'by_event_type', COALESCE((SELECT jsonb_agg(jsonb_build_object('event_type', event_type, 'count', n)
                                                        ORDER BY n DESC)
                              FROM (SELECT event_type, count(*) AS n FROM ev GROUP BY event_type) x), '[]'),
            'by_label_fraud_type', COALESCE((SELECT jsonb_agg(jsonb_build_object('fraud_type', ft, 'count', n)
                                                              ORDER BY n DESC)
                              FROM (SELECT COALESCE(fraud_type, 'other') AS ft, count(*) AS n FROM core.labels
                                    WHERE project_id = $1 AND label = 'fraud'
                                      AND created_at >= $2 AND created_at < $3 GROUP BY 1) x), '[]'),
            'daily', COALESCE((SELECT jsonb_agg(jsonb_build_object('date', day, 'events', events,
                                        'review', review, 'decline', decline, 'avg_score', avg_score) ORDER BY day)
                              FROM (SELECT (ev.occurred_at AT TIME ZONE $4)::date AS day, count(*) AS events,
                                           count(*) FILTER (WHERE d.decision = 'review') AS review,
                                           count(*) FILTER (WHERE d.decision = 'decline') AS decline,
                                           round(avg(d.final_score)::numeric, 2)::float8 AS avg_score
                                    FROM ev LEFT JOIN core.decisions d ON d.event_id = ev.id GROUP BY 1) x), '[]'),
            'score_histogram', COALESCE((SELECT jsonb_agg(jsonb_build_object('bucket', b * 10 - 10, 'count', n)
                                                          ORDER BY b)
                              FROM (SELECT LEAST(width_bucket(final_score, 0, 100, 10), 10) AS b, count(*) AS n
                                    FROM dec GROUP BY 1) x), '[]'),
            'engine_avg', (SELECT jsonb_build_object(
                'rules', round(avg((engine_scores->>'rules')::float8)::numeric, 2),
                'supervised', round(avg((engine_scores->>'supervised')::float8)::numeric, 2),
                'unsupervised', round(avg((engine_scores->>'unsupervised')::float8)::numeric, 2),
                'graph', round(avg((engine_scores->>'graph')::float8)::numeric, 2)) FROM dec),
            'open_cases', (SELECT count(*) FROM core.cases WHERE project_id = $1 AND status IN ('open','in_review')),
            'degraded_rate', (SELECT CASE WHEN count(*) = 0 THEN 0
                              ELSE round((count(*) FILTER (WHERE cardinality(degraded) > 0))::numeric / count(*), 4)
                              END FROM dec)
        )
        "#,
    )
    .bind(project.as_uuid())
    .bind(from)
    .bind(to)
    .bind(tz)
    .fetch_one(&mut **tx)
    .await?;
    tx.commit().await?;
    Ok(v)
}

const SAMPLE: i64 = 20_000;

/// PSI per numeric v1 feature: recent 7 days vs the 30 days before.
pub async fn drift(st: &AppState, caller: &Caller, project: ProjectId) -> AppResult<Value> {
    let tenant = caller.require_project_role(project, ProjectRole::Viewer).await?;
    let now = Utc::now();
    let recent_from = now - Duration::days(7);
    let base_from = recent_from - Duration::days(30);
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let fetch = |from: DateTime<Utc>, to: DateTime<Utc>| {
        sqlx::query_as::<_, (Value,)>(
            "SELECT f.features FROM core.event_features f JOIN core.events e ON e.id = f.event_id \
             WHERE e.project_id = $1 AND e.occurred_at >= $2 AND e.occurred_at < $3 \
             ORDER BY e.occurred_at DESC LIMIT $4",
        )
        .bind(project.as_uuid())
        .bind(from)
        .bind(to)
        .bind(SAMPLE)
    };
    let recent: Vec<(Value,)> = fetch(recent_from, now).fetch_all(&mut **tx).await?;
    let baseline: Vec<(Value,)> = fetch(base_from, recent_from).fetch_all(&mut **tx).await?;
    tx.commit().await?;

    let numeric: Vec<&str> = BUILTIN_FIELDS
        .iter()
        .filter(|f| {
            f.entity == Entity::Features && matches!(f.data_type, DataType::Number | DataType::Integer)
        })
        .filter_map(|f| f.path.strip_prefix("features."))
        .collect();
    let column = |rows: &[(Value,)], name: &str| -> Vec<f64> {
        rows.iter()
            .filter_map(|(v,)| v.get(name).and_then(Value::as_f64))
            .collect()
    };
    let mut items = Vec::new();
    for name in numeric {
        let b = column(&baseline, name);
        let r = column(&recent, name);
        let Some(p) = psi(&b, &r) else { continue };
        items.push(json!({
            "feature": name, "psi": p, "recent_mean": mean(&r), "baseline_mean": mean(&b),
            "recent_n": r.len(), "baseline_n": b.len(), "status": DriftStatus::of(p).as_str(),
        }));
    }
    items.sort_by(|a, b| {
        b["psi"]
            .as_f64()
            .unwrap_or(0.0)
            .total_cmp(&a["psi"].as_f64().unwrap_or(0.0))
    });
    Ok(json!({
        "recent_window": { "from": recent_from, "to": now },
        "baseline_window": { "from": base_from, "to": recent_from },
        "items": items,
    }))
}

/// Labelled fraud per typology and ISO week (count and amount of labelled events).
pub async fn typologies(st: &AppState, caller: &Caller, project: ProjectId, r: &Range) -> AppResult<Value> {
    let tenant = caller.require_project_role(project, ProjectRole::Viewer).await?;
    let (from, to) = r.resolve(90);
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let rows: Vec<(Value,)> = sqlx::query_as(
        "SELECT jsonb_build_object('fraud_type', ft, 'week', wk, 'count', n, 'amount', amt) FROM ( \
            SELECT COALESCE(l.fraud_type, 'other') AS ft, date_trunc('week', l.created_at)::date AS wk, \
                   count(*) AS n, COALESCE(sum(e.amount), 0)::float8 AS amt \
            FROM core.labels l LEFT JOIN core.events e ON l.subject_type = 'event' AND e.id = l.subject_id \
            WHERE l.project_id = $1 AND l.label = 'fraud' AND l.created_at >= $2 AND l.created_at < $3 \
            GROUP BY 1, 2 ORDER BY 2, 1) x",
    )
    .bind(project.as_uuid())
    .bind(from)
    .bind(to)
    .fetch_all(&mut **tx)
    .await?;
    tx.commit().await?;
    let items: Vec<Value> = rows.into_iter().map(|r| r.0).collect();
    let mut totals: Map<String, Value> = Map::new();
    for it in &items {
        let ft = it["fraud_type"].as_str().unwrap_or("other").to_string();
        let e = totals
            .entry(ft)
            .or_insert_with(|| json!({ "count": 0, "amount": 0.0 }));
        e["count"] = json!(e["count"].as_i64().unwrap_or(0) + it["count"].as_i64().unwrap_or(0));
        e["amount"] = json!(e["amount"].as_f64().unwrap_or(0.0) + it["amount"].as_f64().unwrap_or(0.0));
    }
    Ok(json!({ "from": from, "to": to, "items": items, "totals": totals }))
}
