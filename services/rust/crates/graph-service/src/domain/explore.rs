//! Layered breadth-first exploration of the customer graph.
//!
//! One [`Exploration::step`] expands one customer hop:
//!
//! ```text
//! frontier customers ──entities_of──▶ entities ──(similar_entities)──▶ similar entities
//!                                          └──────────────┬──────────────────┘
//!                                                  customers_of ──▶ next frontier
//! ```
//!
//! That is 2 queries per layer (3 with similarity), whatever the frontier size. Steps are driven by
//! the caller, so each use case chooses its own stopping rule: stop at the first fraud customer
//! (`distance_to_fraud`), stop at depth k (`fraud_neighbors`), or keep going until the component
//! is exhausted or the node cap is reached (`component_size`).
//!
//! Every visited customer records its `parent` and the entity (or similarity pair) it was reached
//! through, so a shortest path can be rebuilt without another query (BFS guarantees the first
//! visit is along a shortest path).

use std::collections::{HashMap, HashSet};

use platform::AppResult;

use super::model::{CustomerId, EntityId, EntityRef, TraversalParams, Via, Visit};
use super::ports::{EntityQuery, GraphStore};

/// State of a traversal from one start customer.
#[derive(Debug, Clone)]
pub struct Exploration {
    pub start: CustomerId,
    pub visits: HashMap<CustomerId, Visit>,
    /// Visit order (deterministic output for UIs and tests).
    pub order: Vec<CustomerId>,
    /// Every entity seen, by id.
    pub entities: HashMap<EntityId, EntityRef>,
    /// Entities of the start customer (first layer), for `shared_entity_count`.
    pub start_entities: Vec<EntityRef>,
    /// Recorded `customer — entity` edges (only when `record_edges`).
    pub links: Vec<(CustomerId, EntityId)>,
    /// Recorded similarity edges `(from, to, score)` (only when `record_edges`).
    pub similar: Vec<(EntityId, EntityId, f32)>,
    /// Number of layers expanded so far.
    pub depth: u32,
    /// No more customers reachable.
    pub exhausted: bool,
    /// Node limit reached; results are a lower bound.
    pub truncated: bool,
    frontier: Vec<CustomerId>,
    expanded: HashSet<EntityId>,
    link_set: HashSet<(CustomerId, EntityId)>,
}

/// Result of one step.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StepOutcome {
    pub new_customers: usize,
    pub new_fraud: usize,
}

impl Exploration {
    pub fn new(start: CustomerId) -> Self {
        let mut visits = HashMap::new();
        visits.insert(
            start,
            Visit {
                depth: 0,
                is_fraud: false,
                parent: None,
            },
        );
        Self {
            start,
            visits,
            order: vec![start],
            entities: HashMap::new(),
            start_entities: Vec::new(),
            links: Vec::new(),
            similar: Vec::new(),
            depth: 0,
            exhausted: false,
            truncated: false,
            frontier: vec![start],
            expanded: HashSet::new(),
            link_set: HashSet::new(),
        }
    }

    /// `true` when another [`step`](Self::step) can discover customers.
    pub fn can_continue(&self) -> bool {
        !self.exhausted && !self.truncated
    }

    /// Expands one customer hop.
    pub async fn step<S: GraphStore + ?Sized>(
        &mut self,
        store: &mut S,
        params: &TraversalParams,
        node_limit: usize,
        record_edges: bool,
    ) -> AppResult<StepOutcome> {
        if self.frontier.is_empty() {
            self.exhausted = true;
            return Ok(StepOutcome::default());
        }
        let depth = self.depth + 1;
        let q = EntityQuery {
            kinds: &params.link_kinds,
            supernode_cap: params.supernode_cap,
            only_shared: !params.include_similar,
            min_similarity: params.similarity_threshold,
        };

        // 1. entities of the frontier (each new entity remembers the first customer that reached it)
        let mut rows = store.entities_of(&self.frontier, &q).await?;
        rows.sort_by_key(|r| (r.entity.id, r.customer_id));
        let mut via: HashMap<EntityId, (CustomerId, Via)> = HashMap::new();
        let mut to_expand: Vec<EntityId> = Vec::new();
        for r in rows {
            if record_edges {
                self.record_link(r.customer_id, r.entity.id);
            }
            if depth == 1 {
                self.start_entities.push(r.entity.clone());
            }
            if self.expanded.contains(&r.entity.id) || via.contains_key(&r.entity.id) {
                continue;
            }
            via.insert(
                r.entity.id,
                (
                    r.customer_id,
                    Via {
                        entity: r.entity.id,
                        similar: None,
                    },
                ),
            );
            to_expand.push(r.entity.id);
            self.entities.insert(r.entity.id, r.entity);
        }

        // 2. similarity counterparts
        if params.include_similar && !to_expand.is_empty() {
            let mut sims = store.similar_entities(&to_expand, &q).await?;
            sims.sort_by(|a, b| {
                (a.from, a.to.id)
                    .cmp(&(b.from, b.to.id))
                    .then(b.score.total_cmp(&a.score))
            });
            for s in sims {
                if record_edges {
                    self.similar.push((s.from, s.to.id, s.score));
                }
                if self.expanded.contains(&s.to.id) || via.contains_key(&s.to.id) {
                    continue;
                }
                let Some((origin, _)) = via.get(&s.from).cloned() else {
                    continue;
                };
                via.insert(
                    s.to.id,
                    (
                        origin,
                        Via {
                            entity: s.from,
                            similar: Some((s.to.id, s.score)),
                        },
                    ),
                );
                to_expand.push(s.to.id);
                self.entities.insert(s.to.id, s.to);
            }
        }
        self.expanded.extend(to_expand.iter().copied());

        // 3. customers of those entities → next frontier
        let mut next = Vec::new();
        let mut outcome = StepOutcome::default();
        if !to_expand.is_empty() {
            to_expand.sort_unstable();
            let mut custs = store.customers_of(&to_expand).await?;
            custs.sort_by_key(|c| (c.entity_id, c.customer_id));
            for c in custs {
                if record_edges {
                    self.record_link(c.customer_id, c.entity_id);
                }
                if let Some(v) = self.visits.get_mut(&c.customer_id) {
                    if c.customer_id == self.start {
                        v.is_fraud = c.is_fraud;
                    }
                    continue;
                }
                if self.visits.len() >= node_limit {
                    self.truncated = true;
                    continue;
                }
                let Some((parent, how)) = via.get(&c.entity_id).cloned() else {
                    continue;
                };
                self.visits.insert(
                    c.customer_id,
                    Visit {
                        depth,
                        is_fraud: c.is_fraud,
                        parent: Some((parent, how)),
                    },
                );
                self.order.push(c.customer_id);
                next.push(c.customer_id);
                outcome.new_customers += 1;
                if c.is_fraud {
                    outcome.new_fraud += 1;
                }
            }
        }

        self.depth = depth;
        self.frontier = next;
        if self.frontier.is_empty() {
            self.exhausted = true;
        }
        Ok(outcome)
    }

    fn record_link(&mut self, customer: CustomerId, entity: EntityId) {
        if self.link_set.insert((customer, entity)) {
            self.links.push((customer, entity));
        }
    }

    /// Customers (excluding the start) visited at exactly `depth`.
    pub fn at_depth(&self, depth: u32) -> impl Iterator<Item = (&CustomerId, &Visit)> {
        self.order
            .iter()
            .filter_map(|c| self.visits.get(c).map(|v| (c, v)))
            .filter(move |(c, v)| v.depth == depth && **c != self.start)
    }

    /// Fraud customers (excluding the start) within `max_depth`.
    pub fn fraud_within(&self, max_depth: u32) -> usize {
        self.visits
            .iter()
            .filter(|(c, v)| **c != self.start && v.is_fraud && v.depth >= 1 && v.depth <= max_depth)
            .count()
    }

    /// Nearest fraud customer (excluding the start) within `max_depth`: smallest depth, then visit order.
    pub fn nearest_fraud(&self, max_depth: u32) -> Option<(CustomerId, u32)> {
        self.order
            .iter()
            .filter(|c| **c != self.start)
            .filter_map(|c| self.visits.get(c).map(|v| (*c, v)))
            .filter(|(_, v)| v.is_fraud && v.depth <= max_depth)
            .min_by_key(|(_, v)| v.depth)
            .map(|(c, v)| (c, v.depth))
    }

    /// Hops from the start to `target`, following recorded parents.
    /// Each element is `(from_customer, via, to_customer)`.
    pub fn path_to(&self, target: CustomerId) -> Vec<(CustomerId, Via, CustomerId)> {
        let mut hops = Vec::new();
        let mut cur = target;
        let mut guard = 0;
        while let Some(Visit {
            parent: Some((p, via)),
            ..
        }) = self.visits.get(&cur)
        {
            hops.push((*p, via.clone(), cur));
            cur = *p;
            guard += 1;
            if guard > 64 {
                break;
            }
        }
        hops.reverse();
        hops
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::domain::memory::MemoryGraph;
    use contracts::graph::LinkKind;
    use pretty_assertions::assert_eq;

    fn params() -> TraversalParams {
        TraversalParams {
            include_similar: false,
            ..TraversalParams::default()
        }
    }

    /// a —card— b —device— c —phone— d(fraud)
    fn chain() -> (MemoryGraph, [CustomerId; 4]) {
        let mut g = MemoryGraph::default();
        let [a, b, c, d] = [
            g.customer(false),
            g.customer(false),
            g.customer(false),
            g.customer(true),
        ];
        let card = g.entity(LinkKind::Card);
        let dev = g.entity(LinkKind::Device);
        let phone = g.entity(LinkKind::Phone);
        g.link(a, card);
        g.link(b, card);
        g.link(b, dev);
        g.link(c, dev);
        g.link(c, phone);
        g.link(d, phone);
        (g, [a, b, c, d])
    }

    #[tokio::test]
    async fn layers_follow_customer_hops_and_path_is_rebuilt() {
        let (mut g, [a, b, c, d]) = chain();
        let mut ex = Exploration::new(a);
        for _ in 0..3 {
            ex.step(&mut g, &params(), 100, false).await.unwrap();
        }
        assert_eq!(ex.visits[&b].depth, 1);
        assert_eq!(ex.visits[&c].depth, 2);
        assert_eq!(ex.visits[&d].depth, 3);
        assert_eq!(ex.nearest_fraud(3), Some((d, 3)));
        assert_eq!(ex.nearest_fraud(2), None);
        let path: Vec<CustomerId> = ex.path_to(d).iter().map(|(_, _, to)| *to).collect();
        assert_eq!(path, vec![b, c, d]);
    }

    #[tokio::test]
    async fn node_limit_truncates() {
        let (mut g, [a, ..]) = chain();
        let mut ex = Exploration::new(a);
        while ex.can_continue() {
            ex.step(&mut g, &params(), 2, false).await.unwrap();
        }
        assert!(ex.truncated);
        assert_eq!(ex.visits.len(), 2);
    }

    #[tokio::test]
    async fn disallowed_kinds_are_not_traversed() {
        let (mut g, [a, b, ..]) = chain();
        let p = TraversalParams {
            link_kinds: vec![LinkKind::Device],
            ..params()
        };
        let mut ex = Exploration::new(a);
        ex.step(&mut g, &p, 100, false).await.unwrap();
        assert!(!ex.visits.contains_key(&b));
        assert!(ex.exhausted);
    }
}
