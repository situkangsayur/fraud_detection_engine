//! Case management and labels (the feedback loop for ML and the graph).
//!
//! Customer label changes are mirrored to graph-service (`risk_label` drives graph distance to
//! fraud). graph-service answers 404 when the customer has no graph node yet: that is fine, the
//! next `links` call carries the current label anyway.

use platform::audit::{self, AuditEntry};
use platform::auth::{Caller, ProjectRole};
use platform::db::TenantTx;
use platform::error::{AppError, AppResult, FieldError};
use platform::http::CallCtx;
use platform::pagination::{Page, PageParams};
use platform::{ProjectId, TenantId};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::adapters::repo;
use crate::state::AppState;

use super::util::{collect_page, effective_role, mask_pii};

pub const LABEL_SOURCES: &[&str] = &["analyst", "chargeback", "customer_report", "dataset", "system"];

#[derive(Debug, Clone, Default, Deserialize, utoipa::IntoParams)]
pub struct CaseFilter {
    pub status: Option<String>,
    pub assigned_to: Option<Uuid>,
    pub typology: Option<String>,
    pub priority: Option<i16>,
    pub customer_id: Option<Uuid>,
}

pub async fn list(
    st: &AppState,
    caller: &Caller,
    project: ProjectId,
    f: &CaseFilter,
    page: &PageParams,
) -> AppResult<Page<Value>> {
    let tenant = caller.require_project_role(project, ProjectRole::Viewer).await?;
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let rows: Vec<(Value, i64)> = sqlx::query_as(
        "SELECT to_jsonb(t) AS item, count(*) OVER () FROM ( \
            SELECT k.id, k.status, k.priority, k.typologies, k.assigned_to, k.customer_id, \
                   c.external_id AS customer_external_id, c.risk_label, k.event_id, \
                   cardinality(k.event_ids) AS event_count, d.decision, d.final_score::float8 AS final_score, \
                   k.created_at, k.updated_at, k.resolved_at \
            FROM core.cases k JOIN core.customers c ON c.id = k.customer_id \
            LEFT JOIN core.decisions d ON d.id = k.decision_id \
            WHERE k.project_id = $1 AND ($2::text IS NULL OR k.status = $2) \
              AND ($3::uuid IS NULL OR k.assigned_to = $3) AND ($4::text IS NULL OR $4 = ANY(k.typologies)) \
              AND ($5::int2 IS NULL OR k.priority = $5) AND ($6::uuid IS NULL OR k.customer_id = $6) \
            ORDER BY (k.status IN ('open','in_review')) DESC, k.priority, k.created_at DESC) t \
         LIMIT $7 OFFSET $8",
    )
    .bind(project.as_uuid())
    .bind(&f.status)
    .bind(f.assigned_to)
    .bind(&f.typology)
    .bind(f.priority)
    .bind(f.customer_id)
    .bind(page.limit())
    .bind(page.offset())
    .fetch_all(&mut **tx)
    .await?;
    tx.commit().await?;
    Ok(collect_page(rows, page))
}

pub async fn get(st: &AppState, caller: &Caller, project: ProjectId, id: Uuid) -> AppResult<Value> {
    let tenant = caller.require_project_role(project, ProjectRole::Viewer).await?;
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let row: Option<(Value, Value, Option<Value>)> = sqlx::query_as(
        "SELECT to_jsonb(k) - 'tenant_id', \
                jsonb_build_object('id', c.id, 'external_id', c.external_id, 'full_name', c.full_name, \
                                   'email', c.email, 'phone', c.phone, 'risk_label', c.risk_label, \
                                   'status', c.status, 'segment', c.segment), \
                (SELECT to_jsonb(e.*) - ARRAY['tenant_id','payload'] FROM core.events e WHERE e.id = k.event_id) \
         FROM core.cases k JOIN core.customers c ON c.id = k.customer_id \
         WHERE k.project_id = $1 AND k.id = $2",
    )
    .bind(project.as_uuid())
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?;
    let (case, mut customer, mut event) = row.ok_or_else(|| AppError::not_found("case not found"))?;
    let decision = match case
        .get("event_id")
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<Uuid>().ok())
    {
        Some(eid) => repo::load_decision_out(&mut tx, project, eid).await?,
        None => None,
    };
    tx.commit().await?;
    if effective_role(caller, project) == ProjectRole::Viewer {
        mask_pii(&mut customer);
        if let Some(e) = event.as_mut() {
            mask_pii(e);
        }
    }
    let graph = decision.as_ref().and_then(|d| d.graph.clone());
    Ok(json!({ "case": case, "customer": customer, "event": event, "decision": decision, "graph": graph }))
}

#[derive(Debug, Clone, Default, Deserialize, utoipa::ToSchema)]
pub struct CasePatch {
    pub status: Option<String>,
    pub assigned_to: Option<Uuid>,
    pub priority: Option<i16>,
    pub note: Option<String>,
}

pub async fn patch(
    st: &AppState,
    caller: &Caller,
    project: ProjectId,
    id: Uuid,
    p: &CasePatch,
) -> AppResult<Value> {
    let tenant = caller.require_project_role(project, ProjectRole::Analyst).await?;
    if let Some(s) = &p.status {
        if s != "open" && s != "in_review" {
            return Err(AppError::field(
                "status",
                "open | in_review (use /resolve to close)",
            ));
        }
    }
    if p.priority.is_some_and(|x| !(1..=5).contains(&x)) {
        return Err(AppError::field("priority", "1..5"));
    }
    if p.note.as_ref().is_some_and(|n| n.len() > 5000) {
        return Err(AppError::field("note", "max 5000 characters"));
    }
    let note = p.note.as_ref().map(|n| {
        json!([{ "at": chrono::Utc::now(), "by": caller.actor_user_id().map(|u| u.as_uuid()), "text": n }])
    });
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let row: Option<(Value,)> = sqlx::query_as(
        "UPDATE core.cases SET status = COALESCE($3, status), assigned_to = COALESCE($4, assigned_to), \
            priority = COALESCE($5, priority), notes = CASE WHEN $6::jsonb IS NULL THEN notes ELSE notes || $6 END \
         WHERE project_id = $1 AND id = $2 AND status IN ('open', 'in_review') \
         RETURNING to_jsonb(core.cases.*) - 'tenant_id'",
    )
    .bind(project.as_uuid())
    .bind(id)
    .bind(&p.status)
    .bind(p.assigned_to)
    .bind(p.priority)
    .bind(note)
    .fetch_optional(&mut **tx)
    .await?;
    let out = row
        .map(|r| r.0)
        .ok_or_else(|| AppError::not_found("open case not found"))?;
    audit::record(
        &mut **tx,
        &AuditEntry::by(caller, "case.update")
            .scope(tenant, Some(project))
            .subject("case", id)
            .after(
                json!({ "status": p.status, "assigned_to": p.assigned_to, "priority": p.priority,
                           "note_added": p.note.is_some() }),
            ),
    )
    .await?;
    tx.commit().await?;
    Ok(out)
}

fn validate_label(label: &str, fraud_type: Option<&str>) -> Vec<FieldError> {
    let mut e = Vec::new();
    if label != "fraud" && label != "legit" {
        e.push(FieldError::new("label", "fraud | legit"));
    }
    if let Some(t) = fraud_type {
        if contracts::Typology::parse(t).is_none() {
            e.push(FieldError::new("fraud_type", "unknown typology"));
        }
    }
    e
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
pub struct ResolveIn {
    pub label: String,
    pub fraud_type: Option<String>,
    pub notes: Option<String>,
    #[serde(default)]
    pub apply_to_customer: bool,
}

pub async fn resolve(
    st: &AppState,
    caller: &Caller,
    project: ProjectId,
    id: Uuid,
    r: &ResolveIn,
) -> AppResult<Value> {
    let tenant = caller.require_project_role(project, ProjectRole::Analyst).await?;
    let errors = validate_label(&r.label, r.fraud_type.as_deref());
    if !errors.is_empty() {
        return Err(AppError::validation(errors));
    }
    let actor = caller.actor_user_id().map(|u| u.as_uuid());
    let status = if r.label == "fraud" {
        "resolved_fraud"
    } else {
        "resolved_legit"
    };
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let row: Option<(Uuid, Option<Uuid>, Vec<Uuid>)> = sqlx::query_as(
        "UPDATE core.cases SET status = $3, resolved_at = now(), \
            notes = CASE WHEN $4::text IS NULL THEN notes \
                         ELSE notes || jsonb_build_array(jsonb_build_object('at', now(), 'by', $5::uuid, 'text', $4)) END \
         WHERE project_id = $1 AND id = $2 AND status IN ('open', 'in_review') \
         RETURNING customer_id, event_id, event_ids",
    )
    .bind(project.as_uuid())
    .bind(id)
    .bind(status)
    .bind(&r.notes)
    .bind(actor)
    .fetch_optional(&mut **tx)
    .await?;
    let (customer_id, event_id, event_ids) = row.ok_or_else(|| AppError::not_found("open case not found"))?;
    let mut labelled = Vec::new();
    let mut events: Vec<Uuid> = event_ids;
    if let Some(e) = event_id {
        if !events.contains(&e) {
            events.push(e);
        }
    }
    for e in &events {
        repo::insert_label(
            &mut tx,
            tenant,
            project,
            "event",
            *e,
            &r.label,
            r.fraud_type.as_deref(),
            "analyst",
            r.notes.as_deref(),
            actor,
        )
        .await?;
        labelled.push(*e);
    }
    let customer_changed = if r.apply_to_customer {
        label_customer(
            &mut tx,
            tenant,
            project,
            customer_id,
            &r.label,
            r.fraud_type.as_deref(),
            "analyst",
            r.notes.as_deref(),
            actor,
        )
        .await?;
        true
    } else {
        false
    };
    audit::record(
        &mut **tx,
        &AuditEntry::by(caller, "case.resolve")
            .scope(tenant, Some(project))
            .subject("case", id)
            .after(
                json!({ "status": status, "label": r.label, "fraud_type": r.fraud_type,
                           "events_labelled": labelled.len(), "apply_to_customer": r.apply_to_customer }),
            ),
    )
    .await?;
    tx.commit().await?;
    if customer_changed {
        notify_graph(st, caller, tenant, project, customer_id, &r.label).await;
    }
    Ok(
        json!({ "id": id, "status": status, "labelled_events": labelled, "customer_label_updated": customer_changed }),
    )
}

#[allow(clippy::too_many_arguments)]
async fn label_customer(
    tx: &mut TenantTx<'_>,
    tenant: TenantId,
    project: ProjectId,
    customer_id: Uuid,
    label: &str,
    fraud_type: Option<&str>,
    source: &str,
    notes: Option<&str>,
    actor: Option<Uuid>,
) -> AppResult<()> {
    repo::insert_label(
        tx,
        tenant,
        project,
        "customer",
        customer_id,
        label,
        fraud_type,
        source,
        notes,
        actor,
    )
    .await?;
    sqlx::query("UPDATE core.customers SET risk_label = $3 WHERE project_id = $1 AND id = $2")
        .bind(project.as_uuid())
        .bind(customer_id)
        .bind(label)
        .execute(&mut ***tx)
        .await?;
    Ok(())
}

async fn notify_graph(
    st: &AppState,
    caller: &Caller,
    tenant: TenantId,
    project: ProjectId,
    customer: Uuid,
    label: &str,
) {
    let ctx = CallCtx::new(tenant, Some(project)).with_actor(caller.actor_user_id());
    match st.engines.graph.set_label(&ctx, project, customer, label).await {
        Ok(()) | Err(AppError::NotFound(_)) => {}
        Err(e) => {
            tracing::warn!(error = %e, %customer, "graph label update failed (next links call will carry it)")
        }
    }
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
pub struct LabelIn {
    pub subject_type: String,
    pub subject_id: Uuid,
    pub label: String,
    pub fraud_type: Option<String>,
    pub source: Option<String>,
    pub notes: Option<String>,
    /// For event labels: also label (and graph-flag) the event's customer.
    #[serde(default)]
    pub apply_to_customer: bool,
}

pub async fn create_label(
    st: &AppState,
    caller: &Caller,
    project: ProjectId,
    l: &LabelIn,
) -> AppResult<Value> {
    let tenant = caller.require_project_role(project, ProjectRole::Analyst).await?;
    let mut errors = validate_label(&l.label, l.fraud_type.as_deref());
    if l.subject_type != "event" && l.subject_type != "customer" {
        errors.push(FieldError::new("subject_type", "event | customer"));
    }
    let source = l.source.clone().unwrap_or_else(|| "analyst".into());
    if !LABEL_SOURCES.contains(&source.as_str()) {
        errors.push(FieldError::new("source", LABEL_SOURCES.join(" | ")));
    }
    if l.notes.as_ref().is_some_and(|n| n.len() > 5000) {
        errors.push(FieldError::new("notes", "max 5000 characters"));
    }
    if !errors.is_empty() {
        return Err(AppError::validation(errors));
    }
    let actor = caller.actor_user_id().map(|u| u.as_uuid());
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let customer_id: Uuid = if l.subject_type == "customer" {
        let found: Option<(Uuid,)> =
            sqlx::query_as("SELECT id FROM core.customers WHERE project_id = $1 AND id = $2")
                .bind(project.as_uuid())
                .bind(l.subject_id)
                .fetch_optional(&mut **tx)
                .await?;
        found
            .ok_or_else(|| AppError::field("subject_id", "customer not found in project"))?
            .0
    } else {
        let found: Option<(Uuid,)> =
            sqlx::query_as("SELECT customer_id FROM core.events WHERE project_id = $1 AND id = $2")
                .bind(project.as_uuid())
                .bind(l.subject_id)
                .fetch_optional(&mut **tx)
                .await?;
        found
            .ok_or_else(|| AppError::field("subject_id", "event not found in project"))?
            .0
    };
    let label_id = if l.subject_type == "customer" {
        label_customer(
            &mut tx,
            tenant,
            project,
            customer_id,
            &l.label,
            l.fraud_type.as_deref(),
            &source,
            l.notes.as_deref(),
            actor,
        )
        .await?;
        None
    } else {
        let id = repo::insert_label(
            &mut tx,
            tenant,
            project,
            "event",
            l.subject_id,
            &l.label,
            l.fraud_type.as_deref(),
            &source,
            l.notes.as_deref(),
            actor,
        )
        .await?;
        if l.apply_to_customer {
            label_customer(
                &mut tx,
                tenant,
                project,
                customer_id,
                &l.label,
                l.fraud_type.as_deref(),
                &source,
                l.notes.as_deref(),
                actor,
            )
            .await?;
        }
        Some(id)
    };
    let customer_changed = l.subject_type == "customer" || l.apply_to_customer;
    audit::record(
        &mut **tx,
        &AuditEntry::by(caller, "label.create")
            .scope(tenant, Some(project))
            .subject(l.subject_type.clone(), l.subject_id)
            .after(
                json!({ "label": l.label, "fraud_type": l.fraud_type, "source": source,
                           "apply_to_customer": l.apply_to_customer }),
            ),
    )
    .await?;
    tx.commit().await?;
    if customer_changed {
        notify_graph(st, caller, tenant, project, customer_id, &l.label).await;
    }
    Ok(
        json!({ "id": label_id, "subject_type": l.subject_type, "subject_id": l.subject_id, "label": l.label,
               "fraud_type": l.fraud_type, "source": source, "customer_id": customer_id,
               "customer_label_updated": customer_changed }),
    )
}

#[derive(Debug, Clone, Default, Deserialize, utoipa::IntoParams)]
pub struct LabelFilter {
    pub subject_type: Option<String>,
    pub subject_id: Option<Uuid>,
    pub label: Option<String>,
    pub source: Option<String>,
}

pub async fn list_labels(
    st: &AppState,
    caller: &Caller,
    project: ProjectId,
    f: &LabelFilter,
    page: &PageParams,
) -> AppResult<Page<Value>> {
    let tenant = caller.require_project_role(project, ProjectRole::Viewer).await?;
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let rows: Vec<(Value, i64)> = sqlx::query_as(
        "SELECT to_jsonb(t) AS item, count(*) OVER () FROM ( \
            SELECT id, subject_type, subject_id, label, fraud_type, source, notes, created_by, created_at \
            FROM core.labels WHERE project_id = $1 AND ($2::text IS NULL OR subject_type = $2) \
              AND ($3::uuid IS NULL OR subject_id = $3) AND ($4::text IS NULL OR label = $4) \
              AND ($5::text IS NULL OR source = $5) \
            ORDER BY created_at DESC, id) t LIMIT $6 OFFSET $7",
    )
    .bind(project.as_uuid())
    .bind(&f.subject_type)
    .bind(f.subject_id)
    .bind(&f.label)
    .bind(&f.source)
    .bind(page.limit())
    .bind(page.offset())
    .fetch_all(&mut **tx)
    .await?;
    tx.commit().await?;
    Ok(collect_page(rows, page))
}
