//! Read use cases: pipeline metrics, rule metrics, labels, and the UI exploration endpoints.
//!
//! The read models (`NeighborhoodOut`, `ProximityOut`, …) are defined here and serialised
//! as-is by the API layer. They are this service's published language.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use contracts::graph::{GraphMetric, GraphMetrics, LinkKind};
use platform::db::TenantTx;
use platform::{pii, AppError, AppResult, ProjectId, TenantId};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::links::RISK_LABELS;
use super::project_config::Overrides;
use super::state::AppState;
use crate::adapters::postgres::{self, PgGraphStore};
use crate::domain::components::{build_components, Component};
use crate::domain::explore::Exploration;
use crate::domain::extract::normalize_ip;
use crate::domain::metrics::{compute_metrics, compute_single};
use crate::domain::model::{limits, CustomerId, EntityId};
use crate::domain::ports::GraphStore;

// ---------------------------------------------------------------------------------------------
// Pipeline & rule metrics
// ---------------------------------------------------------------------------------------------

pub async fn metrics(
    state: &AppState,
    tenant: TenantId,
    project: ProjectId,
    customer: CustomerId,
    o: &Overrides,
) -> AppResult<GraphMetrics> {
    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let params = state.configs.get(&mut tx, tenant, project).await?.params(o);
    let m = compute_metrics(&mut PgGraphStore::new(&mut tx, project), customer, &params).await?;
    tx.commit().await?;
    Ok(m)
}

pub async fn single_metric(
    state: &AppState,
    tenant: TenantId,
    project: ProjectId,
    customer: CustomerId,
    metric: GraphMetric,
    o: &Overrides,
) -> AppResult<Option<f64>> {
    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let params = state.configs.get(&mut tx, tenant, project).await?.params(o);
    let v = compute_single(
        &mut PgGraphStore::new(&mut tx, project),
        customer,
        metric,
        &params,
    )
    .await?;
    tx.commit().await?;
    Ok(v)
}

/// Mirrors a customer's label. 404 when the customer has no graph node yet (it will be created with
/// the right label on its next `links` call, because core-api sends the current label).
pub async fn set_label(
    state: &AppState,
    tenant: TenantId,
    project: ProjectId,
    customer: CustomerId,
    label: &str,
) -> AppResult<()> {
    if !RISK_LABELS.contains(&label) {
        return Err(AppError::field(
            "risk_label",
            "must be one of fraud, legit, unknown",
        ));
    }
    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let updated = postgres::set_label(&mut tx, project, customer, label).await?;
    tx.commit().await?;
    if !updated {
        return Err(AppError::not_found("customer has no graph node"));
    }
    state.components.invalidate(&(tenant, project)).await;
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Neighbourhood (Cytoscape)
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct GraphNode {
    /// Customer UUID, or `e<entity id>` for entities.
    pub id: String,
    /// `customer` | `entity`
    #[serde(rename = "type")]
    pub node_type: &'static str,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<LinkKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_center: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub customer_count: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct GraphEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    /// Link kind for customer–entity edges, `similar` for similarity edges.
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub similarity: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct NeighborhoodOut {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    /// `true` when `limit_nodes` cut the result.
    pub truncated: bool,
}

fn entity_node_id(id: EntityId) -> String {
    format!("e{id}")
}

pub async fn neighborhood(
    state: &AppState,
    tenant: TenantId,
    project: ProjectId,
    center: CustomerId,
    o: &Overrides,
    limit_nodes: usize,
) -> AppResult<NeighborhoodOut> {
    let limit_nodes = limit_nodes.clamp(1, limits::NEIGHBORHOOD_MAX_NODES);
    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let mut params = state.configs.get(&mut tx, tenant, project).await?.params(o);
    params.max_depth = params.max_depth.min(3);
    let mut store = PgGraphStore::new(&mut tx, project);
    let infos = store.customers_info(&[center]).await?;
    if infos.is_empty() {
        return Err(AppError::not_found("customer not found in graph"));
    }
    let mut ex = Exploration::new(center);
    while ex.can_continue() && ex.depth < params.max_depth {
        ex.step(&mut store, &params, limit_nodes, true).await?;
    }
    let infos: HashMap<Uuid, _> = store
        .customers_info(&ex.order)
        .await?
        .into_iter()
        .map(|c| (c.id, c))
        .collect();
    tx.commit().await?;

    let mut nodes = Vec::new();
    let mut present: HashSet<String> = HashSet::new();
    let mut truncated = ex.truncated;
    for c in &ex.order {
        let v = &ex.visits[c];
        let info = infos.get(c);
        let id = c.to_string();
        present.insert(id.clone());
        nodes.push(GraphNode {
            id,
            node_type: "customer",
            label: info.map(|i| i.external_id.clone()).unwrap_or_default(),
            kind: None,
            risk_label: Some(info.map_or_else(|| "unknown".into(), |i| i.risk_label.clone())),
            is_center: Some(*c == center),
            depth: Some(v.depth),
            customer_count: None,
        });
    }
    // Entities: only those attached to a present customer, in a stable order, within the node budget.
    let mut entity_ids: Vec<EntityId> = ex
        .links
        .iter()
        .filter(|(c, _)| present.contains(&c.to_string()))
        .map(|(_, e)| *e)
        .collect();
    entity_ids.sort_unstable();
    entity_ids.dedup();
    for e in entity_ids {
        if nodes.len() >= limit_nodes {
            truncated = true;
            break;
        }
        if let Some(r) = ex.entities.get(&e) {
            let id = entity_node_id(e);
            present.insert(id.clone());
            nodes.push(GraphNode {
                id,
                node_type: "entity",
                label: r.display.clone(),
                kind: Some(r.kind),
                risk_label: None,
                is_center: None,
                depth: None,
                customer_count: Some(r.customer_count),
            });
        }
    }
    let mut edges = Vec::new();
    for (c, e) in &ex.links {
        let (src, dst) = (c.to_string(), entity_node_id(*e));
        if present.contains(&src) && present.contains(&dst) {
            let kind = ex.entities.get(e).map_or("entity", |r| r.kind.as_str());
            edges.push(GraphEdge {
                id: format!("l:{src}:{dst}"),
                source: src,
                target: dst,
                kind: kind.to_string(),
                similarity: None,
            });
        }
    }
    let mut seen_sim = HashSet::new();
    for (a, b, s) in &ex.similar {
        let (x, y) = if a < b { (*a, *b) } else { (*b, *a) };
        let (src, dst) = (entity_node_id(x), entity_node_id(y));
        if present.contains(&src) && present.contains(&dst) && seen_sim.insert((x, y)) {
            edges.push(GraphEdge {
                id: format!("s:{src}:{dst}"),
                source: src,
                target: dst,
                kind: "similar".into(),
                similarity: Some(*s),
            });
        }
    }
    Ok(NeighborhoodOut {
        nodes,
        edges,
        truncated,
    })
}

// ---------------------------------------------------------------------------------------------
// Fraud proximity (shortest path)
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct PathNode {
    pub id: String,
    #[serde(rename = "type")]
    pub node_type: &'static str,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<LinkKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk_label: Option<String>,
    /// Set on an entity reached through a similarity edge.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub similarity: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct ProximityOut {
    /// Hops to the nearest fraud customer; `null` when none within `max_depth`.
    pub distance: Option<u32>,
    /// Alternating customer / entity nodes from the start customer to the nearest fraud customer.
    pub path: Vec<PathNode>,
    pub nearest_fraud_customer_id: Option<Uuid>,
    /// Cumulative fraud customers within k hops, keyed `"1"`…`"max_depth"`.
    pub fraud_within: BTreeMap<String, u32>,
    pub truncated: bool,
}

pub async fn fraud_proximity(
    state: &AppState,
    tenant: TenantId,
    project: ProjectId,
    start: CustomerId,
    o: &Overrides,
) -> AppResult<ProximityOut> {
    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let params = state.configs.get(&mut tx, tenant, project).await?.params(o);
    let mut store = PgGraphStore::new(&mut tx, project);
    if store.customers_info(&[start]).await?.is_empty() {
        return Err(AppError::not_found("customer not found in graph"));
    }
    let mut ex = Exploration::new(start);
    while ex.can_continue() && ex.depth < params.max_depth {
        ex.step(&mut store, &params, limits::METRICS_NODE_LIMIT, false)
            .await?;
    }
    let nearest = ex.nearest_fraud(params.max_depth);
    let hops = nearest.map(|(c, _)| ex.path_to(c)).unwrap_or_default();
    let mut ids: Vec<Uuid> = vec![start];
    ids.extend(hops.iter().map(|(_, _, to)| *to));
    let infos: HashMap<Uuid, _> = store
        .customers_info(&ids)
        .await?
        .into_iter()
        .map(|c| (c.id, c))
        .collect();
    tx.commit().await?;

    let customer_node = |c: &Uuid| PathNode {
        id: c.to_string(),
        node_type: "customer",
        label: infos.get(c).map(|i| i.external_id.clone()).unwrap_or_default(),
        kind: None,
        risk_label: infos.get(c).map(|i| i.risk_label.clone()),
        similarity: None,
    };
    let entity_node = |e: EntityId, sim: Option<f32>| {
        let r = ex.entities.get(&e);
        PathNode {
            id: entity_node_id(e),
            node_type: "entity",
            label: r.map(|r| r.display.clone()).unwrap_or_default(),
            kind: r.map(|r| r.kind),
            risk_label: None,
            similarity: sim,
        }
    };
    let mut path = Vec::new();
    if !hops.is_empty() {
        path.push(customer_node(&start));
        for (_, via, to) in &hops {
            path.push(entity_node(via.entity, None));
            if let Some((sim_entity, score)) = via.similar {
                path.push(entity_node(sim_entity, Some(score)));
            }
            path.push(customer_node(to));
        }
    }
    let fraud_within = (1..=params.max_depth)
        .map(|k| {
            (
                k.to_string(),
                u32::try_from(ex.fraud_within(k)).unwrap_or(u32::MAX),
            )
        })
        .collect();
    Ok(ProximityOut {
        distance: nearest.map(|(_, d)| d),
        path,
        nearest_fraud_customer_id: nearest.map(|(c, _)| c),
        fraud_within,
        truncated: ex.truncated,
    })
}

// ---------------------------------------------------------------------------------------------
// Components, stats, search
// ---------------------------------------------------------------------------------------------

/// Hard cap on returned components.
pub const MAX_COMPONENTS: usize = 500;

pub async fn components(
    state: &AppState,
    tenant: TenantId,
    project: ProjectId,
    min_size: u32,
    only_with_fraud: bool,
) -> AppResult<Vec<Component>> {
    let all = match state.components.get(&(tenant, project)).await {
        Some(c) => c,
        None => {
            let computed = Arc::new(compute_components(state, tenant, project).await?);
            state.components.insert((tenant, project), computed.clone()).await;
            computed
        }
    };
    Ok(all
        .iter()
        .filter(|c| c.size >= min_size.max(2))
        .filter(|c| !only_with_fraud || c.fraud_count > 0)
        .take(MAX_COMPONENTS)
        .cloned()
        .collect())
}

async fn compute_components(
    state: &AppState,
    tenant: TenantId,
    project: ProjectId,
) -> AppResult<Vec<Component>> {
    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let cfg = state.configs.get(&mut tx, tenant, project).await?;
    let kinds = cfg.kinds();
    let links = postgres::component_links(&mut tx, project, &kinds, cfg.supernode_cap()).await?;
    let sims = if cfg.include_similar {
        postgres::similarity_customer_pairs(&mut tx, project, &kinds, cfg.supernode_cap(), cfg.threshold())
            .await?
    } else {
        Vec::new()
    };
    let fraud: HashSet<Uuid> = postgres::fraud_customers(&mut tx, project)
        .await?
        .into_iter()
        .collect();
    tx.commit().await?;

    // Star edges: every customer of an entity is joined to the entity's first customer.
    let mut edges = Vec::with_capacity(links.len() + sims.len());
    let mut current: Option<(EntityId, Uuid)> = None;
    for (entity, customer) in links {
        match current {
            Some((e, first)) if e == entity => edges.push((first, customer)),
            _ => current = Some((entity, customer)),
        }
    }
    edges.extend(sims);
    Ok(build_components(edges, &fraud))
}

#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct SupernodeOut {
    pub entity_id: i64,
    pub kind: LinkKind,
    pub display_value: String,
    pub degree: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct StatsOut {
    pub customers: i64,
    pub fraud_customers: i64,
    pub entities: i64,
    pub links: i64,
    pub similarity_links: i64,
    /// Entities above this customer count are not traversed.
    pub supernode_degree_cap: u32,
    pub supernodes: Vec<SupernodeOut>,
}

pub async fn stats(state: &AppState, tenant: TenantId, project: ProjectId) -> AppResult<StatsOut> {
    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let cfg = state.configs.get(&mut tx, tenant, project).await?;
    let (customers, entities, links, similarity_links, fraud_customers) =
        postgres::counts(&mut tx, project).await?;
    let supernodes = postgres::supernodes(&mut tx, project, cfg.supernode_cap(), 10).await?;
    tx.commit().await?;
    Ok(StatsOut {
        customers,
        fraud_customers,
        entities,
        links,
        similarity_links,
        supernode_degree_cap: cfg.supernode_cap(),
        supernodes: supernodes
            .into_iter()
            .map(|e| SupernodeOut {
                entity_id: e.id,
                kind: e.kind,
                display_value: e.display,
                degree: e.customer_count,
            })
            .collect(),
    })
}

#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct CustomerHit {
    pub id: Uuid,
    pub external_id: String,
    pub risk_label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct EntityHit {
    pub id: i64,
    pub kind: LinkKind,
    pub display_value: String,
    pub customer_count: u32,
    /// Up to 10 most recently linked customers.
    pub customer_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct SearchOut {
    pub customers: Vec<CustomerHit>,
    pub entities: Vec<EntityHit>,
}

/// Customers by external-id prefix, entities by exact normalised value (email, phone, IP, device,
/// ref transaction, API client). Card/account entities are hashes and are not searchable by value.
pub async fn search(state: &AppState, tenant: TenantId, project: ProjectId, q: &str) -> AppResult<SearchOut> {
    let q = q.trim();
    if q.chars().count() < 2 {
        return Err(AppError::field("q", "must contain at least 2 characters"));
    }
    let mut values: Vec<String> = vec![q.to_string(), q.to_lowercase()];
    values.extend(pii::normalize_email(q));
    values.extend(pii::normalize_phone(q, "62"));
    values.extend(normalize_ip(q));
    values.sort();
    values.dedup();

    let mut tx = TenantTx::begin(&state.pool, tenant).await?;
    let customers = postgres::search_customers(&mut tx, project, q, 20).await?;
    let entities = postgres::search_entities(&mut tx, project, &values, 20).await?;
    tx.commit().await?;
    Ok(SearchOut {
        customers: customers
            .into_iter()
            .map(|c| CustomerHit {
                id: c.id,
                external_id: c.external_id,
                risk_label: c.risk_label,
            })
            .collect(),
        entities: entities
            .into_iter()
            .map(|(e, customer_ids)| EntityHit {
                id: e.id,
                kind: e.kind,
                display_value: e.display,
                customer_count: e.customer_count,
                customer_ids,
            })
            .collect(),
    })
}
