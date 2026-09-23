//! `CachedProvider`: a caching **decorator** around any [`DataProvider`].
//!
//! In Java you would write `class CachingDataProvider implements DataProvider { private final DataProvider
//! delegate; … }`. The Rust version is the same idea: a struct owning an inner provider and implementing the same
//! trait. The engine cannot tell the difference, and the Postgres adapter stays free of caching concerns.
//!
//! Two caches with different lifetimes:
//! * **reference lookups**: shared across requests for a few seconds (`RefCache`, keyed by project). A newly
//!   blacklisted card therefore takes effect within `ttl` seconds, a deliberate latency/freshness trade-off;
//! * **velocity and graph queries**: memoised *within one evaluation* only. Identical queries issued by several
//!   rules run once (single-flight via `OnceCell`), and results never leak into the next event.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use moka::future::Cache;
use platform::ProjectId;
use rule_engine::ports::{
    DataProvider, GraphMetricQuery, ProviderError, RefLookup, VelocityData, VelocityQuery,
};
use tokio::sync::{Mutex, OnceCell};

/// Cross-request cache of reference lookups.
pub type RefCache = Cache<(ProjectId, String, String), RefLookup>;

pub fn new_ref_cache(ttl: Duration) -> RefCache {
    Cache::builder().max_capacity(100_000).time_to_live(ttl).build()
}

type Memo<T> = Mutex<HashMap<String, Arc<OnceCell<Result<T, ProviderError>>>>>;

pub struct CachedProvider<P> {
    inner: P,
    project: ProjectId,
    refs: Option<RefCache>,
    velocity: Memo<VelocityData>,
    graph: Memo<Option<f64>>,
}

impl<P> std::fmt::Debug for CachedProvider<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CachedProvider")
            .field("project", &self.project)
            .finish_non_exhaustive()
    }
}

impl<P: DataProvider> CachedProvider<P> {
    /// `refs = None` disables the cross-request reference cache (backtests, tests).
    pub fn new(inner: P, project: ProjectId, refs: Option<RefCache>) -> Self {
        Self {
            inner,
            project,
            refs,
            velocity: Mutex::default(),
            graph: Mutex::default(),
        }
    }
}

async fn memoised<T: Clone, F, Fut>(memo: &Memo<T>, key: String, run: F) -> Result<T, ProviderError>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<T, ProviderError>>,
{
    let cell = {
        let mut map = memo.lock().await;
        map.entry(key)
            .or_insert_with(|| Arc::new(OnceCell::new()))
            .clone()
    };
    cell.get_or_init(run).await.clone()
}

#[async_trait]
impl<P: DataProvider> DataProvider for CachedProvider<P> {
    async fn velocity(&self, query: &VelocityQuery) -> Result<VelocityData, ProviderError> {
        let key = serde_json::to_string(query).map_err(|e| ProviderError::Other(e.to_string()))?;
        memoised(&self.velocity, key, || self.inner.velocity(query)).await
    }

    async fn reference_lookup(&self, list: &str, key: &str) -> Result<RefLookup, ProviderError> {
        let Some(cache) = &self.refs else {
            return self.inner.reference_lookup(list, key).await;
        };
        let cache_key = (self.project, list.to_string(), key.to_string());
        if let Some(hit) = cache.get(&cache_key).await {
            metrics::counter!("rule_reference_cache_total", "result" => "hit").increment(1);
            return Ok(hit);
        }
        metrics::counter!("rule_reference_cache_total", "result" => "miss").increment(1);
        let result = self.inner.reference_lookup(list, key).await?;
        cache.insert(cache_key, result.clone()).await;
        Ok(result)
    }

    async fn graph_metric(&self, query: &GraphMetricQuery) -> Result<Option<f64>, ProviderError> {
        let key = serde_json::to_string(query).map_err(|e| ProviderError::Other(e.to_string()))?;
        memoised(&self.graph, key, || self.inner.graph_metric(query)).await
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use chrono::Utc;
    use rule_engine::model::AggFn;
    use rule_engine::ports::{GroupKey, QueryWindow, SeriesRequest};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use uuid::Uuid;

    #[derive(Default)]
    struct Counting {
        velocity: AtomicUsize,
        refs: AtomicUsize,
    }

    #[async_trait]
    impl DataProvider for Counting {
        async fn velocity(&self, _: &VelocityQuery) -> Result<VelocityData, ProviderError> {
            self.velocity.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(5)).await;
            Ok(VelocityData {
                aggregate: Some(3.0),
                samples: 3,
                ..Default::default()
            })
        }
        async fn reference_lookup(&self, _: &str, _: &str) -> Result<RefLookup, ProviderError> {
            self.refs.fetch_add(1, Ordering::SeqCst);
            Ok(RefLookup::NotFound)
        }
        async fn graph_metric(&self, _: &GraphMetricQuery) -> Result<Option<f64>, ProviderError> {
            Ok(Some(1.0))
        }
    }

    fn query() -> VelocityQuery {
        VelocityQuery {
            event_id: None,
            anchor: Utc::now(),
            history_event_types: vec!["transaction".into()],
            group_by: vec![GroupKey {
                field: "device_id".into(),
                value: serde_json::json!("d1"),
            }],
            window: QueryWindow::Duration { seconds: 60 },
            aggregate_fn: AggFn::Count,
            aggregate_field: None,
            percentile: None,
            include_current: true,
            filter: None,
            series: SeriesRequest::None,
        }
    }

    #[tokio::test]
    async fn identical_velocity_queries_run_once_even_concurrently() {
        let p = CachedProvider::new(Counting::default(), ProjectId(Uuid::nil()), None);
        let q = query();
        let (a, b) = tokio::join!(p.velocity(&q), p.velocity(&q));
        assert_eq!(a.unwrap(), b.unwrap());
        assert_eq!(p.inner.velocity.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn reference_cache_is_shared_across_providers() {
        let cache = new_ref_cache(Duration::from_secs(5));
        let p1 = CachedProvider::new(Counting::default(), ProjectId(Uuid::nil()), Some(cache.clone()));
        p1.reference_lookup("l", "k").await.unwrap();
        p1.reference_lookup("l", "k").await.unwrap();
        assert_eq!(p1.inner.refs.load(Ordering::SeqCst), 1);
        let p2 = CachedProvider::new(Counting::default(), ProjectId(Uuid::nil()), Some(cache));
        p2.reference_lookup("l", "k").await.unwrap();
        assert_eq!(p2.inner.refs.load(Ordering::SeqCst), 0);
    }
}
