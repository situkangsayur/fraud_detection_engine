//! Postgres persistence for the scoring hot path (customers, events, features, decisions, cases).
//!
//! All functions take a [`TenantTx`] (RLS context already set) and filter by `project_id`
//! explicitly. Numeric / inet / char columns are bound as `float8` / `text` and cast in SQL
//! (`$n::float8::numeric`, `$n::text::inet`) so the Rust side only deals with f64 and String.

use chrono::{DateTime, Utc};
use contracts::events::{CanonicalEventIn, DecisionOut, EngineScores, MlSummary};
use contracts::graph::GraphMetrics;
use contracts::scoring::{Reason, RuleResultTrace};
use contracts::Decision;
use platform::db::TenantTx;
use platform::error::AppResult;
use platform::{ProjectId, TenantId};
use serde_json::{Map, Value};
use sqlx::Row;
use uuid::Uuid;

use crate::domain::normalize::NormalizedEvent;

/// Customer row as needed by the pipeline.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CustomerRow {
    pub id: Uuid,
    pub external_id: String,
    pub risk_label: String,
    pub status: String,
    pub kyc_level: Option<i16>,
    pub segment: Option<String>,
    pub registered_at: Option<DateTime<Utc>>,
    pub attributes: Value,
    pub email_normalized: Option<String>,
    pub phone_normalized: Option<String>,
    pub created_at: DateTime<Utc>,
}

const CUSTOMER_COLS: &str =
    "id, external_id, risk_label, status, kyc_level, segment, registered_at, attributes, \
                             email_normalized, phone_normalized, created_at";

pub async fn upsert_customer(
    tx: &mut TenantTx<'_>,
    tenant: TenantId,
    project: ProjectId,
    n: &NormalizedEvent,
) -> AppResult<CustomerRow> {
    let c = &n.event.customer;
    let attrs = c.attributes.clone().map(Value::Object);
    let sql = format!(
        "INSERT INTO core.customers (tenant_id, project_id, external_id, full_name, email, email_normalized, \
                                     phone, phone_normalized, kyc_level, segment, registered_at, attributes) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11, COALESCE($12::jsonb, '{{}}'::jsonb)) \
         ON CONFLICT (project_id, external_id) DO UPDATE SET \
            full_name = COALESCE(EXCLUDED.full_name, core.customers.full_name), \
            email = COALESCE(EXCLUDED.email, core.customers.email), \
            email_normalized = COALESCE(EXCLUDED.email_normalized, core.customers.email_normalized), \
            phone = COALESCE(EXCLUDED.phone, core.customers.phone), \
            phone_normalized = COALESCE(EXCLUDED.phone_normalized, core.customers.phone_normalized), \
            kyc_level = COALESCE(EXCLUDED.kyc_level, core.customers.kyc_level), \
            segment = COALESCE(EXCLUDED.segment, core.customers.segment), \
            registered_at = COALESCE(core.customers.registered_at, EXCLUDED.registered_at), \
            attributes = core.customers.attributes || EXCLUDED.attributes \
         RETURNING {CUSTOMER_COLS}"
    );
    let row = sqlx::query_as::<_, CustomerRow>(&sql)
        .bind(tenant.as_uuid())
        .bind(project.as_uuid())
        .bind(&c.external_id)
        .bind(&c.full_name)
        .bind(&c.email)
        .bind(&n.email_normalized)
        .bind(&c.phone)
        .bind(&n.phone_normalized)
        .bind(c.kyc_level)
        .bind(&c.segment)
        .bind(c.registered_at)
        .bind(attrs)
        .fetch_one(&mut ***tx)
        .await?;
    Ok(row)
}

pub async fn find_customer_by_external(
    tx: &mut TenantTx<'_>,
    project: ProjectId,
    external_id: &str,
) -> AppResult<Option<CustomerRow>> {
    let sql =
        format!("SELECT {CUSTOMER_COLS} FROM core.customers WHERE project_id = $1 AND external_id = $2");
    Ok(sqlx::query_as::<_, CustomerRow>(&sql)
        .bind(project.as_uuid())
        .bind(external_id)
        .fetch_optional(&mut ***tx)
        .await?)
}

pub async fn find_customer(
    tx: &mut TenantTx<'_>,
    project: ProjectId,
    id: Uuid,
) -> AppResult<Option<CustomerRow>> {
    let sql = format!("SELECT {CUSTOMER_COLS} FROM core.customers WHERE project_id = $1 AND id = $2");
    Ok(sqlx::query_as::<_, CustomerRow>(&sql)
        .bind(project.as_uuid())
        .bind(id)
        .fetch_optional(&mut ***tx)
        .await?)
}

/// Inserts the event. Returns `None` when `(data_source_id, external_id)` already exists (dedupe).
pub async fn insert_event(
    tx: &mut TenantTx<'_>,
    tenant: TenantId,
    project: ProjectId,
    source_id: Uuid,
    customer_id: Uuid,
    ev: &CanonicalEventIn,
    load_only: bool,
) -> AppResult<Option<Uuid>> {
    let payload = Value::Object(ev.payload.clone().unwrap_or_default());
    let row = sqlx::query(
        "INSERT INTO core.events (tenant_id, project_id, data_source_id, external_id, event_type, customer_id, \
            occurred_at, channel, status, amount, currency, merchant_id, merchant_category, payment_method, \
            instrument_fingerprint, card_bin, card_last4, issuer_country, recipient_fingerprint, device_id, \
            ip_address, user_agent, geo_country, geo_city, latitude, longitude, promo_code, discount_amount, \
            cashback_amount, ref_transaction_id, shipping_address, billing_address, account_change_type, \
            login_success, api_client_id, payload, load_only) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10::float8::numeric,$11::text,$12,$13,$14,$15,$16,$17,$18::text,$19,$20, \
            $21::text::inet,$22,$23::text,$24,$25,$26,$27,$28::float8::numeric,$29::float8::numeric,$30,$31,$32,$33, \
            $34,$35,$36,$37) \
         ON CONFLICT (data_source_id, external_id) DO NOTHING \
         RETURNING id",
    )
    .bind(tenant.as_uuid())
    .bind(project.as_uuid())
    .bind(source_id)
    .bind(&ev.external_id)
    .bind(&ev.event_type)
    .bind(customer_id)
    .bind(ev.occurred_at)
    .bind(&ev.channel)
    .bind(&ev.status)
    .bind(ev.amount)
    .bind(&ev.currency)
    .bind(&ev.merchant_id)
    .bind(&ev.merchant_category)
    .bind(&ev.payment_method)
    .bind(&ev.instrument_fingerprint)
    .bind(&ev.card_bin)
    .bind(&ev.card_last4)
    .bind(&ev.issuer_country)
    .bind(&ev.recipient_fingerprint)
    .bind(&ev.device_id)
    .bind(&ev.ip_address)
    .bind(&ev.user_agent)
    .bind(&ev.geo_country)
    .bind(&ev.geo_city)
    .bind(ev.latitude)
    .bind(ev.longitude)
    .bind(&ev.promo_code)
    .bind(ev.discount_amount)
    .bind(ev.cashback_amount)
    .bind(&ev.ref_transaction_id)
    .bind(&ev.shipping_address)
    .bind(&ev.billing_address)
    .bind(&ev.account_change_type)
    .bind(ev.login_success)
    .bind(&ev.api_client_id)
    .bind(payload)
    .bind(load_only)
    .fetch_optional(&mut ***tx)
    .await?;
    Ok(row.map(|r| r.get::<Uuid, _>("id")))
}

pub async fn find_event_id(
    tx: &mut TenantTx<'_>,
    project: ProjectId,
    source_id: Uuid,
    external_id: &str,
) -> AppResult<Option<Uuid>> {
    let row: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM core.events WHERE project_id = $1 AND data_source_id = $2 AND external_id = $3",
    )
    .bind(project.as_uuid())
    .bind(source_id)
    .bind(external_id)
    .fetch_optional(&mut ***tx)
    .await?;
    Ok(row.map(|r| r.0))
}

#[allow(clippy::too_many_arguments)]
pub async fn insert_label(
    tx: &mut TenantTx<'_>,
    tenant: TenantId,
    project: ProjectId,
    subject_type: &str,
    subject_id: Uuid,
    label: &str,
    fraud_type: Option<&str>,
    source: &str,
    notes: Option<&str>,
    created_by: Option<Uuid>,
) -> AppResult<Uuid> {
    let (id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO core.labels (tenant_id, project_id, subject_type, subject_id, label, fraud_type, source, notes, created_by) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING id",
    )
    .bind(tenant.as_uuid())
    .bind(project.as_uuid())
    .bind(subject_type)
    .bind(subject_id)
    .bind(label)
    .bind(fraud_type)
    .bind(source)
    .bind(notes)
    .bind(created_by)
    .fetch_one(&mut ***tx)
    .await?;
    Ok(id)
}

pub async fn upsert_features(
    tx: &mut TenantTx<'_>,
    tenant: TenantId,
    project: ProjectId,
    event_id: Uuid,
    features: &Map<String, Value>,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO core.event_features (event_id, tenant_id, project_id, feature_set_version, features) \
         VALUES ($1,$2,$3,$4,$5) \
         ON CONFLICT (event_id) DO UPDATE SET features = EXCLUDED.features, \
            feature_set_version = EXCLUDED.feature_set_version, computed_at = now()",
    )
    .bind(event_id)
    .bind(tenant.as_uuid())
    .bind(project.as_uuid())
    .bind(contracts::catalog::FEATURE_SET_VERSION)
    .bind(Value::Object(features.clone()))
    .execute(&mut ***tx)
    .await?;
    Ok(())
}

/// Decision payload to persist.
#[derive(Debug, Clone)]
pub struct DecisionRecord<'a> {
    pub event_id: Uuid,
    pub decision: Decision,
    pub final_score: f64,
    pub engine_scores: &'a EngineScores,
    pub ml: &'a MlSummary,
    pub graph: Option<&'a GraphMetrics>,
    pub reasons: &'a [Reason],
    pub rule_results: &'a [RuleResultTrace],
    pub degraded: &'a [String],
    pub latency_ms: u64,
}

/// Inserts (or replaces on rescore) the decision; returns its id.
pub async fn upsert_decision(
    tx: &mut TenantTx<'_>,
    tenant: TenantId,
    project: ProjectId,
    d: &DecisionRecord<'_>,
) -> AppResult<Uuid> {
    let (id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO core.decisions (tenant_id, project_id, event_id, decision, final_score, engine_scores, ml, graph, \
            reasons, rule_results, degraded, latency_ms) \
         VALUES ($1,$2,$3,$4,$5::float8,$6,$7,$8,$9,$10,$11,$12) \
         ON CONFLICT (event_id) DO UPDATE SET decision = EXCLUDED.decision, final_score = EXCLUDED.final_score, \
            engine_scores = EXCLUDED.engine_scores, ml = EXCLUDED.ml, graph = EXCLUDED.graph, \
            reasons = EXCLUDED.reasons, rule_results = EXCLUDED.rule_results, degraded = EXCLUDED.degraded, \
            latency_ms = EXCLUDED.latency_ms, created_at = now() \
         RETURNING id",
    )
    .bind(tenant.as_uuid())
    .bind(project.as_uuid())
    .bind(d.event_id)
    .bind(d.decision.as_str())
    .bind(d.final_score)
    .bind(json(d.engine_scores))
    .bind(json(d.ml))
    .bind(d.graph.map(json).unwrap_or_else(|| Value::Object(Map::new())))
    .bind(json(d.reasons))
    .bind(json(d.rule_results))
    .bind(d.degraded)
    .bind(i32::try_from(d.latency_ms).unwrap_or(i32::MAX))
    .fetch_one(&mut ***tx)
    .await?;
    Ok(id)
}

/// Opens a case or attaches the event to the customer's open case. Returns the case id.
#[allow(clippy::too_many_arguments)]
pub async fn open_or_attach_case(
    tx: &mut TenantTx<'_>,
    tenant: TenantId,
    project: ProjectId,
    customer_id: Uuid,
    event_id: Uuid,
    decision_id: Uuid,
    priority: i16,
) -> AppResult<Uuid> {
    let (id,): (Uuid,) = sqlx::query_as(
        "INSERT INTO core.cases (tenant_id, project_id, customer_id, event_id, decision_id, priority, event_ids) \
         VALUES ($1,$2,$3,$4,$5,$6, ARRAY[$4]::uuid[]) \
         ON CONFLICT (customer_id) WHERE status IN ('open', 'in_review') DO UPDATE SET \
            event_ids = CASE WHEN $4 = ANY(core.cases.event_ids) THEN core.cases.event_ids \
                             ELSE array_append(core.cases.event_ids, $4) END, \
            priority = LEAST(core.cases.priority, EXCLUDED.priority), updated_at = now() \
         RETURNING id",
    )
    .bind(tenant.as_uuid())
    .bind(project.as_uuid())
    .bind(customer_id)
    .bind(event_id)
    .bind(decision_id)
    .bind(priority)
    .fetch_one(&mut ***tx)
    .await?;
    Ok(id)
}

pub async fn mark_needs_rescore(tx: &mut TenantTx<'_>, event_id: Uuid, needs: bool) -> AppResult<()> {
    sqlx::query("UPDATE core.events SET needs_rescore = $2 WHERE id = $1")
        .bind(event_id)
        .bind(needs)
        .execute(&mut ***tx)
        .await?;
    Ok(())
}

/// Loads a stored decision as `DecisionOut` (dedupe replies, GET /decisions).
pub async fn load_decision_out(
    tx: &mut TenantTx<'_>,
    project: ProjectId,
    event_id: Uuid,
) -> AppResult<Option<DecisionOut>> {
    let row = sqlx::query(
        "SELECT d.decision, d.final_score::float8 AS final_score, d.engine_scores, d.ml, d.graph, d.reasons, \
                d.rule_results, d.degraded, d.latency_ms, e.external_id, \
                (SELECT c.id FROM core.cases c WHERE c.project_id = d.project_id AND d.event_id = ANY(c.event_ids) \
                 ORDER BY c.created_at DESC LIMIT 1) AS case_id \
         FROM core.decisions d JOIN core.events e ON e.id = d.event_id \
         WHERE d.project_id = $1 AND d.event_id = $2",
    )
    .bind(project.as_uuid())
    .bind(event_id)
    .fetch_optional(&mut ***tx)
    .await?;
    let Some(r) = row else { return Ok(None) };
    let decision: String = r.get("decision");
    let graph: Value = r.get("graph");
    let latency: i32 = r.get("latency_ms");
    Ok(Some(DecisionOut {
        event_id,
        external_id: r.get("external_id"),
        project_id: project.as_uuid(),
        decision: match decision.as_str() {
            "decline" => Decision::Decline,
            "review" => Decision::Review,
            _ => Decision::Approve,
        },
        final_score: r.get("final_score"),
        engine_scores: serde_json::from_value(r.get("engine_scores")).unwrap_or_default(),
        reasons: serde_json::from_value(r.get("reasons")).unwrap_or_default(),
        rule_results: serde_json::from_value(r.get("rule_results")).unwrap_or_default(),
        ml: serde_json::from_value(r.get("ml")).unwrap_or_default(),
        graph: if graph.as_object().is_some_and(|m| m.is_empty()) {
            None
        } else {
            serde_json::from_value(graph).ok()
        },
        degraded: r.get("degraded"),
        case_id: r.get("case_id"),
        latency_ms: u64::try_from(latency).unwrap_or(0),
        persisted: true,
    }))
}

fn json<T: serde::Serialize + ?Sized>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}
