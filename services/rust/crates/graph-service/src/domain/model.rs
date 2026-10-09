//! Value types of the graph domain.

use contracts::graph::LinkKind;
use uuid::Uuid;

pub type CustomerId = Uuid;
pub type EntityId = i64;

/// Hard limits that protect latency and memory regardless of request parameters.
pub mod limits {
    /// Maximum customer hops any traversal may use (rule-dsl allows up to 3; 4 leaves headroom).
    pub const MAX_DEPTH: u32 = 4;
    /// `component_size` is reported up to this many customers.
    pub const COMPONENT_CAP: usize = 1000;
    /// Component exploration stops after this many BFS layers (long chains).
    pub const COMPONENT_MAX_LAYERS: u32 = 25;
    /// Customers visited by metric traversals before stopping.
    pub const METRICS_NODE_LIMIT: usize = 5000;
    /// Nodes returned by the UI neighbourhood endpoint.
    pub const NEIGHBORHOOD_MAX_NODES: usize = 300;
    /// Similarity candidates considered per new entity during ingestion.
    pub const SIMILARITY_CANDIDATES: i64 = 20;
    /// Maximum stored length of an entity value.
    pub const MAX_VALUE_LEN: usize = 512;
}

/// An entity node as seen by traversals.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityRef {
    pub id: EntityId,
    pub kind: LinkKind,
    /// Masked value, safe for UI.
    pub display: String,
    /// Number of distinct customers linked to the entity.
    pub customer_count: u32,
}

/// An entity extracted from an event, before it has an id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityKey {
    pub kind: LinkKind,
    /// Normalised value (hashes for card/bank account). Unique per (project, kind).
    pub value: String,
    /// Masked display value.
    pub display: String,
}

impl EntityKey {
    /// Stable ordering key: rows are always upserted in the same order, so concurrent ingests of
    /// the same entities cannot deadlock on row locks.
    pub fn sort_key(&self) -> (&'static str, &str) {
        (self.kind.as_str(), self.value.as_str())
    }
}

/// Parameters shared by every traversal. Built from the project's `graph_config` plus request
/// overrides (see `app::project_config`).
#[derive(Debug, Clone, PartialEq)]
pub struct TraversalParams {
    pub link_kinds: Vec<LinkKind>,
    pub include_similar: bool,
    /// Customer hops (1 hop = customer → entity → customer).
    pub max_depth: u32,
    /// Entities with more customers than this are not traversed.
    pub supernode_cap: u32,
    /// Minimum similarity score for similarity edges to be followed.
    pub similarity_threshold: f32,
}

impl Default for TraversalParams {
    fn default() -> Self {
        Self {
            link_kinds: LinkKind::DEFAULT.to_vec(),
            include_similar: true,
            max_depth: 3,
            supernode_cap: 50,
            similarity_threshold: 0.85,
        }
    }
}

/// How a visited customer was reached from its parent customer.
#[derive(Debug, Clone, PartialEq)]
pub struct Via {
    /// Entity of the parent customer.
    pub entity: EntityId,
    /// When the hop used a similarity edge: the similar entity (linked to the child) and its score.
    pub similar: Option<(EntityId, f32)>,
}

/// A customer reached by a traversal.
#[derive(Debug, Clone, PartialEq)]
pub struct Visit {
    pub depth: u32,
    pub is_fraud: bool,
    /// `None` only for the start customer.
    pub parent: Option<(CustomerId, Via)>,
}

/// Customer attributes needed for display.
#[derive(Debug, Clone, PartialEq)]
pub struct CustomerInfo {
    pub id: CustomerId,
    pub external_id: String,
    pub risk_label: String,
}
