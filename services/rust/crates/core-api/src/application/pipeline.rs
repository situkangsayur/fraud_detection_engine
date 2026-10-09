//! Scoring pipeline orchestrator (architecture.md §3).
//!
//! ```text
//!  1 resolve source ─▶ 2 map + normalise ─▶ 3 upsert customer ─▶ 4 persist event (commit!)
//!  ─▶ 5 graph links ─▶ 6 features v1 ─▶ 7 graph metrics ─▶ ml predict ∥ ml score
//!  ─▶ 8 rule-service evaluate ─▶ 9 combine ─▶ 10 persist decision (+ case) ─▶ 11 respond
//! ```
//!
//! Key properties:
//! * **The event is committed before any engine is called**, so ingest never loses an event even if
//!   everything downstream is down (it is flagged `needs_rescore`).
//! * Every engine call has its own timeout from project settings. A failure removes that engine from
//!   the combination and records it in `decision.degraded`. A missing *model* (new project) is not
//!   a failure: it is simply absent and its weight is renormalised away.
//! * Graph metrics run **before** ML (not in parallel as first sketched in the architecture doc),
//!   so the model sees the same `graph_*` features at serving time as the ones stored for
//!   training. Supervised and unsupervised calls run in parallel with each other.
//! * `load_only` events stop after the features (with graph features) are stored.

use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use contracts::events::{CanonicalEventIn, DecisionOut, MlSummary, ModelRef};
use contracts::graph::{
    GraphCustomer, GraphEventLinks, GraphLinksRequest, GraphMetrics, GraphMetricsRequest,
};
use contracts::scoring::{EvaluateRequest, EvaluationContext, RuleResultTrace};
use contracts::Decision;
use platform::db::TenantTx;
use platform::error::{AppError, AppResult, FieldError};
use platform::http::CallCtx;
use platform::{pii, ProjectId, TenantId, UserId};
use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::adapters::{features_sql, repo};
use crate::application::context::{project_ctx, source_cfg};
use crate::application::ports::MlRequest;
use crate::domain::combine::{combine, CombineInput, CombineOutput};
use crate::domain::features::{add_graph_features, assemble, graph_context, CustomerFacts};
use crate::domain::mapping::{apply_mapping, canonical_from_json, MappedLabel, MappingCtx};
use crate::domain::normalize::{normalize_event, NormalizedEvent};
use crate::state::{AppState, ProjectCtx, SourceCfg};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestMode {
    Score,
    LoadOnly,
}

impl IngestMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "score" => Some(Self::Score),
            "load_only" => Some(Self::LoadOnly),
            _ => None,
        }
    }
}

/// Who/where an ingest runs.
#[derive(Debug, Clone)]
pub struct IngestCtx {
    pub tenant: TenantId,
    pub project: ProjectId,
    pub actor: Option<UserId>,
    pub request_id: Option<String>,
    pub job_id: Option<Uuid>,
}

impl IngestCtx {
    fn call(&self) -> CallCtx {
        let mut c = CallCtx::new(self.tenant, Some(self.project)).with_actor(self.actor);
        if let Some(r) = &self.request_id {
            c = c.with_request_id(r.clone());
        }
        c
    }
}

/// Result of one record.
#[derive(Debug, Clone)]
pub enum RecordOutcome {
    /// Scored now, or an earlier decision for a duplicate.
    Decided(Box<DecisionOut>),
    /// Stored without a decision (`load_only`, or a duplicate of a load-only event).
    Loaded { event_id: Uuid, external_id: String },
}

impl RecordOutcome {
    pub fn event_id(&self) -> Uuid {
        match self {
            Self::Decided(d) => d.event_id,
            Self::Loaded { event_id, .. } => *event_id,
        }
    }
    pub fn external_id(&self) -> &str {
        match self {
            Self::Decided(d) => &d.external_id,
            Self::Loaded { external_id, .. } => external_id,
        }
    }
}

#[derive(Debug)]
pub enum RecordError {
    /// Mapping / validation errors (dead-lettered; 422 for single records).
    Invalid(Vec<FieldError>),
    /// Infrastructure failure (DB); the batch should be retried.
    Failed(AppError),
}

impl From<AppError> for RecordError {
    fn from(e: AppError) -> Self {
        Self::Failed(e)
    }
}

impl RecordError {
    pub fn reason(&self) -> String {
        match self {
            Self::Invalid(errs) => errs
                .iter()
                .map(|e| format!("{}: {}", e.field, e.message))
                .collect::<Vec<_>>()
                .join("; "),
            Self::Failed(e) => e.to_string(),
        }
    }
}

fn stage(name: &'static str, started: Instant) {
    metrics::histogram!("scoring_stage_seconds", "stage" => name).record(started.elapsed().as_secs_f64());
}

// ---------------------------------------------------------------------------------------------
// Mapping
// ---------------------------------------------------------------------------------------------

/// Maps a raw record of `source` to a normalised canonical event (+ dataset label).
pub fn map_record(
    source: &SourceCfg,
    pepper: &platform::config::Secret,
    tz: chrono_tz::Tz,
    raw: &Value,
) -> Result<(NormalizedEvent, Option<MappedLabel>), Vec<FieldError>> {
    let (event, label) = match &source.mapping {
        Some(m) if source.kind != "internal" => {
            let ctx = MappingCtx {
                pepper: pepper.clone(),
                default_tz: tz,
                default_event_type: source.default_event_type.clone(),
            };
            let out = apply_mapping(m, raw, &ctx)?;
            (out.event, out.label)
        }
        // canonical source, or a source without an active mapping that receives canonical JSON
        _ => {
            let mut ev = canonical_from_json(raw)?;
            if ev.event_type.is_empty() {
                if let Some(d) = &source.default_event_type {
                    ev.event_type = d.clone();
                }
            }
            (ev, None)
        }
    };
    let n = normalize_event(event, pepper)?;
    Ok((n, label))
}

// ---------------------------------------------------------------------------------------------
// Ingest
// ---------------------------------------------------------------------------------------------

/// Processes one raw record end to end. Invalid records are dead-lettered.
pub async fn process_record(
    st: &AppState,
    ictx: &IngestCtx,
    source: &SourceCfg,
    raw: &Value,
    mode: IngestMode,
) -> Result<RecordOutcome, RecordError> {
    let started = Instant::now();
    let pctx = project_ctx(st, ictx.tenant, ictx.project).await?;
    if pctx.status != "active" {
        return Err(RecordError::Failed(AppError::Conflict(
            "project is archived".into(),
        )));
    }
    let pepper = pii::tenant_pepper(&st.cfg.pii_pepper, ictx.tenant);
    let mapped = map_record(source, &pepper, pctx.timezone, raw);
    stage("mapping", started);
    let (norm, label) = match mapped {
        Ok(v) => v,
        Err(errs) => {
            dead_letter(st, ictx, source.id, raw, &errs).await;
            metrics::counter!("ingest_rejected_total").increment(1);
            return Err(RecordError::Invalid(errs));
        }
    };
    ingest_normalized(st, ictx, &pctx, source.id, norm, label, mode, started)
        .await
        .map_err(RecordError::Failed)
}

async fn dead_letter(st: &AppState, ictx: &IngestCtx, source_id: Uuid, raw: &Value, errs: &[FieldError]) {
    let reason = errs
        .iter()
        .map(|e| format!("{}: {}", e.field, e.message))
        .collect::<Vec<_>>()
        .join("; ");
    let res: AppResult<()> = async {
        let mut tx = TenantTx::begin(&st.pool, ictx.tenant).await?;
        sqlx::query(
            "INSERT INTO core.ingest_errors (tenant_id, project_id, data_source_id, job_id, record, reason) \
             VALUES ($1,$2,$3,$4,$5,$6)",
        )
        .bind(ictx.tenant.as_uuid())
        .bind(ictx.project.as_uuid())
        .bind(source_id)
        .bind(ictx.job_id)
        .bind(redact_record(raw))
        .bind(reason)
        .execute(&mut **tx)
        .await?;
        tx.commit().await
    }
    .await;
    if let Err(e) = res {
        tracing::warn!(error = %e, "failed to write ingest dead-letter row");
    }
}

/// Dead-letter rows keep the record for debugging but never raw card/account numbers:
/// every string that looks like a PAN (Luhn-valid 12-19 digits) is masked.
fn redact_record(v: &Value) -> Value {
    match v {
        Value::String(s) => {
            let digits = pii::digits_only(s);
            if (12..=19).contains(&digits.len()) && pii::luhn_valid(s) {
                Value::String(pii::mask_pan(
                    pii::pan_bin(s, 6).as_deref(),
                    pii::pan_last4(s).as_deref(),
                ))
            } else {
                v.clone()
            }
        }
        Value::Array(a) => Value::Array(a.iter().map(redact_record).collect()),
        Value::Object(m) => Value::Object(m.iter().map(|(k, v)| (k.clone(), redact_record(v))).collect()),
        other => other.clone(),
    }
}

#[allow(clippy::too_many_arguments)]
async fn ingest_normalized(
    st: &AppState,
    ictx: &IngestCtx,
    pctx: &ProjectCtx,
    source_id: Uuid,
    norm: NormalizedEvent,
    label: Option<MappedLabel>,
    mode: IngestMode,
    started: Instant,
) -> AppResult<RecordOutcome> {
    let (tenant, project) = (ictx.tenant, ictx.project);

    // --- 3/4: customer + event in one transaction, committed before any engine call
    let t = Instant::now();
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let customer = repo::upsert_customer(&mut tx, tenant, project, &norm).await?;
    let inserted = repo::insert_event(
        &mut tx,
        tenant,
        project,
        source_id,
        customer.id,
        &norm.event,
        mode == IngestMode::LoadOnly,
    )
    .await?;
    let Some(event_id) = inserted else {
        tx.rollback().await?;
        metrics::counter!("ingest_duplicates_total").increment(1);
        return existing_outcome(st, tenant, project, source_id, &norm.event.external_id).await;
    };
    if let Some(l) = &label {
        repo::insert_label(
            &mut tx,
            tenant,
            project,
            "event",
            event_id,
            if l.fraud { "fraud" } else { "legit" },
            l.fraud_type.as_deref(),
            "dataset",
            None,
            None,
        )
        .await?;
    }
    if let Some(payload) = &norm.event.payload {
        register_source_paths(st, &mut tx, tenant, project, source_id, payload).await?;
    }
    tx.commit().await?;
    stage("persist_event", t);

    let call = ictx.call();
    let mut degraded: Vec<String> = Vec::new();

    // --- 5: graph entity resolution
    let t = Instant::now();
    let links = GraphLinksRequest {
        customer: GraphCustomer {
            id: customer.id,
            external_id: customer.external_id.clone(),
            risk_label: customer.risk_label.clone(),
            email: customer.email_normalized.clone(),
            phone: customer.phone_normalized.clone(),
        },
        event: GraphEventLinks {
            id: event_id,
            occurred_at: norm.event.occurred_at,
            device_id: norm.event.device_id.clone(),
            ip_address: norm.event.ip_address.clone(),
            instrument_fingerprint: norm.event.instrument_fingerprint.clone(),
            card_bin: norm.event.card_bin.clone(),
            card_last4: norm.event.card_last4.clone(),
            recipient_fingerprint: norm.event.recipient_fingerprint.clone(),
            shipping_address: norm.event.shipping_address.clone(),
            billing_address: norm.event.billing_address.clone(),
            ref_transaction_id: norm.event.ref_transaction_id.clone(),
            api_client_id: norm.event.api_client_id.clone(),
        },
    };
    let links_timeout = Duration::from_millis(pctx.settings.timeouts.graph_ms.saturating_mul(3));
    let graph_linked = match st
        .engines
        .graph
        .links(&call, project, &links, links_timeout)
        .await
    {
        Ok(_) => true,
        Err(e) => {
            tracing::warn!(error = %e, "graph links failed; graph engine degraded");
            degraded.push("graph".into());
            false
        }
    };
    stage("graph_links", t);

    // --- 6: features
    let t = Instant::now();
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let raw = features_sql::fetch(&mut tx, project, event_id, customer.id, &norm.event).await?;
    tx.commit().await?;
    let features = assemble(
        &norm.event,
        &raw,
        &CustomerFacts {
            registered_at: customer.registered_at,
        },
        pctx.timezone,
    );
    stage("features", t);

    let source_map = norm.event.payload.clone().unwrap_or_default();
    let input = ScoreInput {
        pctx,
        call,
        event_id,
        customer_id: customer.id,
        event_type: norm.event.event_type.clone(),
        occurred_at: norm.event.occurred_at,
        event_ctx: event_context(&norm.event, customer.id),
        source: source_map,
        customer_ctx: customer_context(&customer, &features),
        features,
        graph_linked,
        dry_run: false,
        degraded,
    };

    if mode == IngestMode::LoadOnly {
        let (features, _graph) = graph_stage(st, &input).await;
        let mut tx = TenantTx::begin(&st.pool, tenant).await?;
        repo::upsert_features(&mut tx, tenant, project, event_id, &features.0).await?;
        tx.commit().await?;
        metrics::counter!("ingest_loaded_total").increment(1);
        return Ok(RecordOutcome::Loaded {
            event_id,
            external_id: norm.event.external_id,
        });
    }

    let scored = score_stages(st, input).await;
    let out = persist_scored(
        st,
        tenant,
        project,
        customer.id,
        event_id,
        &norm.event.external_id,
        &scored,
        started,
    )
    .await?;
    Ok(RecordOutcome::Decided(Box::new(out)))
}

async fn existing_outcome(
    st: &AppState,
    tenant: TenantId,
    project: ProjectId,
    source_id: Uuid,
    external_id: &str,
) -> AppResult<RecordOutcome> {
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let event_id = repo::find_event_id(&mut tx, project, source_id, external_id)
        .await?
        .ok_or_else(|| AppError::Conflict("duplicate event could not be loaded".into()))?;
    let decision = repo::load_decision_out(&mut tx, project, event_id).await?;
    tx.commit().await?;
    Ok(match decision {
        Some(d) => RecordOutcome::Decided(Box::new(d)),
        None => RecordOutcome::Loaded {
            event_id,
            external_id: external_id.to_string(),
        },
    })
}

/// Registers unseen `source.*` leaf paths in `core.field_catalog` (velocity disabled by default).
async fn register_source_paths(
    st: &AppState,
    tx: &mut TenantTx<'_>,
    tenant: TenantId,
    project: ProjectId,
    source_id: Uuid,
    payload: &Map<String, Value>,
) -> AppResult<()> {
    let mut leaves = Vec::new();
    flatten("source", &Value::Object(payload.clone()), 0, &mut leaves);
    let mut paths = Vec::new();
    let mut types = Vec::new();
    for (path, ty) in leaves {
        let key = (project.as_uuid(), path.clone());
        if st.caches.known_paths.get(&key).await.is_none() {
            paths.push(path);
            types.push(ty);
        }
    }
    if paths.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO core.field_catalog (tenant_id, project_id, path, data_type, data_source_id) \
         SELECT $1, $2, p, t, $3 FROM unnest($4::text[], $5::text[]) AS x(p, t) \
         ON CONFLICT (project_id, path) DO NOTHING",
    )
    .bind(tenant.as_uuid())
    .bind(project.as_uuid())
    .bind(source_id)
    .bind(&paths)
    .bind(&types)
    .execute(&mut ***tx)
    .await?;
    for p in paths {
        st.caches.known_paths.insert((project.as_uuid(), p), ()).await;
    }
    Ok(())
}

const MAX_PATHS: usize = 200;

/// Leaf paths of a JSON value with an inferred catalog type (arrays: first element as `[0]`).
pub fn flatten(prefix: &str, v: &Value, depth: usize, out: &mut Vec<(String, String)>) {
    if out.len() >= MAX_PATHS {
        return;
    }
    let ty = |v: &Value| -> &'static str {
        match v {
            Value::Bool(_) => "bool",
            Value::Number(n) if n.is_i64() || n.is_u64() => "integer",
            Value::Number(_) => "number",
            Value::String(s) if chrono::DateTime::parse_from_rfc3339(s).is_ok() => "datetime",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
            _ => "string",
        }
    };
    match v {
        Value::Object(m) if depth < 6 && !m.is_empty() => {
            for (k, child) in m {
                flatten(&format!("{prefix}.{k}"), child, depth + 1, out);
            }
        }
        Value::Array(a) if depth < 6 && a.first().is_some_and(Value::is_object) => {
            if let Some(first) = a.first() {
                flatten(&format!("{prefix}[0]"), first, depth + 1, out);
            }
        }
        Value::Null => out.push((prefix.to_string(), "string".into())),
        other => out.push((prefix.to_string(), ty(other).into())),
    }
}

// ---------------------------------------------------------------------------------------------
// Scoring stages (7-9), shared by ingest, simulate and rescore
// ---------------------------------------------------------------------------------------------

#[derive(Debug)]
pub struct ScoreInput<'a> {
    pub pctx: &'a ProjectCtx,
    pub call: CallCtx,
    pub event_id: Uuid,
    pub customer_id: Uuid,
    pub event_type: String,
    pub occurred_at: DateTime<Utc>,
    pub event_ctx: Value,
    pub source: Map<String, Value>,
    pub customer_ctx: Value,
    pub features: Map<String, Value>,
    /// Customer exists in the graph (links succeeded / customer known).
    pub graph_linked: bool,
    pub dry_run: bool,
    pub degraded: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Scored {
    pub combined: CombineOutput,
    pub ml: MlSummary,
    pub graph: Option<GraphMetrics>,
    pub rule_results: Vec<RuleResultTrace>,
    pub degraded: Vec<String>,
    pub features: Map<String, Value>,
}

impl Scored {
    pub fn rules_degraded(&self) -> bool {
        self.degraded.iter().any(|d| d == "rules")
    }
}

/// Wrapper to keep the tuple readable.
struct Features(Map<String, Value>);

async fn graph_stage(st: &AppState, input: &ScoreInput<'_>) -> (Features, Result<Option<GraphMetrics>, ()>) {
    let mut features = input.features.clone();
    if !input.graph_linked {
        return (Features(features), Ok(None));
    }
    let t = Instant::now();
    let g = &input.pctx.graph;
    let req = GraphMetricsRequest {
        customer_id: input.customer_id,
        link_kinds: g.link_kinds.clone(),
        include_similar: g.include_similar,
        max_depth: g.max_depth,
    };
    let timeout = Duration::from_millis(input.pctx.settings.timeouts.graph_ms);
    let res = st
        .engines
        .graph
        .metrics(&input.call, input.pctx.project, &req, timeout)
        .await;
    stage("graph_metrics", t);
    match res {
        Ok(m) => {
            add_graph_features(&mut features, Some(&m));
            (Features(features), Ok(Some(m)))
        }
        Err(e) => {
            tracing::warn!(error = %e, "graph metrics failed; graph engine degraded");
            (Features(features), Err(()))
        }
    }
}

pub async fn score_stages(st: &AppState, input: ScoreInput<'_>) -> Scored {
    let mut degraded = input.degraded.clone();
    let settings = &input.pctx.settings;
    let project = input.pctx.project;

    // --- 7a graph metrics (before ML: same graph features as training)
    let (Features(features), graph_res) = graph_stage(st, &input).await;
    let graph = match graph_res {
        Ok(g) => g,
        Err(()) => {
            if !degraded.iter().any(|d| d == "graph") {
                degraded.push("graph".into());
            }
            None
        }
    };

    // --- 7b ML (parallel)
    let t = Instant::now();
    let ml_req = MlRequest {
        event_id: Some(input.event_id),
        features: features.clone(),
        source: Some(input.source.clone()),
        explain: true,
    };
    let ml_timeout = Duration::from_millis(settings.timeouts.ml_ms);
    let (pred, score) = tokio::join!(
        st.engines.ml.predict(&input.call, project, &ml_req, ml_timeout),
        st.engines.ml.score(&input.call, project, &ml_req, ml_timeout),
    );
    stage("ml", t);
    let mut ml = MlSummary::default();
    match pred {
        Ok(Some(p)) => {
            ml.fraud_probability = Some(p.fraud_probability);
            ml.supervised_model = Some(ModelRef {
                id: p.model_id,
                version: p.model_version,
                algorithm: p.algorithm,
            });
        }
        Ok(None) => {}
        Err(e) => {
            tracing::warn!(error = %e, "supervised predict failed; engine degraded");
            degraded.push("supervised".into());
        }
    }
    match score {
        Ok(Some(s)) => {
            ml.anomaly_score = Some(s.anomaly_score);
            ml.cluster_id = s.cluster_id;
            ml.cluster_fraud_rate = s.cluster_fraud_rate;
            ml.unsupervised_model = Some(ModelRef {
                id: s.model_id,
                version: s.model_version,
                algorithm: None,
            });
        }
        Ok(None) => {}
        Err(e) => {
            tracing::warn!(error = %e, "unsupervised score failed; engine degraded");
            degraded.push("unsupervised".into());
        }
    }

    // --- 8 rules
    let t = Instant::now();
    let req = EvaluateRequest {
        event_id: input.event_id,
        occurred_at: input.occurred_at,
        event_type: input.event_type.clone(),
        customer_id: input.customer_id,
        context: EvaluationContext {
            event: input.event_ctx.clone(),
            source: Value::Object(input.source.clone()),
            customer: input.customer_ctx.clone(),
            features: Value::Object(features.clone()),
            ml: ml_context(&ml),
            graph: graph_context(graph.as_ref()),
        },
        dry_run: input.dry_run,
    };
    let rules = st
        .engines
        .rules
        .evaluate(
            &input.call,
            project,
            &req,
            Duration::from_millis(settings.timeouts.rules_ms),
        )
        .await;
    stage("rules", t);
    let (rules_score, actions, rule_reasons, rule_results) = match rules {
        Ok(r) => (Some(r.rules_score), r.actions, r.reasons, r.rule_results),
        Err(e) => {
            tracing::warn!(error = %e, "rule-service evaluate failed; using rules_unavailable_decision");
            degraded.push("rules".into());
            (None, Default::default(), Vec::new(), Vec::new())
        }
    };
    for d in &degraded {
        metrics::counter!("engine_degraded_total", "engine" => d.clone()).increment(1);
    }

    // --- 9 combine
    let combined = combine(
        &CombineInput {
            rules_score,
            actions,
            rule_reasons: &rule_reasons,
            ml: Some(&ml),
            graph: graph.as_ref(),
            degraded: &degraded,
        },
        settings,
    );
    Scored {
        combined,
        ml,
        graph,
        rule_results,
        degraded,
        features,
    }
}

#[allow(clippy::too_many_arguments)]
async fn persist_scored(
    st: &AppState,
    tenant: TenantId,
    project: ProjectId,
    customer_id: Uuid,
    event_id: Uuid,
    external_id: &str,
    scored: &Scored,
    started: Instant,
) -> AppResult<DecisionOut> {
    let t = Instant::now();
    let pctx = project_ctx(st, tenant, project).await?;
    let latency_ms = started.elapsed().as_millis() as u64;
    let c = &scored.combined;
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    repo::upsert_features(&mut tx, tenant, project, event_id, &scored.features).await?;
    let decision_id = repo::upsert_decision(
        &mut tx,
        tenant,
        project,
        &repo::DecisionRecord {
            event_id,
            decision: c.decision,
            final_score: c.final_score,
            engine_scores: &c.engine_scores,
            ml: &scored.ml,
            graph: scored.graph.as_ref(),
            reasons: &c.reasons,
            rule_results: &scored.rule_results,
            degraded: &scored.degraded,
            latency_ms,
        },
    )
    .await?;
    let case_id = if pctx.settings.cases.auto_create_on.contains(&c.decision) {
        let priority = if c.decision == Decision::Decline { 1 } else { 3 };
        Some(
            repo::open_or_attach_case(
                &mut tx,
                tenant,
                project,
                customer_id,
                event_id,
                decision_id,
                priority,
            )
            .await?,
        )
    } else {
        None
    };
    repo::mark_needs_rescore(&mut tx, event_id, scored.rules_degraded()).await?;
    tx.commit().await?;
    stage("persist_decision", t);
    stage("total", started);
    metrics::counter!("decisions_total", "decision" => c.decision.as_str()).increment(1);

    Ok(DecisionOut {
        event_id,
        external_id: external_id.to_string(),
        project_id: project.as_uuid(),
        decision: c.decision,
        final_score: c.final_score,
        engine_scores: c.engine_scores.clone(),
        reasons: c.reasons.clone(),
        rule_results: scored.rule_results.clone(),
        ml: scored.ml.clone(),
        graph: scored.graph.clone(),
        degraded: scored.degraded.clone(),
        case_id,
        latency_ms,
        persisted: true,
    })
}

// ---------------------------------------------------------------------------------------------
// Context builders
// ---------------------------------------------------------------------------------------------

/// `event.*` context: canonical fields without the nested customer/payload, plus `customer_id`.
pub fn event_context(ev: &CanonicalEventIn, customer_id: Uuid) -> Value {
    let mut v = serde_json::to_value(ev).unwrap_or_else(|_| json!({}));
    if let Some(m) = v.as_object_mut() {
        m.remove("customer");
        m.remove("payload");
        m.retain(|_, v| !v.is_null());
        m.insert("customer_id".into(), json!(customer_id));
    }
    v
}

pub fn customer_context(c: &repo::CustomerRow, features: &Map<String, Value>) -> Value {
    json!({
        "external_id": c.external_id,
        "kyc_level": c.kyc_level,
        "segment": c.segment,
        "status": c.status,
        "registered_at": c.registered_at,
        "risk_label": c.risk_label,
        "account_age_days": features.get("account_age_days").cloned().unwrap_or(Value::Null),
        "attributes": c.attributes,
    })
}

fn ml_context(ml: &MlSummary) -> Value {
    let mut m = Map::new();
    if let Some(p) = ml.fraud_probability {
        m.insert("fraud_probability".into(), json!(p));
    }
    if let Some(a) = ml.anomaly_score {
        m.insert("anomaly_score".into(), json!(a));
    }
    if let Some(c) = ml.cluster_id {
        m.insert("cluster_id".into(), json!(c));
    }
    if let Some(r) = ml.cluster_fraud_rate {
        m.insert("cluster_fraud_rate".into(), json!(r));
    }
    if let Some(s) = &ml.supervised_model {
        m.insert("model_version".into(), json!(s.version));
    }
    Value::Object(m)
}

// ---------------------------------------------------------------------------------------------
// Simulate & rescore
// ---------------------------------------------------------------------------------------------

/// Dry-run of the whole pipeline: nothing is persisted, rule-service runs with `dry_run=true`.
pub async fn simulate(
    st: &AppState,
    ictx: &IngestCtx,
    source: &SourceCfg,
    raw: &Value,
) -> AppResult<DecisionOut> {
    let started = Instant::now();
    let pctx = project_ctx(st, ictx.tenant, ictx.project).await?;
    let pepper = pii::tenant_pepper(&st.cfg.pii_pepper, ictx.tenant);
    let (norm, _label) = map_record(source, &pepper, pctx.timezone, raw).map_err(AppError::validation)?;

    let mut tx = TenantTx::begin(&st.pool, ictx.tenant).await?;
    let existing =
        repo::find_customer_by_external(&mut tx, ictx.project, &norm.event.customer.external_id).await?;
    let event_id = Uuid::new_v4();
    let (customer_id, registered_at, customer_ctx_row) = match &existing {
        Some(c) => (c.id, c.registered_at, Some(c.clone())),
        None => (Uuid::new_v4(), norm.event.customer.registered_at, None),
    };
    let raw_aggs = features_sql::fetch(&mut tx, ictx.project, event_id, customer_id, &norm.event).await?;
    tx.rollback().await?;
    let features = assemble(
        &norm.event,
        &raw_aggs,
        &CustomerFacts { registered_at },
        pctx.timezone,
    );
    let customer_ctx = match &customer_ctx_row {
        Some(c) => customer_context(c, &features),
        None => json!({
            "external_id": norm.event.customer.external_id,
            "kyc_level": norm.event.customer.kyc_level,
            "segment": norm.event.customer.segment,
            "status": "active",
            "registered_at": norm.event.customer.registered_at,
            "risk_label": "unknown",
            "account_age_days": features.get("account_age_days").cloned().unwrap_or(Value::Null),
            "attributes": norm.event.customer.attributes.clone().unwrap_or_default(),
        }),
    };
    let input = ScoreInput {
        pctx: &pctx,
        call: ictx.call(),
        event_id,
        customer_id,
        event_type: norm.event.event_type.clone(),
        occurred_at: norm.event.occurred_at,
        event_ctx: event_context(&norm.event, customer_id),
        source: norm.event.payload.clone().unwrap_or_default(),
        customer_ctx,
        features,
        graph_linked: existing.is_some(),
        dry_run: true,
        degraded: Vec::new(),
    };
    let s = score_stages(st, input).await;
    let c = s.combined;
    Ok(DecisionOut {
        event_id,
        external_id: norm.event.external_id,
        project_id: ictx.project.as_uuid(),
        decision: c.decision,
        final_score: c.final_score,
        engine_scores: c.engine_scores,
        reasons: c.reasons,
        rule_results: s.rule_results,
        ml: s.ml,
        graph: s.graph,
        degraded: s.degraded,
        case_id: None,
        latency_ms: started.elapsed().as_millis() as u64,
        persisted: false,
    })
}

/// `(event_ctx, customer_id, event_type, occurred_at, external_id, needs_rescore, payload)`.
type StoredEventRow = (Value, Uuid, String, DateTime<Utc>, String, bool, Value);

/// Re-runs steps 7–10 for a stored event (after an outage or rule changes).
pub async fn rescore(
    st: &AppState,
    ictx: &IngestCtx,
    event_id: Uuid,
) -> AppResult<(Option<DecisionOut>, DecisionOut)> {
    let started = Instant::now();
    let (tenant, project) = (ictx.tenant, ictx.project);
    let pctx = project_ctx(st, tenant, project).await?;
    let mut tx = TenantTx::begin(&st.pool, tenant).await?;
    let row: Option<StoredEventRow> = sqlx::query_as(
        "SELECT to_jsonb(e.*) - ARRAY['tenant_id','project_id','payload','load_only','needs_rescore', \
                'received_at','data_source_id'], \
                e.customer_id, e.event_type, e.occurred_at, e.external_id, e.needs_rescore, e.payload \
         FROM core.events e WHERE e.project_id = $1 AND e.id = $2",
    )
    .bind(project.as_uuid())
    .bind(event_id)
    .fetch_optional(&mut **tx)
    .await?;
    let (event_ctx, customer_id, event_type, occurred_at, external_id, needs_rescore, payload) =
        row.ok_or_else(|| AppError::not_found("event not found"))?;
    let features: Option<(Value,)> =
        sqlx::query_as("SELECT features FROM core.event_features WHERE event_id = $1")
            .bind(event_id)
            .fetch_optional(&mut **tx)
            .await?;
    let customer = repo::find_customer(&mut tx, project, customer_id)
        .await?
        .ok_or_else(|| AppError::not_found("customer not found"))?;
    let before = repo::load_decision_out(&mut tx, project, event_id).await?;
    tx.commit().await?;

    let features = features
        .and_then(|f| f.0.as_object().cloned())
        .unwrap_or_default();
    let mut event_ctx = event_ctx;
    if let Some(m) = event_ctx.as_object_mut() {
        m.remove("id");
        m.retain(|_, v| !v.is_null());
    }
    let input = ScoreInput {
        pctx: &pctx,
        call: ictx.call(),
        event_id,
        customer_id,
        event_type,
        occurred_at,
        event_ctx,
        source: payload.as_object().cloned().unwrap_or_default(),
        customer_ctx: customer_context(&customer, &features),
        features,
        graph_linked: true,
        // rule hits were already written for this event unless rules were down at ingest
        dry_run: !needs_rescore && before.is_some(),
        degraded: Vec::new(),
    };
    let scored = score_stages(st, input).await;
    let out = persist_scored(
        st,
        tenant,
        project,
        customer_id,
        event_id,
        &external_id,
        &scored,
        started,
    )
    .await?;
    Ok((before, out))
}

/// Resolves the source used by `POST /events` and `/score/simulate` with `source_id`.
pub async fn resolve_source(
    st: &AppState,
    tenant: TenantId,
    project: ProjectId,
    source_id: Option<Uuid>,
) -> AppResult<std::sync::Arc<SourceCfg>> {
    let id = match source_id {
        Some(id) => id,
        None => {
            let mut tx = TenantTx::begin(&st.pool, tenant).await?;
            let id = crate::application::context::canonical_source_id(&mut tx, project).await?;
            tx.commit().await?;
            id
        }
    };
    source_cfg(st, tenant, project, id).await
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn flatten_paths_and_types() {
        let v = json!({ "a": { "b": 1, "c": [ { "d": "x" } ], "t": "2026-09-23T10:00:00Z" }, "n": null, "f": 1.5,
                        "tags": ["x", "y"], "ok": true });
        let mut out = Vec::new();
        flatten("source", &v, 0, &mut out);
        let m: std::collections::HashMap<_, _> = out.into_iter().collect();
        assert_eq!(m["source.a.b"], "integer");
        assert_eq!(m["source.a.c[0].d"], "string");
        assert_eq!(m["source.a.t"], "datetime");
        assert_eq!(m["source.f"], "number");
        assert_eq!(m["source.tags"], "array");
        assert_eq!(m["source.ok"], "bool");
        assert_eq!(m["source.n"], "string");
    }

    #[test]
    fn redacts_pans_in_dead_letters() {
        let v = json!({ "card": "4111 1111 1111 1111", "id": "123", "nested": ["5555555555554444"] });
        let r = redact_record(&v);
        assert!(!r.to_string().contains("4111 1111 1111 1111"));
        assert!(!r.to_string().contains("5555555555554444"));
        assert_eq!(r["id"], json!("123"));
    }

    #[test]
    fn event_context_drops_nested_parts() {
        let ev = CanonicalEventIn {
            external_id: "T".into(),
            event_type: "login".into(),
            login_success: Some(false),
            payload: Some(Map::new()),
            ..Default::default()
        };
        let cid = Uuid::nil();
        let v = event_context(&ev, cid);
        assert!(v.get("customer").is_none());
        assert!(v.get("payload").is_none());
        assert_eq!(v["login_success"], json!(false));
        assert_eq!(v["customer_id"], json!(cid));
    }
}
