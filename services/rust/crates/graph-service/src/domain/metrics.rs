//! Graph metrics used by the scoring pipeline (`graph.*`) and by graph rules (rule-dsl §6.5).
//!
//! Semantics:
//! * `distance_to_fraud`: minimum customer→customer hops from the start customer to **another**
//!   customer labelled `fraud`, within `max_depth`. `None` means none found (rules read it as +∞).
//!   The start customer's own label is ignored, because "is this customer already known fraud" is
//!   a customer attribute, not a graph signal.
//! * `fraud_neighbors_k`: fraud customers within k hops.
//! * `degree`: customers at distance 1.
//! * `shared_entity_count`: the start customer's entities (allowed kinds, excluding supernodes)
//!   that are linked to at least one other customer.
//! * `component_size`: customers reachable at any depth, including the start customer. Capped at
//!   [`limits::COMPONENT_CAP`] nodes and [`limits::COMPONENT_MAX_LAYERS`] layers.
//! * `community_fraud_rate`: fraud rate of the customer's Louvain community (ml-service).

use contracts::graph::{GraphMetric, GraphMetrics, LinkKind};
use platform::AppResult;

use super::explore::Exploration;
use super::model::{limits, CustomerId, TraversalParams};
use super::ports::GraphStore;

/// Computes every pipeline metric in one traversal (continued until the component is known).
pub async fn compute_metrics<S: GraphStore + ?Sized>(
    store: &mut S,
    start: CustomerId,
    params: &TraversalParams,
) -> AppResult<GraphMetrics> {
    let mut ex = Exploration::new(start);
    let near_depth = params.max_depth.max(2);
    while ex.can_continue() && ex.depth < near_depth {
        ex.step(store, params, limits::COMPONENT_CAP, false).await?;
    }

    let distance_to_fraud = ex.nearest_fraud(params.max_depth).map(|(_, d)| d);
    let fraud_neighbors_1 = count_u32(ex.fraud_within(1));
    let fraud_neighbors_2 = count_u32(ex.fraud_within(2));
    let degree = count_u32(ex.at_depth(1).count());
    let shared_entity_count = shared_entities(&ex);
    let shared_with_fraud_kinds = shared_with_fraud_kinds(&ex);

    while ex.can_continue() && ex.depth < limits::COMPONENT_MAX_LAYERS {
        ex.step(store, params, limits::COMPONENT_CAP, false).await?;
    }
    let component_size = count_u32(ex.visits.len().min(limits::COMPONENT_CAP));
    let community_fraud_rate = store.community_fraud_rate(start).await?;

    metrics::histogram!("graph_traversal_layers").record(f64::from(ex.depth));
    Ok(GraphMetrics {
        distance_to_fraud,
        fraud_neighbors_1,
        fraud_neighbors_2,
        component_size,
        shared_entity_count,
        degree,
        community_fraud_rate,
        shared_with_fraud_kinds,
    })
}

/// Computes a single graph-rule metric with the rule's own parameters.
/// Returns `None` for "+∞ distance" or "community not available".
pub async fn compute_single<S: GraphStore + ?Sized>(
    store: &mut S,
    start: CustomerId,
    metric: GraphMetric,
    params: &TraversalParams,
) -> AppResult<Option<f64>> {
    let mut ex = Exploration::new(start);
    let value = match metric {
        GraphMetric::DistanceToFraud => {
            while ex.can_continue() && ex.depth < params.max_depth {
                let out = ex.step(store, params, limits::METRICS_NODE_LIMIT, false).await?;
                if out.new_fraud > 0 {
                    break;
                }
            }
            ex.nearest_fraud(params.max_depth).map(|(_, d)| f64::from(d))
        }
        GraphMetric::FraudNeighbors => {
            while ex.can_continue() && ex.depth < params.max_depth {
                ex.step(store, params, limits::METRICS_NODE_LIMIT, false).await?;
            }
            Some(ex.fraud_within(params.max_depth) as f64)
        }
        GraphMetric::Degree => {
            ex.step(store, params, limits::METRICS_NODE_LIMIT, false).await?;
            Some(ex.at_depth(1).count() as f64)
        }
        GraphMetric::SharedEntityCount => {
            ex.step(store, params, limits::METRICS_NODE_LIMIT, false).await?;
            Some(f64::from(shared_entities(&ex)))
        }
        GraphMetric::ComponentSize => {
            while ex.can_continue() && ex.depth < limits::COMPONENT_MAX_LAYERS {
                ex.step(store, params, limits::COMPONENT_CAP, false).await?;
            }
            Some(ex.visits.len().min(limits::COMPONENT_CAP) as f64)
        }
        GraphMetric::CommunityFraudRate => store.community_fraud_rate(start).await?,
    };
    Ok(value)
}

fn shared_entities(ex: &Exploration) -> u32 {
    let mut ids: Vec<i64> = ex
        .start_entities
        .iter()
        .filter(|e| e.customer_count >= 2)
        .map(|e| e.id)
        .collect();
    ids.sort_unstable();
    ids.dedup();
    count_u32(ids.len())
}

/// Kinds of the start customer's entities through which a fraud customer is reached in one hop.
fn shared_with_fraud_kinds(ex: &Exploration) -> Vec<LinkKind> {
    let mut kinds: Vec<LinkKind> = Vec::new();
    for (_, v) in ex.at_depth(1).filter(|(_, v)| v.is_fraud) {
        if let Some((_, via)) = &v.parent {
            if let Some(e) = ex.entities.get(&via.entity) {
                if !kinds.contains(&e.kind) {
                    kinds.push(e.kind);
                }
            }
        }
    }
    kinds.sort_by_key(|k| k.as_str());
    kinds
}

fn count_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::domain::memory::MemoryGraph;
    use pretty_assertions::assert_eq;

    fn no_sim() -> TraversalParams {
        TraversalParams {
            include_similar: false,
            ..TraversalParams::default()
        }
    }

    /// a —card— f1(fraud) ; a —device— b —phone— f2(fraud) ; plus a supernode IP shared by all + f3(fraud)
    fn graph() -> (MemoryGraph, CustomerId, CustomerId) {
        let mut g = MemoryGraph::default();
        let a = g.customer(false);
        let f1 = g.customer(true);
        let b = g.customer(false);
        let f2 = g.customer(true);
        let card = g.entity(LinkKind::Card);
        let dev = g.entity(LinkKind::Device);
        let phone = g.entity(LinkKind::Phone);
        g.link(a, card);
        g.link(f1, card);
        g.link(a, dev);
        g.link(b, dev);
        g.link(b, phone);
        g.link(f2, phone);
        (g, a, b)
    }

    #[tokio::test]
    async fn pipeline_metrics() {
        let (mut g, a, _) = graph();
        let m = compute_metrics(&mut g, a, &no_sim()).await.unwrap();
        assert_eq!(m.distance_to_fraud, Some(1));
        assert_eq!(m.fraud_neighbors_1, 1);
        assert_eq!(m.fraud_neighbors_2, 2);
        assert_eq!(m.degree, 2);
        assert_eq!(m.shared_entity_count, 2);
        assert_eq!(m.component_size, 4);
        assert_eq!(m.shared_with_fraud_kinds, vec![LinkKind::Card]);
        assert_eq!(m.community_fraud_rate, None);
    }

    #[tokio::test]
    async fn supernodes_are_skipped() {
        let mut g = MemoryGraph::default();
        let a = g.customer(false);
        let fraud = g.customer(true);
        let ip = g.entity(LinkKind::Device);
        g.link(a, ip);
        g.link(fraud, ip);
        for _ in 0..10 {
            let x = g.customer(false);
            g.link(x, ip);
        }
        let p = TraversalParams {
            supernode_cap: 5,
            ..no_sim()
        };
        let m = compute_metrics(&mut g, a, &p).await.unwrap();
        assert_eq!(m.distance_to_fraud, None);
        assert_eq!(m.component_size, 1);
        assert_eq!(m.shared_entity_count, 0);
    }

    #[tokio::test]
    async fn similarity_edges_only_when_included_and_above_threshold() {
        let mut g = MemoryGraph::default();
        let a = g.customer(false);
        let fraud = g.customer(true);
        let p1 = g.entity(LinkKind::Phone);
        let p2 = g.entity(LinkKind::Phone);
        g.link(a, p1);
        g.link(fraud, p2);
        g.similar(p1, p2, 0.9);

        let off = compute_single(&mut g, a, GraphMetric::DistanceToFraud, &no_sim())
            .await
            .unwrap();
        assert_eq!(off, None);

        let on = TraversalParams::default();
        let v = compute_single(&mut g, a, GraphMetric::DistanceToFraud, &on)
            .await
            .unwrap();
        assert_eq!(v, Some(1.0));

        let strict = TraversalParams {
            similarity_threshold: 0.95,
            ..TraversalParams::default()
        };
        let v = compute_single(&mut g, a, GraphMetric::DistanceToFraud, &strict)
            .await
            .unwrap();
        assert_eq!(v, None);
    }

    #[tokio::test]
    async fn single_metrics_and_depth_limit() {
        let (mut g, a, b) = graph();
        let p = TraversalParams {
            max_depth: 1,
            ..no_sim()
        };
        assert_eq!(
            compute_single(&mut g, a, GraphMetric::FraudNeighbors, &p)
                .await
                .unwrap(),
            Some(1.0)
        );
        assert_eq!(
            compute_single(&mut g, b, GraphMetric::Degree, &p).await.unwrap(),
            Some(2.0)
        );
        assert_eq!(
            compute_single(&mut g, a, GraphMetric::ComponentSize, &p)
                .await
                .unwrap(),
            Some(4.0)
        );
        g.set_community_rate(a, 0.25);
        assert_eq!(
            compute_single(&mut g, a, GraphMetric::CommunityFraudRate, &p)
                .await
                .unwrap(),
            Some(0.25)
        );
    }

    #[tokio::test]
    async fn component_is_capped() {
        let mut g = MemoryGraph::default();
        let start = g.customer(false);
        let dev = g.entity(LinkKind::Device);
        g.link(start, dev);
        // a long chain larger than the cap is bounded by COMPONENT_CAP / layers
        let mut prev = start;
        for _ in 0..40 {
            let e = g.entity(LinkKind::Card);
            let c = g.customer(false);
            g.link(prev, e);
            g.link(c, e);
            prev = c;
        }
        let v = compute_single(&mut g, start, GraphMetric::ComponentSize, &no_sim())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(v, f64::from(limits::COMPONENT_MAX_LAYERS) + 1.0);
    }
}
