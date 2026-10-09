//! Use case: entity resolution for one event (`POST /v1/projects/{pid}/links`).
//!
//! Steps, in one tenant transaction:
//! 1. extract and normalise entities (domain);
//! 2. upsert the customer node, the entities and the `customer — entity` links;
//! 3. for entities **created by this call**, look for similar existing entities (phone, email,
//!    address) and store scored similarity edges. At most `SIMILARITY_CANDIDATES` candidates per
//!    entity are checked, which keeps latency bounded.
//!
//! Repeating the same request is idempotent: no duplicate entities, links or similarity edges,
//! and `event_count` is not incremented twice.

use contracts::graph::{GraphLinksRequest, GraphLinksResponse, LinkKind};
use platform::db::TenantTx;
use platform::{AppError, AppResult, ProjectId, TenantId};

use super::state::AppState;
use crate::adapters::postgres::{self, SimilarityEdge, UpsertedEntity};
use crate::domain::extract::extract_entities;
use crate::domain::model::limits;
use crate::domain::similarity::{
    email_local, email_similarity, ordered_pair, phone_similarity, phone_suffix,
};

pub const RISK_LABELS: [&str; 3] = ["fraud", "legit", "unknown"];

pub async fn ingest_links(
    state: &AppState,
    tenant: TenantId,
    project: ProjectId,
    req: &GraphLinksRequest,
) -> AppResult<GraphLinksResponse> {
    if !RISK_LABELS.contains(&req.customer.risk_label.as_str()) {
        return Err(AppError::field(
            "customer.risk_label",
            "must be one of fraud, legit, unknown",
        ));
    }
    if req.customer.external_id.trim().is_empty() {
        return Err(AppError::field("customer.external_id", "must not be empty"));
    }
    let keys = extract_entities(req);

    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let cfg = state.configs.get(&mut tx, tenant, project).await?;

    postgres::upsert_customer(&mut tx, tenant, project, &req.customer).await?;
    let entities = postgres::upsert_entities(&mut tx, tenant, project, &keys, req.event.occurred_at).await?;
    let ids: Vec<i64> = entities.iter().map(|e| e.id).collect();
    let new_links = postgres::upsert_links(
        &mut tx,
        tenant,
        project,
        req.customer.id,
        req.event.id,
        &ids,
        req.event.occurred_at,
    )
    .await?;

    let mut edges = Vec::new();
    for e in entities.iter().filter(|e| e.inserted) {
        edges.extend(similarity_for(&mut tx, project, e, cfg.threshold()).await?);
    }
    let similarity_links_created = postgres::insert_similarity(&mut tx, tenant, project, &edges).await?;
    tx.commit().await?;

    metrics::counter!("graph_links_new_total").increment(new_links.len() as u64);
    metrics::counter!("graph_similarity_links_total").increment(u64::from(similarity_links_created));

    let mut entity_ids = ids;
    entity_ids.sort_unstable();
    Ok(GraphLinksResponse {
        entity_ids,
        new_links: u32::try_from(new_links.len()).unwrap_or(u32::MAX),
        similarity_links_created,
    })
}

async fn similarity_for(
    tx: &mut TenantTx<'static>,
    project: ProjectId,
    e: &UpsertedEntity,
    address_threshold: f32,
) -> AppResult<Vec<SimilarityEdge>> {
    let limit = limits::SIMILARITY_CANDIDATES;
    let mut out = Vec::new();
    match e.kind {
        LinkKind::Phone => {
            let Some(suffix) = phone_suffix(&e.value, 9) else {
                return Ok(out);
            };
            for (id, other) in postgres::phone_candidates(tx, project, e.id, &e.value, &suffix, limit).await?
            {
                if let Some((score, method)) = phone_similarity(&e.value, &other) {
                    out.push(edge(e.id, id, score, method));
                }
            }
        }
        LinkKind::Email => {
            let Some(local) = email_local(&e.value) else {
                return Ok(out);
            };
            for (id, other) in postgres::email_candidates(tx, project, e.id, local, limit).await? {
                if let Some((score, method)) = email_similarity(&e.value, &other) {
                    out.push(edge(e.id, id, score, method));
                }
            }
        }
        LinkKind::Address => {
            for (id, score) in
                postgres::address_candidates(tx, project, e.id, &e.value, address_threshold, limit).await?
            {
                out.push(edge(e.id, id, score, "trigram"));
            }
        }
        _ => {}
    }
    Ok(out)
}

fn edge(x: i64, y: i64, score: f32, method: &'static str) -> SimilarityEdge {
    let (a, b) = ordered_pair(x, y);
    SimilarityEdge { a, b, score, method }
}
