//! In-memory [`GraphStore`] for unit tests (a "fake" in test-double terms).

use std::collections::HashMap;

use async_trait::async_trait;
use contracts::graph::LinkKind;
use platform::AppResult;
use uuid::Uuid;

use super::model::{CustomerId, CustomerInfo, EntityId, EntityRef};
use super::ports::{CustomerLink, EntityLink, EntityQuery, GraphStore, SimilarEntity};

#[derive(Debug, Default)]
pub struct MemoryGraph {
    fraud: HashMap<CustomerId, bool>,
    entities: HashMap<EntityId, LinkKind>,
    links: Vec<(CustomerId, EntityId)>,
    similar: Vec<(EntityId, EntityId, f32)>,
    community: HashMap<CustomerId, f64>,
    next_entity: EntityId,
}

impl MemoryGraph {
    pub fn customer(&mut self, is_fraud: bool) -> CustomerId {
        let id = Uuid::new_v4();
        self.fraud.insert(id, is_fraud);
        id
    }

    pub fn entity(&mut self, kind: LinkKind) -> EntityId {
        self.next_entity += 1;
        self.entities.insert(self.next_entity, kind);
        self.next_entity
    }

    pub fn link(&mut self, c: CustomerId, e: EntityId) {
        self.links.push((c, e));
    }

    pub fn similar(&mut self, a: EntityId, b: EntityId, score: f32) {
        self.similar.push((a, b, score));
    }

    pub fn set_community_rate(&mut self, c: CustomerId, rate: f64) {
        self.community.insert(c, rate);
    }

    fn entity_ref(&self, id: EntityId) -> Option<EntityRef> {
        let kind = *self.entities.get(&id)?;
        let count = self.links.iter().filter(|(_, e)| *e == id).count();
        Some(EntityRef {
            id,
            kind,
            display: format!("{}-{id}", kind.as_str()),
            customer_count: u32::try_from(count).unwrap_or(u32::MAX),
        })
    }

    fn allowed(&self, e: &EntityRef, q: &EntityQuery<'_>) -> bool {
        q.kinds.contains(&e.kind) && e.customer_count <= q.supernode_cap
    }
}

#[async_trait]
impl GraphStore for MemoryGraph {
    async fn entities_of(
        &mut self,
        customers: &[CustomerId],
        q: &EntityQuery<'_>,
    ) -> AppResult<Vec<EntityLink>> {
        Ok(self
            .links
            .iter()
            .filter(|(c, _)| customers.contains(c))
            .filter_map(|(c, e)| self.entity_ref(*e).map(|r| (*c, r)))
            .filter(|(_, r)| self.allowed(r, q) && (!q.only_shared || r.customer_count >= 2))
            .map(|(customer_id, entity)| EntityLink { customer_id, entity })
            .collect())
    }

    async fn customers_of(&mut self, entities: &[EntityId]) -> AppResult<Vec<CustomerLink>> {
        Ok(self
            .links
            .iter()
            .filter(|(_, e)| entities.contains(e))
            .map(|(c, e)| CustomerLink {
                entity_id: *e,
                customer_id: *c,
                is_fraud: self.fraud.get(c).copied().unwrap_or(false),
            })
            .collect())
    }

    async fn similar_entities(
        &mut self,
        entities: &[EntityId],
        q: &EntityQuery<'_>,
    ) -> AppResult<Vec<SimilarEntity>> {
        let mut out = Vec::new();
        for (a, b, s) in &self.similar {
            if *s < q.min_similarity {
                continue;
            }
            for (from, to) in [(*a, *b), (*b, *a)] {
                if entities.contains(&from) {
                    if let Some(r) = self.entity_ref(to) {
                        if self.allowed(&r, q) {
                            out.push(SimilarEntity {
                                from,
                                to: r,
                                score: *s,
                            });
                        }
                    }
                }
            }
        }
        Ok(out)
    }

    async fn community_fraud_rate(&mut self, customer: CustomerId) -> AppResult<Option<f64>> {
        Ok(self.community.get(&customer).copied())
    }

    async fn customers_info(&mut self, customers: &[CustomerId]) -> AppResult<Vec<CustomerInfo>> {
        Ok(customers
            .iter()
            .filter_map(|c| {
                self.fraud.get(c).map(|f| CustomerInfo {
                    id: *c,
                    external_id: c.to_string()[..8].to_string(),
                    risk_label: if *f { "fraud".into() } else { "unknown".into() },
                })
            })
            .collect())
    }
}
