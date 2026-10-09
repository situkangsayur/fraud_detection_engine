//! Read models: events, customers, decisions, audit.
//!
//! List endpoints build the JSON in SQL (`to_jsonb`) and use `count(*) OVER ()` for the total, so a
//! page costs one query. All filters are bound parameters (never string-interpolated).

use chrono::{DateTime, Utc};
use platform::auth::{Caller, ProjectRole};
use platform::db::TenantTx;
use platform::error::{AppError, AppResult};
use platform::pagination::{Page, PageParams};
use platform::{ProjectId, TenantId};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::adapters::repo;
use crate::state::AppState;

use super::util::{collect_page, effective_role, mask_pii};

#[derive(Debug, Clone, Default, Deserialize, utoipa::IntoParams)]
pub struct EventFilter {
    pub event_type: Option<String>,
    pub decision: Option<String>,
    pub customer_id: Option<Uuid>,
    pub source_id: Option<Uuid>,
    #[serde(default, deserialize_with = "platform::query_time::opt_from")]
    pub from: Option<DateTime<Utc>>,
    #[serde(default, deserialize_with = "platform::query_time::opt_to")]
    pub to: Option<DateTime<Utc>>,
    pub min_score: Option<f64>,
    /// External id (exact or prefix) of the event or of its customer.
    pub q: Option<String>,
}

pub async fn list_events(
    st: &AppState,
    caller: &Caller,
    project: ProjectId,
    f: &EventFilter,
    page: &PageParams,
) -> AppResult<Page<Value>> {
    let tenant = caller.require_project_role(project, ProjectRole::Viewer).await?;
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let rows: Vec<(Value, i64)> = sqlx::query_as(
        "SELECT to_jsonb(t) AS item, count(*) OVER () FROM ( \
            SELECT e.id, e.external_id, e.event_type, e.occurred_at, e.received_at, e.customer_id, \
                   c.external_id AS customer_external_id, e.amount::float8 AS amount, e.currency::text AS currency, \
                   e.channel, e.data_source_id, e.load_only, d.decision, d.final_score::float8 AS final_score \
            FROM core.events e \
            JOIN core.customers c ON c.id = e.customer_id \
            LEFT JOIN core.decisions d ON d.event_id = e.id \
            WHERE e.project_id = $1 \
              AND ($2::text IS NULL OR e.event_type = $2) \
              AND ($3::text IS NULL OR d.decision = $3) \
              AND ($4::uuid IS NULL OR e.customer_id = $4) \
              AND ($5::uuid IS NULL OR e.data_source_id = $5) \
              AND ($6::timestamptz IS NULL OR e.occurred_at >= $6) \
              AND ($7::timestamptz IS NULL OR e.occurred_at < $7) \
              AND ($8::float8 IS NULL OR d.final_score >= $8) \
              AND ($9::text IS NULL \
                   OR e.id IN (SELECT x.id FROM core.events x WHERE x.project_id = $1 \
                               AND x.external_id ~>=~ $9 AND x.external_id ~<~ ($9 || chr(1114111))) \
                   OR e.customer_id IN (SELECT y.id FROM core.customers y WHERE y.project_id = $1 AND y.external_id = $9)) \
            ORDER BY e.occurred_at DESC, e.id) t \
         LIMIT $10 OFFSET $11",
    )
    .bind(project.as_uuid())
    .bind(&f.event_type)
    .bind(&f.decision)
    .bind(f.customer_id)
    .bind(f.source_id)
    .bind(f.from)
    .bind(f.to)
    .bind(f.min_score)
    .bind(f.q.as_deref().map(str::trim).filter(|s| !s.is_empty()))
    .bind(page.limit())
    .bind(page.offset())
    .fetch_all(&mut **tx)
    .await?;
    tx.commit().await?;
    Ok(collect_page(rows, page))
}

pub async fn get_event(st: &AppState, caller: &Caller, project: ProjectId, id: Uuid) -> AppResult<Value> {
    let tenant = caller.require_project_role(project, ProjectRole::Viewer).await?;
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let row: Option<(Value, Value, Option<Value>, Value)> = sqlx::query_as(
        "SELECT to_jsonb(e.*) - ARRAY['tenant_id','payload'], e.payload, f.features, \
                jsonb_build_object('id', c.id, 'external_id', c.external_id, 'full_name', c.full_name, \
                                   'email', c.email, 'phone', c.phone, 'risk_label', c.risk_label, \
                                   'status', c.status, 'segment', c.segment, 'kyc_level', c.kyc_level) \
         FROM core.events e \
         JOIN core.customers c ON c.id = e.customer_id \
         LEFT JOIN core.event_features f ON f.event_id = e.id \
         WHERE e.project_id = $1 AND e.id = $2",
    )
    .bind(project.as_uuid())
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?;
    let (mut event, payload, features, mut customer) =
        row.ok_or_else(|| AppError::not_found("event not found"))?;
    let decision = repo::load_decision_out(&mut tx, project, id).await?;
    let labels: Vec<(Value,)> = sqlx::query_as(
        "SELECT to_jsonb(l) - 'tenant_id' FROM core.labels l \
         WHERE l.project_id = $1 AND l.subject_type = 'event' AND l.subject_id = $2 ORDER BY created_at DESC",
    )
    .bind(project.as_uuid())
    .bind(id)
    .fetch_all(&mut **tx)
    .await?;
    let case: Option<(Value,)> = sqlx::query_as(
        "SELECT jsonb_build_object('id', id, 'status', status, 'priority', priority) FROM core.cases \
         WHERE project_id = $1 AND $2 = ANY(event_ids) ORDER BY created_at DESC LIMIT 1",
    )
    .bind(project.as_uuid())
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?;
    tx.commit().await?;
    if effective_role(caller, project) == ProjectRole::Viewer {
        mask_pii(&mut event);
        mask_pii(&mut customer);
    }
    Ok(json!({
        "event": event, "source": payload, "features": features, "customer": customer,
        "decision": decision, "labels": labels.into_iter().map(|l| l.0).collect::<Vec<_>>(),
        "case": case.map(|c| c.0),
    }))
}

pub async fn get_decision(
    st: &AppState,
    caller: &Caller,
    project: ProjectId,
    event_id: Uuid,
) -> AppResult<Value> {
    let tenant = caller.require_project_role(project, ProjectRole::Viewer).await?;
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let d = repo::load_decision_out(&mut tx, project, event_id).await?;
    tx.commit().await?;
    let d = d.ok_or_else(|| AppError::not_found("decision not found"))?;
    Ok(serde_json::to_value(d).unwrap_or(Value::Null))
}

#[derive(Debug, Clone, Default, Deserialize, utoipa::IntoParams)]
pub struct CustomerFilter {
    pub q: Option<String>,
    pub risk_label: Option<String>,
}

pub async fn list_customers(
    st: &AppState,
    caller: &Caller,
    project: ProjectId,
    f: &CustomerFilter,
    page: &PageParams,
) -> AppResult<Page<Value>> {
    let tenant = caller.require_project_role(project, ProjectRole::Viewer).await?;
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let q = f.q.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let rows: Vec<(Value, i64)> = sqlx::query_as(
        "SELECT to_jsonb(t) AS item, count(*) OVER () FROM ( \
            SELECT id, external_id, full_name, email, phone, risk_label, status, segment, kyc_level, \
                   registered_at, created_at \
            FROM core.customers \
            WHERE project_id = $1 AND ($2::text IS NULL OR risk_label = $2) \
              AND ($3::text IS NULL OR external_id = $3 OR external_id LIKE $3 || '%' \
                   OR full_name ILIKE '%' || $3 || '%' OR email_normalized = lower($3)) \
            ORDER BY created_at DESC, id) t LIMIT $4 OFFSET $5",
    )
    .bind(project.as_uuid())
    .bind(&f.risk_label)
    .bind(q)
    .bind(page.limit())
    .bind(page.offset())
    .fetch_all(&mut **tx)
    .await?;
    tx.commit().await?;
    let mut p = collect_page(rows, page);
    if effective_role(caller, project) == ProjectRole::Viewer {
        p.items.iter_mut().for_each(mask_pii);
    }
    Ok(p)
}

pub async fn get_customer(st: &AppState, caller: &Caller, project: ProjectId, id: Uuid) -> AppResult<Value> {
    let tenant = caller.require_project_role(project, ProjectRole::Viewer).await?;
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let row: Option<(Value, Value)> = sqlx::query_as(
        "SELECT to_jsonb(c) - 'tenant_id', jsonb_build_object( \
            'events_30d', (SELECT count(*) FROM core.events e WHERE e.customer_id = c.id \
                            AND e.occurred_at > now() - interval '30 days'), \
            'declines_30d', (SELECT count(*) FROM core.decisions d JOIN core.events e ON e.id = d.event_id \
                            WHERE e.customer_id = c.id AND d.decision = 'decline' \
                            AND d.created_at > now() - interval '30 days'), \
            'avg_score_30d', (SELECT avg(d.final_score)::float8 FROM core.decisions d \
                            JOIN core.events e ON e.id = d.event_id WHERE e.customer_id = c.id \
                            AND d.created_at > now() - interval '30 days'), \
            'open_case_id', (SELECT id FROM core.cases WHERE customer_id = c.id \
                            AND status IN ('open','in_review') LIMIT 1)) \
         FROM core.customers c WHERE c.project_id = $1 AND c.id = $2",
    )
    .bind(project.as_uuid())
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?;
    tx.commit().await?;
    let (mut customer, stats) = row.ok_or_else(|| AppError::not_found("customer not found"))?;
    if effective_role(caller, project) == ProjectRole::Viewer {
        mask_pii(&mut customer);
    }
    if let Some(m) = customer.as_object_mut() {
        m.insert("stats".into(), stats);
    }
    Ok(customer)
}

#[derive(Debug, Clone, Default, Deserialize, utoipa::IntoParams)]
pub struct AuditFilter {
    pub actor: Option<String>,
    pub action: Option<String>,
    pub subject_type: Option<String>,
    pub subject_id: Option<String>,
    #[serde(default, deserialize_with = "platform::query_time::opt_from")]
    pub from: Option<DateTime<Utc>>,
    #[serde(default, deserialize_with = "platform::query_time::opt_to")]
    pub to: Option<DateTime<Utc>>,
}

/// `core.audit_log` has no RLS (platform table): the tenant/project filter is mandatory here.
pub async fn list_audit(
    st: &AppState,
    tenant: TenantId,
    project: Option<ProjectId>,
    f: &AuditFilter,
    page: &PageParams,
) -> AppResult<Page<Value>> {
    let rows: Vec<(Value, i64)> = sqlx::query_as(
        "SELECT to_jsonb(t) AS item, count(*) OVER () FROM ( \
            SELECT id, occurred_at, project_id, actor_type, actor_id, action, subject_type, subject_id, \
                   before, after, metadata, request_id \
            FROM core.audit_log \
            WHERE tenant_id = $1 AND ($2::uuid IS NULL OR project_id = $2) \
              AND ($3::text IS NULL OR actor_id = $3) AND ($4::text IS NULL OR action LIKE $4 || '%') \
              AND ($5::text IS NULL OR subject_type = $5) AND ($6::text IS NULL OR subject_id = $6) \
              AND ($7::timestamptz IS NULL OR occurred_at >= $7) AND ($8::timestamptz IS NULL OR occurred_at < $8) \
            ORDER BY id DESC) t LIMIT $9 OFFSET $10",
    )
    .bind(tenant.as_uuid())
    .bind(project.map(|p| p.as_uuid()))
    .bind(&f.actor)
    .bind(&f.action)
    .bind(&f.subject_type)
    .bind(&f.subject_id)
    .bind(f.from)
    .bind(f.to)
    .bind(page.limit())
    .bind(page.offset())
    .fetch_all(&st.pool)
    .await?;
    Ok(collect_page(rows, page))
}
