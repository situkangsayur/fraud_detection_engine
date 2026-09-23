//! Ingest entry points: webhook API-key authentication and batch processing.
//!
//! Webhook keys are resolved **before** any tenant context exists, via the SECURITY DEFINER function
//! `core.resolve_source_key(prefix)` (returns routing ids + argon2 hash only), then verified with
//! argon2. A successful verification is cached for 5 minutes, keyed by SHA-256 of the key, so the
//! slow hash is not paid per event.

use platform::error::{AppError, AppResult};
use platform::{ProjectId, TenantId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::Row;
use uuid::Uuid;

use crate::adapters::crypto::{api_key_prefix, sha256_hex, verify_secret};
use crate::application::context::source_cfg;
use crate::application::pipeline::{process_record, IngestCtx, IngestMode, RecordError, RecordOutcome};
use crate::state::{AppState, SourceAuth, SourceCfg};

pub const MAX_BATCH: usize = 1000;

/// Verifies `X-Api-Key` for the webhook source `slug`.
pub async fn authenticate_key(st: &AppState, slug: &str, key: &str) -> AppResult<SourceAuth> {
    let unauthorized = || AppError::Unauthorized("invalid API key".into());
    let digest = sha256_hex(key);
    if let Some(a) = st.caches.api_keys.get(&digest).await {
        if a.slug == slug {
            return Ok(a);
        }
        return Err(unauthorized());
    }
    let prefix = api_key_prefix(key).ok_or_else(unauthorized)?;
    let row = sqlx::query(
        "SELECT tenant_id, project_id, data_source_id, slug, api_key_hash, is_active \
         FROM core.resolve_source_key($1)",
    )
    .bind(&prefix)
    .fetch_optional(&st.pool)
    .await?;
    let Some(row) = row else {
        crate::adapters::crypto::dummy_verify(key);
        return Err(unauthorized());
    };
    let hash: Option<String> = row.get("api_key_hash");
    let active: bool = row.get("is_active");
    let row_slug: String = row.get("slug");
    if !hash.as_deref().is_some_and(|h| verify_secret(key, h)) {
        return Err(unauthorized());
    }
    if !active {
        return Err(AppError::Forbidden(
            "data source, project or tenant is inactive".into(),
        ));
    }
    if row_slug != slug {
        return Err(unauthorized());
    }
    let auth = SourceAuth {
        tenant: TenantId(row.get("tenant_id")),
        project: ProjectId(row.get("project_id")),
        source_id: row.get("data_source_id"),
        slug: row_slug,
    };
    st.caches.api_keys.insert(digest, auth.clone()).await;
    Ok(auth)
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
pub struct BatchIn {
    #[schema(value_type = Vec<Object>)]
    pub records: Vec<Value>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub job_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct BatchError {
    pub index: usize,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct BatchDecision {
    pub external_id: String,
    pub event_id: Uuid,
    /// `null` for `load_only` records.
    pub decision: Option<contracts::Decision>,
    pub final_score: Option<f64>,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct BatchOut {
    pub accepted: usize,
    pub rejected: usize,
    pub errors: Vec<BatchError>,
    pub decisions: Vec<BatchDecision>,
}

pub fn resolve_mode(requested: Option<&str>, source: &SourceCfg) -> AppResult<IngestMode> {
    match requested {
        Some(m) => IngestMode::parse(m).ok_or_else(|| AppError::field("mode", "score | load_only")),
        None => Ok(IngestMode::parse(&source.mode).unwrap_or(IngestMode::Score)),
    }
}

/// Processes records **in order** (velocity features depend on it). Invalid records are reported
/// and dead-lettered; an infrastructure failure aborts the batch (5xx → the sender retries;
/// already-processed records are deduplicated on retry).
pub async fn process_batch(
    st: &AppState,
    ictx: &IngestCtx,
    source_id: Uuid,
    input: &BatchIn,
) -> AppResult<BatchOut> {
    if input.records.is_empty() || input.records.len() > MAX_BATCH {
        return Err(AppError::field("records", format!("1..{MAX_BATCH} records")));
    }
    let source = source_cfg(st, ictx.tenant, ictx.project, source_id).await?;
    if !source.is_active {
        return Err(AppError::Forbidden("data source is inactive".into()));
    }
    let mode = resolve_mode(input.mode.as_deref(), &source)?;
    let mut out = BatchOut {
        accepted: 0,
        rejected: 0,
        errors: Vec::new(),
        decisions: Vec::new(),
    };
    for (index, raw) in input.records.iter().enumerate() {
        match process_record(st, ictx, &source, raw, mode).await {
            Ok(outcome) => {
                out.accepted += 1;
                out.decisions.push(match &outcome {
                    RecordOutcome::Decided(d) => BatchDecision {
                        external_id: d.external_id.clone(),
                        event_id: d.event_id,
                        decision: Some(d.decision),
                        final_score: Some(d.final_score),
                    },
                    RecordOutcome::Loaded {
                        event_id,
                        external_id,
                    } => BatchDecision {
                        external_id: external_id.clone(),
                        event_id: *event_id,
                        decision: None,
                        final_score: None,
                    },
                });
            }
            Err(RecordError::Invalid(errs)) => {
                out.rejected += 1;
                out.errors.push(BatchError {
                    index,
                    reason: RecordError::Invalid(errs).reason(),
                });
            }
            Err(RecordError::Failed(e)) => return Err(e),
        }
    }
    Ok(out)
}
