//! The **port**: what the graph algorithms need from storage.
//!
//! In Java this would be `interface GraphRepository`. Methods take `&mut self` because the
//! Postgres adapter wraps a transaction (one per request, tenant-scoped via RLS), and a
//! transaction is a single, mutable connection.
//!
//! Each method takes a **batch** of ids. The BFS asks for a whole layer at once, so a depth-3
//! traversal costs about 2–3 queries per layer instead of one per node. That is what keeps p95
//! latency in the tens of milliseconds.

use async_trait::async_trait;
use contracts::graph::LinkKind;
use platform::AppResult;

use super::model::{CustomerId, CustomerInfo, EntityId, EntityRef};

/// Filters applied to entity expansion.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityQuery<'a> {
    pub kinds: &'a [LinkKind],
    /// Skip entities linked to more customers than this (supernodes).
    pub supernode_cap: u32,
    /// Only entities shared by ≥ 2 customers (the only useful ones when similarity is off).
    pub only_shared: bool,
    /// Minimum similarity score for [`GraphStore::similar_entities`].
    pub min_similarity: f32,
}

/// `customer —has→ entity`
#[derive(Debug, Clone, PartialEq)]
pub struct EntityLink {
    pub customer_id: CustomerId,
    pub entity: EntityRef,
}

/// `entity ←has— customer`, with the customer's fraud label.
#[derive(Debug, Clone, PartialEq)]
pub struct CustomerLink {
    pub entity_id: EntityId,
    pub customer_id: CustomerId,
    pub is_fraud: bool,
}

/// `from ~similar~ to`
#[derive(Debug, Clone, PartialEq)]
pub struct SimilarEntity {
    pub from: EntityId,
    pub to: EntityRef,
    pub score: f32,
}

#[async_trait]
pub trait GraphStore: Send {
    /// Entities of the given customers matching the query (supernodes excluded).
    async fn entities_of(
        &mut self,
        customers: &[CustomerId],
        q: &EntityQuery<'_>,
    ) -> AppResult<Vec<EntityLink>>;

    /// Customers linked to the given entities.
    async fn customers_of(&mut self, entities: &[EntityId]) -> AppResult<Vec<CustomerLink>>;

    /// Similarity counterparts of the given entities with score ≥ `q.min_similarity`
    /// (counterpart supernodes and disallowed kinds excluded).
    async fn similar_entities(
        &mut self,
        entities: &[EntityId],
        q: &EntityQuery<'_>,
    ) -> AppResult<Vec<SimilarEntity>>;

    /// Fraud rate of the customer's Louvain community (computed by ml-service), if any.
    async fn community_fraud_rate(&mut self, customer: CustomerId) -> AppResult<Option<f64>>;

    /// Display attributes of customers (for UI responses).
    async fn customers_info(&mut self, customers: &[CustomerId]) -> AppResult<Vec<CustomerInfo>>;
}
