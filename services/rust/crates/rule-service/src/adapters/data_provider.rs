//! `PgDataProvider`: the Postgres + HTTP implementation of the rule engine's [`DataProvider`] port.
//!
//! ## Why a port/adapter split (the Java "interface + implementation" story, done with a trait)
//!
//! The rule engine declares **what** it needs (`velocity`, `reference_lookup`, `graph_metric`) as a trait and
//! never knows **how** it is fetched. This adapter is one implementation: SQL over `core.events` /
//! `rules.reference_*` and HTTP to graph-service. The engine's own tests use an in-memory implementation, and
//! [`super::provider_cache::CachedProvider`] is a third one that *wraps* this one (decorator pattern). In Java
//! you would write `interface DataProvider` + `@Repository class PgDataProvider implements DataProvider`; in
//! Rust it is `trait DataProvider` + `impl DataProvider for PgDataProvider`, and callers take
//! `&dyn DataProvider`, so the concrete type is swappable at runtime without inheritance.
//!
//! One adapter instance is created per request: it carries the tenant/project scope, so every query runs in a
//! [`TenantTx`] (RLS) **and** filters by `project_id` explicitly.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use contracts::graph::{GraphMetricRequest, GraphMetricResponse, LinkKind};
use platform::db::TenantTx;
use platform::http::{CallCtx, ServiceClient};
use platform::{ProjectId, TenantId};
use rule_engine::ports::{
    BucketPoint, DataProvider, GraphMetricQuery, ProviderError, RefLookup, VelocityData, VelocityQuery,
};
use serde_json::Value;
use sqlx::postgres::PgArguments;
use sqlx::query::Query;
use sqlx::{PgPool, Postgres, Row};
use uuid::Uuid;

use super::catalog::ProjectCatalog;
use super::velocity_sql::{self, Param};

/// HTTP client for graph-service metrics used by `graph` rules.
#[derive(Debug, Clone)]
pub struct GraphClient {
    client: ServiceClient,
    timeout: Duration,
}

impl GraphClient {
    pub fn new(client: ServiceClient, timeout: Duration) -> Self {
        Self { client, timeout }
    }

    pub async fn metric(
        &self,
        ctx: &CallCtx,
        req: &GraphMetricRequest,
    ) -> Result<Option<f64>, ProviderError> {
        let project = ctx
            .project
            .ok_or_else(|| ProviderError::Other("graph call without project".into()))?;
        let path = format!("/v1/projects/{project}/metric");
        let resp: GraphMetricResponse = self
            .client
            .post_json(&path, req, ctx, Some(self.timeout))
            .await
            .map_err(|e| match e {
                platform::AppError::Upstream { detail, .. } if detail.contains("timed out") => {
                    ProviderError::Timeout
                }
                other => ProviderError::Other(format!("graph-service: {other}")),
            })?;
        Ok(resp.value)
    }
}

/// Binds compiled parameters in `$n` order.
pub fn bind_params<'q>(
    mut q: Query<'q, Postgres, PgArguments>,
    params: &[Param],
) -> Query<'q, Postgres, PgArguments> {
    for p in params {
        q = match p {
            Param::Uuid(u) => q.bind(*u),
            Param::Text(s) => q.bind(s.clone()),
            Param::TextArray(v) => q.bind(v.clone()),
            Param::Timestamp(t) => q.bind(*t),
            Param::Int(i) => q.bind(*i),
            Param::Float(f) => q.bind(*f),
        };
    }
    q
}

fn provider_err(e: impl std::fmt::Display) -> ProviderError {
    ProviderError::Other(e.to_string())
}

/// Request-scoped data access for one project.
#[derive(Clone)]
pub struct PgDataProvider {
    pool: PgPool,
    tenant: TenantId,
    project: ProjectId,
    catalog: Arc<ProjectCatalog>,
    graph: Option<GraphClient>,
    call_ctx: CallCtx,
}

impl std::fmt::Debug for PgDataProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PgDataProvider")
            .field("tenant", &self.tenant)
            .field("project", &self.project)
            .finish_non_exhaustive()
    }
}

impl PgDataProvider {
    pub fn new(
        pool: PgPool,
        tenant: TenantId,
        project: ProjectId,
        catalog: Arc<ProjectCatalog>,
        graph: Option<GraphClient>,
        call_ctx: CallCtx,
    ) -> Self {
        Self {
            pool,
            tenant,
            project,
            catalog,
            graph,
            call_ctx,
        }
    }
}

#[async_trait]
impl DataProvider for PgDataProvider {
    async fn velocity(&self, query: &VelocityQuery) -> Result<VelocityData, ProviderError> {
        let started = std::time::Instant::now();
        let compiled = velocity_sql::compile(query, self.project.as_uuid(), &self.catalog)?;
        let mut tx = TenantTx::begin(&self.pool, self.tenant)
            .await
            .map_err(provider_err)?;

        let row = bind_params(sqlx::query(&compiled.main.sql), &compiled.main.params)
            .fetch_one(&mut **tx)
            .await
            .map_err(provider_err)?;
        let samples: i64 = row.try_get("samples").map_err(provider_err)?;
        let aggregate: Option<f64> = row.try_get("aggregate").map_err(provider_err)?;
        let values: Vec<f64> = if compiled.wants_values {
            row.try_get::<Vec<f64>, _>("vals").map_err(provider_err)?
        } else {
            Vec::new()
        };

        let mut buckets = Vec::new();
        if let Some((stmt, zero_filled)) = &compiled.buckets {
            let rows = bind_params(sqlx::query(&stmt.sql), &stmt.params)
                .fetch_all(&mut **tx)
                .await
                .map_err(provider_err)?;
            // Rows come oldest → newest (bucket index k descending), which is what the engine expects.
            for r in &rows {
                let start: DateTime<Utc> = r.try_get("start").map_err(provider_err)?;
                let v: Option<f64> = r.try_get("v").map_err(provider_err)?;
                match (v, zero_filled) {
                    (Some(value), _) => buckets.push(BucketPoint { start, value }),
                    (None, true) => buckets.push(BucketPoint { start, value: 0.0 }),
                    (None, false) => {}
                }
            }
        }
        tx.commit().await.map_err(provider_err)?;
        metrics::histogram!("rule_velocity_query_seconds").record(started.elapsed().as_secs_f64());
        Ok(VelocityData {
            aggregate,
            samples: u64::try_from(samples).unwrap_or(0),
            values,
            buckets,
        })
    }

    async fn reference_lookup(&self, list: &str, key: &str) -> Result<RefLookup, ProviderError> {
        let mut tx = TenantTx::begin(&self.pool, self.tenant)
            .await
            .map_err(provider_err)?;
        // Project list first, then the tenant-wide list with the same name (rule-dsl §6.4).
        let row = sqlx::query(
            "WITH l AS (SELECT id FROM rules.reference_lists \
                        WHERE name = $1 AND (project_id = $2 OR project_id IS NULL) \
                        ORDER BY (project_id IS NULL) ASC LIMIT 1) \
             SELECT (SELECT id FROM l) AS list_id, en.attributes, \
                    (en.valid_from <= now() AND (en.valid_until IS NULL OR en.valid_until > now())) AS valid \
             FROM (SELECT 1) AS one \
             LEFT JOIN rules.reference_entries en ON en.list_id = (SELECT id FROM l) AND en.key = $3",
        )
        .bind(list)
        .bind(self.project.as_uuid())
        .bind(key)
        .fetch_one(&mut **tx)
        .await
        .map_err(provider_err)?;
        tx.commit().await.map_err(provider_err)?;
        let list_id: Option<Uuid> = row.try_get("list_id").map_err(provider_err)?;
        if list_id.is_none() {
            return Ok(RefLookup::UnknownList);
        }
        let attributes: Option<Value> = row.try_get("attributes").map_err(provider_err)?;
        let valid: Option<bool> = row.try_get("valid").map_err(provider_err)?;
        Ok(match attributes {
            None => RefLookup::NotFound,
            Some(attributes) => RefLookup::Found {
                attributes,
                valid: valid.unwrap_or(false),
            },
        })
    }

    async fn graph_metric(&self, query: &GraphMetricQuery) -> Result<Option<f64>, ProviderError> {
        let graph = self
            .graph
            .as_ref()
            .ok_or_else(|| ProviderError::Other("graph-service is not configured".into()))?;
        let customer_id = Uuid::parse_str(&query.customer_id)
            .map_err(|_| ProviderError::InvalidQuery("customer id is not a UUID".into()))?;
        let metric = serde_json::from_value(serde_json::json!(query.metric.as_str()))
            .map_err(|e| ProviderError::InvalidQuery(format!("unknown graph metric: {e}")))?;
        let link_kinds: Vec<LinkKind> = if query.link_kinds.is_empty() {
            LinkKind::DEFAULT.to_vec()
        } else {
            query
                .link_kinds
                .iter()
                .map(|k| {
                    LinkKind::parse(k)
                        .ok_or_else(|| ProviderError::InvalidQuery(format!("unknown link kind '{k}'")))
                })
                .collect::<Result<_, _>>()?
        };
        let req = GraphMetricRequest {
            customer_id,
            metric,
            link_kinds,
            include_similar: query.include_similar,
            max_depth: query.max_depth,
        };
        graph.metric(&self.call_ctx, &req).await
    }
}
