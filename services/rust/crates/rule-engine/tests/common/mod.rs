//! Shared test doubles: an in-memory `DataProvider` that records queries, and timers.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use rule_engine::model::RuleEnvelope;
use rule_engine::ports::{
    DataProvider, GraphMetricQuery, ProviderError, RefLookup, Timer, VelocityData, VelocityQuery,
};
use rule_engine::EvalContext;
use serde_json::{json, Value};

type VelocityFn = Box<dyn Fn(&VelocityQuery) -> Result<VelocityData, ProviderError> + Send + Sync>;

/// In-memory provider. Velocity answers come from a closure; lists and graph metrics from maps.
pub struct MockProvider {
    pub velocity_fn: VelocityFn,
    pub lists: HashMap<String, HashMap<String, (Value, bool)>>,
    pub graph: HashMap<String, Option<f64>>,
    pub graph_error: bool,
    pub velocity_calls: Mutex<Vec<VelocityQuery>>,
    pub reference_calls: Mutex<Vec<(String, String)>>,
    pub graph_calls: Mutex<Vec<GraphMetricQuery>>,
    pub hang_velocity: bool,
}

impl Default for MockProvider {
    fn default() -> Self {
        Self {
            velocity_fn: Box::new(|_| Ok(VelocityData::default())),
            lists: HashMap::new(),
            graph: HashMap::new(),
            graph_error: false,
            velocity_calls: Mutex::new(Vec::new()),
            reference_calls: Mutex::new(Vec::new()),
            graph_calls: Mutex::new(Vec::new()),
            hang_velocity: false,
        }
    }
}

impl MockProvider {
    pub fn with_velocity(
        f: impl Fn(&VelocityQuery) -> Result<VelocityData, ProviderError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            velocity_fn: Box::new(f),
            ..Default::default()
        }
    }

    pub fn aggregate(value: f64, samples: u64) -> Self {
        Self::with_velocity(move |_| {
            Ok(VelocityData {
                aggregate: Some(value),
                samples,
                ..Default::default()
            })
        })
    }

    pub fn with_list(mut self, list: &str, entries: &[(&str, Value, bool)]) -> Self {
        let map = self.lists.entry(list.to_string()).or_default();
        for (key, attrs, valid) in entries {
            map.insert((*key).to_string(), (attrs.clone(), *valid));
        }
        self
    }

    pub fn with_graph(mut self, metric: &str, value: Option<f64>) -> Self {
        self.graph.insert(metric.to_string(), value);
        self
    }

    pub fn velocity_call_count(&self) -> usize {
        self.velocity_calls.lock().unwrap().len()
    }

    pub fn last_velocity_query(&self) -> VelocityQuery {
        self.velocity_calls
            .lock()
            .unwrap()
            .last()
            .cloned()
            .expect("no velocity query recorded")
    }
}

#[async_trait]
impl DataProvider for MockProvider {
    async fn velocity(&self, query: &VelocityQuery) -> Result<VelocityData, ProviderError> {
        self.velocity_calls.lock().unwrap().push(query.clone());
        if self.hang_velocity {
            futures::future::pending::<()>().await;
        }
        (self.velocity_fn)(query)
    }

    async fn reference_lookup(&self, list: &str, key: &str) -> Result<RefLookup, ProviderError> {
        self.reference_calls
            .lock()
            .unwrap()
            .push((list.to_string(), key.to_string()));
        Ok(match self.lists.get(list) {
            None => RefLookup::UnknownList,
            Some(entries) => match entries.get(key) {
                None => RefLookup::NotFound,
                Some((attributes, valid)) => RefLookup::Found {
                    attributes: attributes.clone(),
                    valid: *valid,
                },
            },
        })
    }

    async fn graph_metric(&self, query: &GraphMetricQuery) -> Result<Option<f64>, ProviderError> {
        self.graph_calls.lock().unwrap().push(query.clone());
        if self.graph_error {
            return Err(ProviderError::Other("graph down".into()));
        }
        Ok(self.graph.get(query.metric.as_str()).copied().flatten())
    }
}

/// Timer whose sleep completes immediately (forces timeouts of pending work).
pub struct InstantTimer;

#[async_trait]
impl Timer for InstantTimer {
    async fn sleep(&self, _duration: std::time::Duration) {}
}

/// Timer that never fires.
pub struct NeverTimer;

#[async_trait]
impl Timer for NeverTimer {
    async fn sleep(&self, _duration: std::time::Duration) {
        futures::future::pending::<()>().await;
    }
}

pub fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 1, 12, 0, 0).unwrap()
}

/// A realistic context for a card transaction.
pub fn ctx() -> EvalContext {
    EvalContext::new(
        json!({
            "event": {
                "event_type": "transaction", "customer_id": "cust-1", "amount": 7_500_000, "currency": "IDR",
                "channel": "web", "device_id": "dev-9", "ip_address": "10.0.0.1",
                "instrument_fingerprint": "card-abc", "card_bin": "411111", "issuer_country": "SG",
                "geo_country": "ID", "promo_code": null, "merchant_id": "m-1",
                "occurred_at": "2026-09-01T12:00:00Z"
            },
            "source": { "order": { "items": [{ "sku": "A", "qty": 2 }], "coupon": "HEMAT50" } },
            "customer": { "external_id": "C-1", "kyc_level": 1, "account_age_days": 3,
                          "attributes": { "monthly_income": 5_000_000 } },
            "features": { "cust_cnt_24h": 4, "amount_zscore_30d": 3.2, "is_new_device": 1 },
            "ml": { "fraud_probability": 0.82, "anomaly_score": 0.4 },
            "graph": { "distance_to_fraud": 2 }
        }),
        "transaction",
        t0(),
    )
    .with_event_id("evt-1")
    .with_customer_id("cust-1")
}

pub fn rule(value: Value) -> RuleEnvelope {
    serde_json::from_value(value).expect("valid rule fixture")
}

pub fn block_on<F: std::future::Future>(f: F) -> F::Output {
    futures::executor::block_on(f)
}
