//! Postgres implementation of [`GraphStore`] plus the write/query functions used by the use cases.
//!
//! Performance notes:
//! * Every traversal query takes an **array** of ids (`= ANY($n)`), so one BFS layer costs one
//!   round trip per query type, not one per node. The indexes that serve them are
//!   `entity_links(customer_id, entity_id)` (PK), `entity_links(entity_id)`, `entities(id)` and
//!   `entity_similarity(entity_a, entity_b)` / `(entity_b)`.
//! * Supernodes are filtered with `entities.customer_count`, maintained at ingest, so no
//!   `count(*)` runs at query time.
//! * Upserts use `INSERT … SELECT FROM unnest(…)` (one statement per table), with keys sorted
//!   so concurrent ingests always lock rows in the same order and cannot deadlock.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use contracts::graph::{GraphCustomer, LinkKind};
use platform::db::TenantTx;
use platform::{AppResult, ProjectId, TenantId};
use sqlx::Row;
use uuid::Uuid;

use crate::domain::model::{CustomerId, CustomerInfo, EntityId, EntityKey, EntityRef};
use crate::domain::ports::{CustomerLink, EntityLink, EntityQuery, GraphStore, SimilarEntity};

/// Read adapter bound to one tenant transaction and one project.
#[derive(Debug)]
pub struct PgGraphStore<'a> {
    tx: &'a mut TenantTx<'static>,
    project: Uuid,
}

impl<'a> PgGraphStore<'a> {
    pub fn new(tx: &'a mut TenantTx<'static>, project: ProjectId) -> Self {
        Self {
            tx,
            project: project.as_uuid(),
        }
    }
}

fn kinds_param(kinds: &[LinkKind]) -> Vec<String> {
    kinds.iter().map(|k| k.as_str().to_string()).collect()
}

fn to_u32(v: i32) -> u32 {
    u32::try_from(v).unwrap_or(0)
}

fn cap_param(cap: u32) -> i32 {
    i32::try_from(cap).unwrap_or(i32::MAX)
}

fn entity_ref(row: &sqlx::postgres::PgRow, id_col: &str) -> Option<EntityRef> {
    let kind: String = row.try_get("kind").ok()?;
    Some(EntityRef {
        id: row.try_get(id_col).ok()?,
        kind: LinkKind::parse(&kind)?,
        display: row.try_get("display_value").ok()?,
        customer_count: to_u32(row.try_get("customer_count").ok()?),
    })
}

#[async_trait]
impl GraphStore for PgGraphStore<'_> {
    async fn entities_of(
        &mut self,
        customers: &[CustomerId],
        q: &EntityQuery<'_>,
    ) -> AppResult<Vec<EntityLink>> {
        if customers.is_empty() || q.kinds.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query(
            "SELECT l.customer_id, e.id, e.kind, e.display_value, e.customer_count
             FROM graph.entity_links l
             JOIN graph.entities e ON e.id = l.entity_id
             WHERE l.project_id = $1 AND l.customer_id = ANY($2)
               AND e.kind = ANY($3) AND e.customer_count <= $4 AND e.customer_count >= $5",
        )
        .bind(self.project)
        .bind(customers)
        .bind(kinds_param(q.kinds))
        .bind(cap_param(q.supernode_cap))
        .bind(if q.only_shared { 2_i32 } else { 1_i32 })
        .fetch_all(&mut ***self.tx)
        .await?;
        Ok(rows
            .iter()
            .filter_map(|r| {
                Some(EntityLink {
                    customer_id: r.try_get("customer_id").ok()?,
                    entity: entity_ref(r, "id")?,
                })
            })
            .collect())
    }

    async fn customers_of(&mut self, entities: &[EntityId]) -> AppResult<Vec<CustomerLink>> {
        if entities.is_empty() {
            return Ok(Vec::new());
        }
        let rows: Vec<(i64, Uuid, bool)> = sqlx::query_as(
            "SELECT l.entity_id, l.customer_id, (c.risk_label = 'fraud')
             FROM graph.entity_links l
             JOIN graph.nodes_customer c ON c.customer_id = l.customer_id
             WHERE l.project_id = $1 AND l.entity_id = ANY($2)",
        )
        .bind(self.project)
        .bind(entities)
        .fetch_all(&mut ***self.tx)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(entity_id, customer_id, is_fraud)| CustomerLink {
                entity_id,
                customer_id,
                is_fraud,
            })
            .collect())
    }

    async fn similar_entities(
        &mut self,
        entities: &[EntityId],
        q: &EntityQuery<'_>,
    ) -> AppResult<Vec<SimilarEntity>> {
        if entities.is_empty() || q.kinds.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query(
            "SELECT s.entity_a AS from_id, e.id, e.kind, e.display_value, e.customer_count, s.score
             FROM graph.entity_similarity s JOIN graph.entities e ON e.id = s.entity_b
             WHERE s.project_id = $1 AND s.entity_a = ANY($2) AND s.score >= $3
               AND e.kind = ANY($4) AND e.customer_count <= $5
             UNION ALL
             SELECT s.entity_b AS from_id, e.id, e.kind, e.display_value, e.customer_count, s.score
             FROM graph.entity_similarity s JOIN graph.entities e ON e.id = s.entity_a
             WHERE s.project_id = $1 AND s.entity_b = ANY($2) AND s.score >= $3
               AND e.kind = ANY($4) AND e.customer_count <= $5",
        )
        .bind(self.project)
        .bind(entities)
        .bind(q.min_similarity)
        .bind(kinds_param(q.kinds))
        .bind(cap_param(q.supernode_cap))
        .fetch_all(&mut ***self.tx)
        .await?;
        Ok(rows
            .iter()
            .filter_map(|r| {
                Some(SimilarEntity {
                    from: r.try_get("from_id").ok()?,
                    to: entity_ref(r, "id")?,
                    score: r.try_get("score").ok()?,
                })
            })
            .collect())
    }

    async fn community_fraud_rate(&mut self, customer: CustomerId) -> AppResult<Option<f64>> {
        let row: Option<(f64,)> = sqlx::query_as(
            "SELECT s.fraud_rate::float8
             FROM ml.graph_communities c
             JOIN ml.graph_community_stats s
               ON s.project_id = c.project_id AND s.community_id = c.community_id
             WHERE c.customer_id = $1 AND c.project_id = $2",
        )
        .bind(customer)
        .bind(self.project)
        .fetch_optional(&mut ***self.tx)
        .await?;
        Ok(row.map(|(r,)| r))
    }

    async fn customers_info(&mut self, customers: &[CustomerId]) -> AppResult<Vec<CustomerInfo>> {
        if customers.is_empty() {
            return Ok(Vec::new());
        }
        let rows: Vec<(Uuid, String, String)> = sqlx::query_as(
            "SELECT customer_id, external_id, risk_label FROM graph.nodes_customer
             WHERE project_id = $1 AND customer_id = ANY($2)",
        )
        .bind(self.project)
        .bind(customers)
        .fetch_all(&mut ***self.tx)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, external_id, risk_label)| CustomerInfo {
                id,
                external_id,
                risk_label,
            })
            .collect())
    }
}

// ---------------------------------------------------------------------------------------------
// Writes (entity resolution)
// ---------------------------------------------------------------------------------------------

/// Upserts the customer node (label mirrored from core-api).
pub async fn upsert_customer(
    tx: &mut TenantTx<'static>,
    tenant: TenantId,
    project: ProjectId,
    c: &GraphCustomer,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO graph.nodes_customer (customer_id, tenant_id, project_id, external_id, risk_label)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (customer_id) DO UPDATE
         SET external_id = EXCLUDED.external_id, risk_label = EXCLUDED.risk_label, updated_at = now()",
    )
    .bind(c.id)
    .bind(tenant.as_uuid())
    .bind(project.as_uuid())
    .bind(&c.external_id)
    .bind(&c.risk_label)
    .execute(&mut ***tx)
    .await?;
    Ok(())
}

/// Result of an entity upsert.
#[derive(Debug, Clone, PartialEq)]
pub struct UpsertedEntity {
    pub id: EntityId,
    pub kind: LinkKind,
    pub value: String,
    pub inserted: bool,
}

/// Upserts entities (keys must be deduplicated; they are sorted here) and returns their ids.
pub async fn upsert_entities(
    tx: &mut TenantTx<'static>,
    tenant: TenantId,
    project: ProjectId,
    keys: &[EntityKey],
    seen_at: DateTime<Utc>,
) -> AppResult<Vec<UpsertedEntity>> {
    if keys.is_empty() {
        return Ok(Vec::new());
    }
    let mut sorted: Vec<&EntityKey> = keys.iter().collect();
    sorted.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    let kinds: Vec<&str> = sorted.iter().map(|k| k.kind.as_str()).collect();
    let values: Vec<&str> = sorted.iter().map(|k| k.value.as_str()).collect();
    let displays: Vec<&str> = sorted.iter().map(|k| k.display.as_str()).collect();
    let rows: Vec<(i64, String, String, bool)> = sqlx::query_as(
        "INSERT INTO graph.entities
             (tenant_id, project_id, kind, value_normalized, display_value, first_seen_at, last_seen_at)
         SELECT $1, $2, t.kind, t.value, t.display, $3, $3
         FROM unnest($4::text[], $5::text[], $6::text[]) WITH ORDINALITY AS t(kind, value, display, ord)
         ORDER BY t.ord
         ON CONFLICT (project_id, kind, value_normalized) DO UPDATE
         SET last_seen_at = GREATEST(graph.entities.last_seen_at, EXCLUDED.last_seen_at),
             display_value = EXCLUDED.display_value
         RETURNING id, kind, value_normalized, (xmax = 0)",
    )
    .bind(tenant.as_uuid())
    .bind(project.as_uuid())
    .bind(seen_at)
    .bind(&kinds)
    .bind(&values)
    .bind(&displays)
    .fetch_all(&mut ***tx)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, kind, value, inserted)| {
            Some(UpsertedEntity {
                id,
                kind: LinkKind::parse(&kind)?,
                value,
                inserted,
            })
        })
        .collect())
}

/// Upserts `customer — entity` links; returns the entity ids whose link is **new**.
///
/// `event_count` is incremented only when the event is newer than the link's `last_seen_at`, so
/// replaying the same event (at-least-once delivery) is idempotent. Out-of-order older events
/// do not increment the counter (documented approximation).
pub async fn upsert_links(
    tx: &mut TenantTx<'static>,
    tenant: TenantId,
    project: ProjectId,
    customer: CustomerId,
    event_id: Uuid,
    entity_ids: &[EntityId],
    seen_at: DateTime<Utc>,
) -> AppResult<Vec<EntityId>> {
    if entity_ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut ids = entity_ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    let rows: Vec<(i64, bool)> = sqlx::query_as(
        "INSERT INTO graph.entity_links
             (customer_id, entity_id, tenant_id, project_id, first_event_id, first_seen_at, last_seen_at)
         SELECT $1, e, $2, $3, $4, $5, $5 FROM unnest($6::bigint[]) AS e ORDER BY e
         ON CONFLICT (customer_id, entity_id) DO UPDATE
         SET event_count = graph.entity_links.event_count
                 + CASE WHEN EXCLUDED.last_seen_at > graph.entity_links.last_seen_at THEN 1 ELSE 0 END,
             last_seen_at = GREATEST(graph.entity_links.last_seen_at, EXCLUDED.last_seen_at)
         RETURNING entity_id, (xmax = 0)",
    )
    .bind(customer)
    .bind(tenant.as_uuid())
    .bind(project.as_uuid())
    .bind(event_id)
    .bind(seen_at)
    .bind(&ids)
    .fetch_all(&mut ***tx)
    .await?;
    let new_ids: Vec<EntityId> = rows
        .into_iter()
        .filter(|(_, ins)| *ins)
        .map(|(id, _)| id)
        .collect();
    if !new_ids.is_empty() {
        sqlx::query("UPDATE graph.entities SET customer_count = customer_count + 1 WHERE id = ANY($1)")
            .bind(&new_ids)
            .execute(&mut ***tx)
            .await?;
    }
    Ok(new_ids)
}

/// Phone candidates sharing the last 9 digits or trigram-similar (indexed prefilter).
pub async fn phone_candidates(
    tx: &mut TenantTx<'static>,
    project: ProjectId,
    entity_id: EntityId,
    phone: &str,
    suffix: &str,
    limit: i64,
) -> AppResult<Vec<(EntityId, String)>> {
    let pattern = format!("%{}", crate::domain::similarity::escape_like(suffix));
    let rows = sqlx::query_as(
        "SELECT id, value_normalized FROM graph.entities
         WHERE project_id = $1 AND kind = 'phone' AND id <> $2
           AND (value_normalized LIKE $3 ESCAPE '\\' OR value_normalized % $4)
         LIMIT $5",
    )
    .bind(project.as_uuid())
    .bind(entity_id)
    .bind(pattern)
    .bind(phone)
    .bind(limit)
    .fetch_all(&mut ***tx)
    .await?;
    Ok(rows)
}

/// Email candidates with the same local part (trigram-indexed `LIKE 'local@%'`).
pub async fn email_candidates(
    tx: &mut TenantTx<'static>,
    project: ProjectId,
    entity_id: EntityId,
    local: &str,
    limit: i64,
) -> AppResult<Vec<(EntityId, String)>> {
    let pattern = format!("{}@%", crate::domain::similarity::escape_like(local));
    let rows = sqlx::query_as(
        "SELECT id, value_normalized FROM graph.entities
         WHERE project_id = $1 AND kind = 'email' AND id <> $2 AND value_normalized LIKE $3 ESCAPE '\\'
         LIMIT $4",
    )
    .bind(project.as_uuid())
    .bind(entity_id)
    .bind(pattern)
    .bind(limit)
    .fetch_all(&mut ***tx)
    .await?;
    Ok(rows)
}

/// Address candidates with pg_trgm similarity ≥ `threshold`.
pub async fn address_candidates(
    tx: &mut TenantTx<'static>,
    project: ProjectId,
    entity_id: EntityId,
    address: &str,
    threshold: f32,
    limit: i64,
) -> AppResult<Vec<(EntityId, f32)>> {
    let rows: Vec<(EntityId, f32)> = sqlx::query_as(
        "SELECT id, similarity(value_normalized, $3)::real AS s FROM graph.entities
         WHERE project_id = $1 AND kind = 'address' AND id <> $2 AND value_normalized % $3
         ORDER BY s DESC
         LIMIT $4",
    )
    .bind(project.as_uuid())
    .bind(entity_id)
    .bind(address)
    .bind(limit)
    .fetch_all(&mut ***tx)
    .await?;
    Ok(rows.into_iter().filter(|(_, s)| *s >= threshold).collect())
}

/// A similarity edge to insert (`a < b`).
#[derive(Debug, Clone, PartialEq)]
pub struct SimilarityEdge {
    pub a: EntityId,
    pub b: EntityId,
    pub score: f32,
    pub method: &'static str,
}

/// Inserts similarity edges; returns how many were new.
pub async fn insert_similarity(
    tx: &mut TenantTx<'static>,
    tenant: TenantId,
    project: ProjectId,
    edges: &[SimilarityEdge],
) -> AppResult<u32> {
    if edges.is_empty() {
        return Ok(0);
    }
    let mut edges = edges.to_vec();
    edges.sort_by_key(|e| (e.a, e.b));
    edges.dedup_by_key(|e| (e.a, e.b));
    let a: Vec<i64> = edges.iter().map(|e| e.a).collect();
    let b: Vec<i64> = edges.iter().map(|e| e.b).collect();
    let s: Vec<f32> = edges.iter().map(|e| e.score).collect();
    let m: Vec<&str> = edges.iter().map(|e| e.method).collect();
    let res = sqlx::query(
        "INSERT INTO graph.entity_similarity (entity_a, entity_b, tenant_id, project_id, score, method)
         SELECT t.a, t.b, $1, $2, t.s, t.m FROM unnest($3::bigint[], $4::bigint[], $5::real[], $6::text[])
             AS t(a, b, s, m)
         ON CONFLICT (entity_a, entity_b) DO NOTHING",
    )
    .bind(tenant.as_uuid())
    .bind(project.as_uuid())
    .bind(&a)
    .bind(&b)
    .bind(&s)
    .bind(&m)
    .execute(&mut ***tx)
    .await?;
    Ok(u32::try_from(res.rows_affected()).unwrap_or(u32::MAX))
}

/// Updates the mirrored risk label. Returns `false` when the customer has no graph node yet.
pub async fn set_label(
    tx: &mut TenantTx<'static>,
    project: ProjectId,
    customer: CustomerId,
    label: &str,
) -> AppResult<bool> {
    let res = sqlx::query(
        "UPDATE graph.nodes_customer SET risk_label = $3, updated_at = now()
         WHERE project_id = $1 AND customer_id = $2",
    )
    .bind(project.as_uuid())
    .bind(customer)
    .bind(label)
    .execute(&mut ***tx)
    .await?;
    Ok(res.rows_affected() > 0)
}

/// Raw `graph_config` of a project (`None` when the project is not visible to this tenant).
pub async fn project_graph_config(
    tx: &mut TenantTx<'static>,
    project: ProjectId,
) -> AppResult<Option<serde_json::Value>> {
    let row: Option<(serde_json::Value,)> =
        sqlx::query_as("SELECT graph_config FROM core.projects WHERE id = $1")
            .bind(project.as_uuid())
            .fetch_optional(&mut ***tx)
            .await?;
    Ok(row.map(|(v,)| v))
}

// ---------------------------------------------------------------------------------------------
// Read models for UI endpoints
// ---------------------------------------------------------------------------------------------

/// `(customers, entities, links, similarity_links, fraud_customers)`
pub async fn counts(tx: &mut TenantTx<'static>, project: ProjectId) -> AppResult<(i64, i64, i64, i64, i64)> {
    let row = sqlx::query_as(
        "SELECT (SELECT count(*) FROM graph.nodes_customer WHERE project_id = $1),
                (SELECT count(*) FROM graph.entities WHERE project_id = $1),
                (SELECT count(*) FROM graph.entity_links WHERE project_id = $1),
                (SELECT count(*) FROM graph.entity_similarity WHERE project_id = $1),
                (SELECT count(*) FROM graph.nodes_customer WHERE project_id = $1 AND risk_label = 'fraud')",
    )
    .bind(project.as_uuid())
    .fetch_one(&mut ***tx)
    .await?;
    Ok(row)
}

/// Top entities above the supernode cap.
pub async fn supernodes(
    tx: &mut TenantTx<'static>,
    project: ProjectId,
    cap: u32,
    limit: i64,
) -> AppResult<Vec<EntityRef>> {
    let rows = sqlx::query(
        "SELECT id, kind, display_value, customer_count FROM graph.entities
         WHERE project_id = $1 AND customer_count > $2
         ORDER BY customer_count DESC, id LIMIT $3",
    )
    .bind(project.as_uuid())
    .bind(cap_param(cap))
    .bind(limit)
    .fetch_all(&mut ***tx)
    .await?;
    Ok(rows.iter().filter_map(|r| entity_ref(r, "id")).collect())
}

/// Customers whose external id starts with `prefix`.
pub async fn search_customers(
    tx: &mut TenantTx<'static>,
    project: ProjectId,
    prefix: &str,
    limit: i64,
) -> AppResult<Vec<CustomerInfo>> {
    let pattern = format!("{}%", crate::domain::similarity::escape_like(prefix));
    let rows: Vec<(Uuid, String, String)> = sqlx::query_as(
        "SELECT customer_id, external_id, risk_label FROM graph.nodes_customer
         WHERE project_id = $1 AND external_id ILIKE $2 ESCAPE '\\'
         ORDER BY external_id LIMIT $3",
    )
    .bind(project.as_uuid())
    .bind(pattern)
    .bind(limit)
    .fetch_all(&mut ***tx)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, external_id, risk_label)| CustomerInfo {
            id,
            external_id,
            risk_label,
        })
        .collect())
}

/// Entities whose normalised value equals one of `values` (non-hashed kinds only), with up to 10
/// linked customers each.
pub async fn search_entities(
    tx: &mut TenantTx<'static>,
    project: ProjectId,
    values: &[String],
    limit: i64,
) -> AppResult<Vec<(EntityRef, Vec<Uuid>)>> {
    if values.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query(
        "SELECT e.id, e.kind, e.display_value, e.customer_count,
                ARRAY(SELECT l.customer_id FROM graph.entity_links l
                      WHERE l.entity_id = e.id ORDER BY l.last_seen_at DESC LIMIT 10) AS customers
         FROM graph.entities e
         WHERE e.project_id = $1 AND e.value_normalized = ANY($2)
           AND e.kind IN ('email', 'phone', 'ip', 'device', 'ref_transaction', 'api_client')
         ORDER BY e.customer_count DESC LIMIT $3",
    )
    .bind(project.as_uuid())
    .bind(values)
    .bind(limit)
    .fetch_all(&mut ***tx)
    .await?;
    Ok(rows
        .iter()
        .filter_map(|r| Some((entity_ref(r, "id")?, r.try_get::<Vec<Uuid>, _>("customers").ok()?)))
        .collect())
}

/// `(entity_id, customer_id)` links of shareable entities, ordered by entity (for union–find).
pub async fn component_links(
    tx: &mut TenantTx<'static>,
    project: ProjectId,
    kinds: &[LinkKind],
    cap: u32,
) -> AppResult<Vec<(EntityId, Uuid)>> {
    let rows = sqlx::query_as(
        "SELECT l.entity_id, l.customer_id
         FROM graph.entity_links l JOIN graph.entities e ON e.id = l.entity_id
         WHERE l.project_id = $1 AND e.kind = ANY($2) AND e.customer_count BETWEEN 2 AND $3
         ORDER BY l.entity_id",
    )
    .bind(project.as_uuid())
    .bind(kinds_param(kinds))
    .bind(cap_param(cap))
    .fetch_all(&mut ***tx)
    .await?;
    Ok(rows)
}

/// Customer pairs connected through similarity edges.
pub async fn similarity_customer_pairs(
    tx: &mut TenantTx<'static>,
    project: ProjectId,
    kinds: &[LinkKind],
    cap: u32,
    min_score: f32,
) -> AppResult<Vec<(Uuid, Uuid)>> {
    let rows = sqlx::query_as(
        "SELECT DISTINCT la.customer_id, lb.customer_id
         FROM graph.entity_similarity s
         JOIN graph.entities ea ON ea.id = s.entity_a AND ea.customer_count <= $3 AND ea.kind = ANY($2)
         JOIN graph.entities eb ON eb.id = s.entity_b AND eb.customer_count <= $3
         JOIN graph.entity_links la ON la.entity_id = s.entity_a
         JOIN graph.entity_links lb ON lb.entity_id = s.entity_b
         WHERE s.project_id = $1 AND s.score >= $4 AND la.customer_id <> lb.customer_id",
    )
    .bind(project.as_uuid())
    .bind(kinds_param(kinds))
    .bind(cap_param(cap))
    .bind(min_score)
    .fetch_all(&mut ***tx)
    .await?;
    Ok(rows)
}

/// All fraud-labelled customers of the project.
pub async fn fraud_customers(tx: &mut TenantTx<'static>, project: ProjectId) -> AppResult<Vec<Uuid>> {
    let rows: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT customer_id FROM graph.nodes_customer WHERE project_id = $1 AND risk_label = 'fraud'",
    )
    .bind(project.as_uuid())
    .fetch_all(&mut ***tx)
    .await?;
    Ok(rows.into_iter().map(|(c,)| c).collect())
}

/// SQL for the customer–customer projection exported to ml-service (Louvain).
/// `$1` project, `$2` supernode cap, `$3` kinds, `$4` min similarity (> 1 disables similarity).
pub const EXPORT_SQL: &str = "
WITH e AS (
    SELECT id FROM graph.entities
    WHERE project_id = $1 AND customer_count BETWEEN 2 AND $2 AND kind = ANY($3)
),
shared AS (
    SELECT a.customer_id AS s, b.customer_id AS t, count(*)::float8 AS w
    FROM graph.entity_links a
    JOIN graph.entity_links b ON b.entity_id = a.entity_id AND b.customer_id > a.customer_id
    WHERE a.entity_id IN (SELECT id FROM e)
    GROUP BY 1, 2
),
sim AS (
    SELECT LEAST(la.customer_id, lb.customer_id) AS s, GREATEST(la.customer_id, lb.customer_id) AS t,
           max(s.score)::float8 AS w
    FROM graph.entity_similarity s
    JOIN graph.entities ea ON ea.id = s.entity_a AND ea.customer_count <= $2 AND ea.kind = ANY($3)
    JOIN graph.entities eb ON eb.id = s.entity_b AND eb.customer_count <= $2
    JOIN graph.entity_links la ON la.entity_id = s.entity_a
    JOIN graph.entity_links lb ON lb.entity_id = s.entity_b
    WHERE s.project_id = $1 AND s.score >= $4 AND la.customer_id <> lb.customer_id
    GROUP BY 1, 2
)
SELECT s, t, sum(w)::float8 AS weight FROM (SELECT * FROM shared UNION ALL SELECT * FROM sim) x
GROUP BY s, t";
