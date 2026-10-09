//! Connected components over the customer projection (UI "components" view).
//!
//! Computed with a union–find (disjoint-set) over customer–customer edges. Union by size plus path
//! halving makes it near-linear, which is fine for a project-wide recomputation that is cached
//! for 5 minutes.

use std::collections::{HashMap, HashSet};

use serde::Serialize;
use utoipa::ToSchema;

use super::model::CustomerId;

/// Maximum sample customers returned per component.
pub const SAMPLE_SIZE: usize = 10;

#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct Component {
    /// Stable id: the smallest customer id in the component.
    pub component_id: uuid::Uuid,
    pub size: u32,
    pub fraud_count: u32,
    pub fraud_rate: f64,
    pub sample_customer_ids: Vec<uuid::Uuid>,
}

#[derive(Debug, Default)]
struct DisjointSet {
    index: HashMap<CustomerId, usize>,
    ids: Vec<CustomerId>,
    parent: Vec<usize>,
    size: Vec<u32>,
}

impl DisjointSet {
    fn node(&mut self, c: CustomerId) -> usize {
        if let Some(i) = self.index.get(&c) {
            return *i;
        }
        let i = self.ids.len();
        self.index.insert(c, i);
        self.ids.push(c);
        self.parent.push(i);
        self.size.push(1);
        i
    }

    fn find(&mut self, mut i: usize) -> usize {
        while self.parent[i] != i {
            self.parent[i] = self.parent[self.parent[i]];
            i = self.parent[i];
        }
        i
    }

    fn union(&mut self, a: usize, b: usize) {
        let (mut ra, mut rb) = (self.find(a), self.find(b));
        if ra == rb {
            return;
        }
        if self.size[ra] < self.size[rb] {
            std::mem::swap(&mut ra, &mut rb);
        }
        self.parent[rb] = ra;
        self.size[ra] += self.size[rb];
    }
}

/// Builds components (size ≥ 2) from customer–customer edges, sorted by fraud count then size.
pub fn build_components(
    edges: impl IntoIterator<Item = (CustomerId, CustomerId)>,
    fraud: &HashSet<CustomerId>,
) -> Vec<Component> {
    let mut ds = DisjointSet::default();
    for (a, b) in edges {
        if a == b {
            continue;
        }
        let (ia, ib) = (ds.node(a), ds.node(b));
        ds.union(ia, ib);
    }
    let mut groups: HashMap<usize, Vec<CustomerId>> = HashMap::new();
    for i in 0..ds.ids.len() {
        let root = ds.find(i);
        groups.entry(root).or_default().push(ds.ids[i]);
    }
    let mut out: Vec<Component> = groups
        .into_values()
        .filter(|members| members.len() >= 2)
        .map(|mut members| {
            members.sort_unstable();
            let size = u32::try_from(members.len()).unwrap_or(u32::MAX);
            let fraud_count =
                u32::try_from(members.iter().filter(|c| fraud.contains(c)).count()).unwrap_or(u32::MAX);
            let mut sample: Vec<CustomerId> = members.iter().filter(|c| fraud.contains(c)).copied().collect();
            sample.extend(members.iter().filter(|c| !fraud.contains(c)).copied());
            sample.truncate(SAMPLE_SIZE);
            Component {
                component_id: members[0],
                size,
                fraud_count,
                fraud_rate: f64::from(fraud_count) / f64::from(size),
                sample_customer_ids: sample,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.fraud_count
            .cmp(&a.fraud_count)
            .then(b.size.cmp(&a.size))
            .then(a.component_id.cmp(&b.component_id))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use uuid::Uuid;

    #[test]
    fn groups_edges_into_components() {
        let ids: Vec<Uuid> = (0..6).map(|_| Uuid::new_v4()).collect();
        let edges = vec![
            (ids[0], ids[1]),
            (ids[1], ids[2]),
            (ids[3], ids[4]),
            (ids[5], ids[5]),
        ];
        let fraud: HashSet<Uuid> = [ids[4]].into_iter().collect();
        let comps = build_components(edges, &fraud);
        assert_eq!(comps.len(), 2);
        // component with fraud first despite being smaller
        assert_eq!(comps[0].size, 2);
        assert_eq!(comps[0].fraud_count, 1);
        assert!((comps[0].fraud_rate - 0.5).abs() < 1e-9);
        assert_eq!(comps[0].sample_customer_ids[0], ids[4]);
        assert_eq!(comps[1].size, 3);
        assert_eq!(comps[1].fraud_count, 0);
    }
}
