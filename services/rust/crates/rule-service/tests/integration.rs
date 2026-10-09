//! Integration tests against a real PostgreSQL 16 with the production schema, roles, grants and RLS.
//!
//! Run with `tests/run-integration.sh` (starts and removes a throwaway container). Without the
//! `RULE_IT_*` environment variables every test is skipped, so a plain `cargo test` stays green offline.
//!
//! Fixtures are written as `migrator` (schema owner, bypasses RLS); the service under test connects as
//! `rule_service`, so grants and RLS are exercised exactly as in production.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use chrono::{DateTime, TimeZone, Utc};
use http_body_util::BodyExt;
use platform::auth::{Claims, JwtKeys, ProjectRole, TenantRole};
use platform::config::Secret;
use platform::http::{CallCtx, ServiceClient};
use platform::{ProjectId, TenantId};
use rule_engine::model::{AggFn, Op};
use rule_engine::ports::{
    DataProvider, GroupKey, HistFilter, HistPredicate, ProviderError, QueryWindow, RefLookup, SeriesRequest,
    VelocityQuery,
};
use rule_service::adapters::catalog::ProjectCatalog;
use rule_service::adapters::data_provider::{GraphClient, PgDataProvider};
use rule_service::config::Settings;
use rule_service::state::AppState;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

const JWT: &str = "integration-test-jwt-secret-0123456789";
const INTERNAL: &str = "integration-test-internal-token";

fn anchor() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 1, 12, 0, 0).unwrap()
}

// ---------------------------------------------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------------------------------------------

static MIGRATED: OnceLock<()> = OnceLock::new();

fn urls() -> Option<(String, String, String)> {
    Some((
        std::env::var("RULE_IT_MIGRATOR_URL").ok()?,
        std::env::var("RULE_IT_SERVICE_URL").ok()?,
        std::env::var("RULE_IT_MIGRATIONS_DIR").ok()?,
    ))
}

/// Applies db/migrations once per test binary (own thread + runtime, independent of test runtimes).
fn migrate_once(url: &str, dir: &str) {
    MIGRATED.get_or_init(|| {
        let (url, dir) = (url.to_string(), dir.to_string());
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                let pool = platform::db::connect_url(&url, 2).await.unwrap();
                platform::db::run_migrations(&pool, std::path::Path::new(&dir))
                    .await
                    .unwrap();
                pool.close().await;
            });
        })
        .join()
        .unwrap();
    });
}

struct Env {
    migrator: PgPool,
    service: PgPool,
    app: Router,
    jwt: JwtKeys,
    tenant: TenantId,
    project: ProjectId,
    source: Uuid,
    analyst: Uuid,
    approver: Uuid,
}

async fn env_with_graph(graph: Option<GraphClient>) -> Option<Env> {
    let Some((migrator_url, service_url, dir)) = urls() else {
        eprintln!("RULE_IT_* not set — skipping integration test (use tests/run-integration.sh)");
        return None;
    };
    migrate_once(&migrator_url, &dir);
    let migrator = platform::db::connect_url(&migrator_url, 4).await.unwrap();
    let service = platform::db::connect_url(&service_url, 8).await.unwrap();
    let (tenant, project, source) = new_project(&migrator).await;
    let analyst = new_user(&migrator, tenant).await;
    let approver = new_user(&migrator, tenant).await;
    let settings = Settings {
        rule_timeout: Duration::from_millis(2_000),
        eval_async_writes: false,
        backtest_concurrency: 4,
    };
    let state = AppState::new(
        service.clone(),
        &Secret::new(JWT),
        &Secret::new(INTERNAL),
        graph,
        settings,
        Duration::from_millis(1),
        Duration::from_millis(1),
    );
    let app = rule_service::api::router(state);
    Some(Env {
        migrator,
        service,
        app,
        jwt: JwtKeys::new(&Secret::new(JWT)),
        tenant,
        project,
        source,
        analyst,
        approver,
    })
}

async fn env() -> Option<Env> {
    env_with_graph(None).await
}

async fn new_project(db: &PgPool) -> (TenantId, ProjectId, Uuid) {
    let slug = format!("t{}", &Uuid::new_v4().simple().to_string()[..12]);
    let tenant: Uuid =
        sqlx::query_scalar("INSERT INTO core.tenants (slug, name) VALUES ($1, $1) RETURNING id")
            .bind(&slug)
            .fetch_one(db)
            .await
            .unwrap();
    let project: Uuid = sqlx::query_scalar(
        "INSERT INTO core.projects (tenant_id, slug, name, stage) VALUES ($1, 'checkout', 'Checkout', 'pre_payment') RETURNING id",
    )
    .bind(tenant)
    .fetch_one(db)
    .await
    .unwrap();
    let source: Uuid = sqlx::query_scalar(
        "INSERT INTO core.data_sources (tenant_id, project_id, slug, name, kind) VALUES ($1, $2, 'canonical', 'Canonical', 'internal') RETURNING id",
    )
    .bind(tenant)
    .bind(project)
    .fetch_one(db)
    .await
    .unwrap();
    (TenantId(tenant), ProjectId(project), source)
}

async fn new_user(db: &PgPool, tenant: TenantId) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO core.app_users (tenant_id, email, full_name, password_hash) VALUES ($1, $2, 'Test', 'x') RETURNING id",
    )
    .bind(tenant.as_uuid())
    .bind(format!("{}@it.local", Uuid::new_v4()))
    .fetch_one(db)
    .await
    .unwrap()
}

impl Env {
    async fn customer(&self, external_id: &str) -> Uuid {
        sqlx::query_scalar(
            "INSERT INTO core.customers (tenant_id, project_id, external_id, registered_at) VALUES ($1, $2, $3, $4) RETURNING id",
        )
        .bind(self.tenant.as_uuid())
        .bind(self.project.as_uuid())
        .bind(external_id)
        .bind(anchor() - chrono::Duration::days(100))
        .fetch_one(&self.migrator)
        .await
        .unwrap()
    }

    /// Inserts an event; `fields` may set amount, device_id, instrument_fingerprint, promo_code, discount_amount,
    /// login_success, merchant_id, payload, event_type.
    async fn event(&self, customer: Uuid, at: DateTime<Utc>, fields: Value) -> Uuid {
        let f = |k: &str| fields.get(k).cloned().unwrap_or(Value::Null);
        sqlx::query_scalar(
            "INSERT INTO core.events (tenant_id, project_id, data_source_id, external_id, event_type, customer_id, occurred_at, \
             amount, device_id, instrument_fingerprint, promo_code, discount_amount, login_success, merchant_id, payload) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,($8->>0)::numeric,$9->>0,$10->>0,$11->>0,($12->>0)::numeric,($13->>0)::boolean,$14->>0,$15) \
             RETURNING id",
        )
        .bind(self.tenant.as_uuid())
        .bind(self.project.as_uuid())
        .bind(self.source)
        .bind(Uuid::new_v4().to_string())
        .bind(fields.get("event_type").and_then(Value::as_str).unwrap_or("transaction"))
        .bind(customer)
        .bind(at)
        .bind(json!([f("amount")]))
        .bind(json!([f("device_id")]))
        .bind(json!([f("instrument_fingerprint")]))
        .bind(json!([f("promo_code")]))
        .bind(json!([f("discount_amount")]))
        .bind(json!([f("login_success")]))
        .bind(json!([f("merchant_id")]))
        .bind(fields.get("payload").cloned().unwrap_or_else(|| json!({})))
        .fetch_one(&self.migrator)
        .await
        .unwrap()
    }

    async fn label(&self, event: Uuid, label: &str) {
        sqlx::query(
            "INSERT INTO core.labels (tenant_id, project_id, subject_type, subject_id, label, source) VALUES ($1,$2,'event',$3,$4,'analyst')",
        )
        .bind(self.tenant.as_uuid())
        .bind(self.project.as_uuid())
        .bind(event)
        .bind(label)
        .execute(&self.migrator)
        .await
        .unwrap();
    }

    fn provider(&self) -> PgDataProvider {
        self.provider_for(self.tenant, self.project, ProjectCatalog::builtins_only())
    }

    fn provider_for(&self, tenant: TenantId, project: ProjectId, catalog: ProjectCatalog) -> PgDataProvider {
        PgDataProvider::new(
            self.service.clone(),
            tenant,
            project,
            Arc::new(catalog),
            None,
            CallCtx::new(tenant, Some(project)),
        )
    }

    fn user_token(&self, user: Uuid, role: ProjectRole) -> String {
        let mut prj = HashMap::new();
        prj.insert(self.project.as_uuid(), role);
        let claims = Claims::new(
            user,
            Some(self.tenant.as_uuid()),
            TenantRole::Member,
            false,
            prj,
            Duration::from_secs(600),
        );
        self.jwt.issue(&claims).unwrap()
    }

    fn tenant_admin_token(&self, user: Uuid) -> String {
        let claims = Claims::new(
            user,
            Some(self.tenant.as_uuid()),
            TenantRole::TenantAdmin,
            false,
            HashMap::new(),
            Duration::from_secs(600),
        );
        self.jwt.issue(&claims).unwrap()
    }

    async fn call(&self, method: &str, uri: &str, auth: &Auth, body: Option<Value>) -> (StatusCode, Value) {
        let mut req = Request::builder().method(method).uri(uri);
        match auth {
            Auth::User(token) => req = req.header("authorization", format!("Bearer {token}")),
            Auth::Service(actor) => {
                req = req
                    .header("authorization", format!("Bearer {INTERNAL}"))
                    .header("x-tenant-id", self.tenant.to_string())
                    .header("x-project-id", self.project.to_string());
                if let Some(a) = actor {
                    req = req.header("x-actor", a.to_string());
                }
            }
        }
        let req = match body {
            Some(b) => req
                .header("content-type", "application/json")
                .body(Body::from(b.to_string()))
                .unwrap(),
            None => req.body(Body::empty()).unwrap(),
        };
        self.send(req).await
    }

    async fn send(&self, req: Request<Body>) -> (StatusCode, Value) {
        let resp = self.app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let value = serde_json::from_slice(&bytes).unwrap_or_else(|_| json!(String::from_utf8_lossy(&bytes)));
        (status, value)
    }

    fn p(&self, path: &str) -> String {
        format!("/api/v1/projects/{}{path}", self.project)
    }
}

enum Auth {
    User(String),
    Service(Option<Uuid>),
}

fn vq(group: (&str, Value), window: QueryWindow, func: AggFn, field: Option<&str>) -> VelocityQuery {
    VelocityQuery {
        event_id: None,
        anchor: anchor(),
        history_event_types: vec!["transaction".into()],
        group_by: vec![GroupKey {
            field: group.0.into(),
            value: group.1,
        }],
        window,
        aggregate_fn: func,
        aggregate_field: field.map(str::to_string),
        percentile: None,
        include_current: true,
        filter: None,
        series: SeriesRequest::None,
    }
}

const DAY: QueryWindow = QueryWindow::Duration { seconds: 86_400 };

// ---------------------------------------------------------------------------------------------------------------
// PgDataProvider (SQL) tests
// ---------------------------------------------------------------------------------------------------------------

#[tokio::test]
async fn velocity_aggregates_windows_series_and_filters() {
    let Some(env) = env().await else { return };
    let a = env.customer("A").await;
    let b = env.customer("B").await;
    let c = env.customer("C").await;
    let mut ids = Vec::new();
    for (i, amount) in [100, 200, 300, 400].iter().enumerate() {
        let at = anchor() - chrono::Duration::hours(3 - i as i64);
        ids.push(
            env.event(
                a,
                at,
                json!({"amount": amount, "device_id": "dev-1", "payload": {"order": {"total": 5}}}),
            )
            .await,
        );
    }
    env.event(
        b,
        anchor() - chrono::Duration::minutes(30),
        json!({"amount": 50, "device_id": "dev-1"}),
    )
    .await;
    env.event(
        c,
        anchor() - chrono::Duration::minutes(10),
        json!({"amount": 60, "device_id": "dev-2"}),
    )
    .await;
    // Outside the 24h window and a different event type: must never be counted.
    env.event(
        a,
        anchor() - chrono::Duration::days(3),
        json!({"amount": 9999, "device_id": "dev-1"}),
    )
    .await;
    env.event(
        a,
        anchor() - chrono::Duration::hours(1),
        json!({"event_type": "login", "device_id": "dev-1"}),
    )
    .await;

    let p = env.provider();
    let cust = ("customer_id", json!(a.to_string()));
    let agg = |func, field| {
        let p = &p;
        let q = vq(cust.clone(), DAY, func, field);
        async move { p.velocity(&q).await.unwrap() }
    };
    let count = agg(AggFn::Count, None).await;
    assert_eq!((count.aggregate, count.samples), (Some(4.0), 4));
    assert_eq!(agg(AggFn::Sum, Some("amount")).await.aggregate, Some(1000.0));
    assert_eq!(agg(AggFn::Avg, Some("amount")).await.aggregate, Some(250.0));
    assert_eq!(agg(AggFn::Min, Some("amount")).await.aggregate, Some(100.0));
    assert_eq!(agg(AggFn::Max, Some("amount")).await.aggregate, Some(400.0));
    assert_eq!(agg(AggFn::Median, Some("amount")).await.aggregate, Some(250.0));
    let sd = agg(AggFn::Stddev, Some("amount")).await.aggregate.unwrap();
    assert!((sd - 129.099_444_873_580_56).abs() < 1e-6, "{sd}");
    let mut q = vq(cust.clone(), DAY, AggFn::Percentile, Some("amount"));
    q.percentile = Some(0.9);
    assert_eq!(p.velocity(&q).await.unwrap().aggregate, Some(370.0));

    // include_current = false excludes the anchor event by id.
    let mut q = vq(cust.clone(), DAY, AggFn::Count, None);
    q.include_current = false;
    q.event_id = Some(ids[3].to_string());
    assert_eq!(p.velocity(&q).await.unwrap().aggregate, Some(3.0));

    // Duration window (90 min) and last_n.
    let q = vq(
        cust.clone(),
        QueryWindow::Duration { seconds: 5_400 },
        AggFn::Count,
        None,
    );
    assert_eq!(p.velocity(&q).await.unwrap().aggregate, Some(2.0));
    let q = vq(
        cust.clone(),
        QueryWindow::LastN { n: 2 },
        AggFn::Sum,
        Some("amount"),
    );
    assert_eq!(p.velocity(&q).await.unwrap().aggregate, Some(700.0));

    // Distinct customers per device.
    let q = vq(
        ("device_id", json!("dev-1")),
        DAY,
        AggFn::DistinctCount,
        Some("customer_id"),
    );
    assert_eq!(p.velocity(&q).await.unwrap().aggregate, Some(2.0));

    // Empty history: count/sum → 0, avg → None.
    let q = vq(("device_id", json!("nope")), DAY, AggFn::Sum, Some("amount"));
    assert_eq!(p.velocity(&q).await.unwrap().aggregate, Some(0.0));
    let q = vq(("device_id", json!("nope")), DAY, AggFn::Avg, Some("amount"));
    assert_eq!(p.velocity(&q).await.unwrap().aggregate, None);

    // Value series (oldest → newest).
    let mut q = vq(cust.clone(), DAY, AggFn::Avg, Some("amount"));
    q.series = SeriesRequest::Values;
    assert_eq!(
        p.velocity(&q).await.unwrap().values,
        vec![100.0, 200.0, 300.0, 400.0]
    );

    // Zero-filled hourly buckets over 6h: oldest → newest, last bucket contains the anchor.
    let mut q = vq(
        cust.clone(),
        QueryWindow::Duration { seconds: 6 * 3600 },
        AggFn::Count,
        None,
    );
    q.series = SeriesRequest::Buckets {
        bucket_seconds: 3600,
        func: AggFn::Count,
    };
    let data = p.velocity(&q).await.unwrap();
    let values: Vec<f64> = data.buckets.iter().map(|b| b.value).collect();
    assert_eq!(values, vec![0.0, 0.0, 1.0, 1.0, 1.0, 1.0]);
    assert_eq!(
        data.buckets.last().unwrap().start,
        anchor() - chrono::Duration::hours(1)
    );

    // History filter.
    let mut q = vq(cust.clone(), DAY, AggFn::Count, None);
    q.filter = Some(HistFilter::And(vec![HistFilter::Pred(HistPredicate {
        field: "amount".into(),
        op: Op::Gt,
        value: json!(150),
    })]));
    assert_eq!(p.velocity(&q).await.unwrap().aggregate, Some(3.0));

    // Other event types via history_event_types.
    let mut q = vq(cust.clone(), DAY, AggFn::Count, None);
    q.history_event_types = vec!["transaction".into(), "login".into()];
    assert_eq!(p.velocity(&q).await.unwrap().aggregate, Some(5.0));

    // source.* velocity requires a velocity-enabled catalogue entry.
    let q = vq(cust.clone(), DAY, AggFn::Sum, Some("source.order.total"));
    assert!(matches!(
        p.velocity(&q).await,
        Err(ProviderError::InvalidQuery(_))
    ));
    let catalog = ProjectCatalog::with_source_fields([("source.order.total".to_string(), true)]);
    let p2 = env.provider_for(env.tenant, env.project, catalog);
    assert_eq!(p2.velocity(&q).await.unwrap().aggregate, Some(20.0));
}

#[tokio::test]
async fn sql_injection_through_field_names_is_rejected() {
    let Some(env) = env().await else { return };
    let p = env.provider();
    for field in [
        "amount; DROP TABLE core.events; --",
        "amount) OR (1=1",
        "payload",
        "customer_id::text",
    ] {
        let q = vq(("device_id", json!("x")), DAY, AggFn::Sum, Some(field));
        assert!(
            matches!(p.velocity(&q).await, Err(ProviderError::InvalidQuery(_))),
            "{field}"
        );
        let q = vq((field, json!("x")), DAY, AggFn::Count, None);
        assert!(
            matches!(p.velocity(&q).await, Err(ProviderError::InvalidQuery(_))),
            "{field}"
        );
    }
    // Values are parameters: an injection attempt in a value is just a value that matches nothing.
    let q = vq(("device_id", json!("x' OR '1'='1")), DAY, AggFn::Count, None);
    assert_eq!(p.velocity(&q).await.unwrap().aggregate, Some(0.0));
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM core.events")
        .fetch_one(&env.migrator)
        .await
        .unwrap();
    assert!(n >= 0, "events table still exists");
}

#[tokio::test]
async fn rls_hides_other_tenants_even_with_a_foreign_project_id() {
    let Some(env) = env().await else { return };
    let (other_tenant, other_project, other_source) = new_project(&env.migrator).await;
    let customer: Uuid = sqlx::query_scalar(
        "INSERT INTO core.customers (tenant_id, project_id, external_id) VALUES ($1,$2,'X') RETURNING id",
    )
    .bind(other_tenant.as_uuid())
    .bind(other_project.as_uuid())
    .fetch_one(&env.migrator)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO core.events (tenant_id, project_id, data_source_id, external_id, event_type, customer_id, occurred_at, device_id) \
         VALUES ($1,$2,$3,'e1','transaction',$4,$5,'shared-device')",
    )
    .bind(other_tenant.as_uuid())
    .bind(other_project.as_uuid())
    .bind(other_source)
    .bind(customer)
    .bind(anchor())
    .execute(&env.migrator)
    .await
    .unwrap();
    let q = vq(("device_id", json!("shared-device")), DAY, AggFn::Count, None);
    // Correct tenant: visible.
    let own = env.provider_for(other_tenant, other_project, ProjectCatalog::builtins_only());
    assert_eq!(own.velocity(&q).await.unwrap().aggregate, Some(1.0));
    // Wrong tenant context with the other tenant's project id: RLS returns nothing.
    let foreign = env.provider_for(env.tenant, other_project, ProjectCatalog::builtins_only());
    assert_eq!(foreign.velocity(&q).await.unwrap().aggregate, Some(0.0));
}

#[tokio::test]
async fn reference_lookup_resolves_project_then_tenant_and_validity() {
    let Some(env) = env().await else { return };
    let tenant_list: Uuid = sqlx::query_scalar(
        "INSERT INTO rules.reference_lists (tenant_id, project_id, name, list_type) VALUES ($1, NULL, 'bl', 'blacklist') RETURNING id",
    )
    .bind(env.tenant.as_uuid())
    .fetch_one(&env.migrator)
    .await
    .unwrap();
    let project_list: Uuid = sqlx::query_scalar(
        "INSERT INTO rules.reference_lists (tenant_id, project_id, name, list_type) VALUES ($1, $2, 'bl', 'blacklist') RETURNING id",
    )
    .bind(env.tenant.as_uuid())
    .bind(env.project.as_uuid())
    .fetch_one(&env.migrator)
    .await
    .unwrap();
    let only_tenant: Uuid = sqlx::query_scalar(
        "INSERT INTO rules.reference_lists (tenant_id, project_id, name, list_type) VALUES ($1, NULL, 'tl', 'watchlist') RETURNING id",
    )
    .bind(env.tenant.as_uuid())
    .fetch_one(&env.migrator)
    .await
    .unwrap();
    let entry = |list: Uuid, key: &'static str, until: Option<DateTime<Utc>>| {
        let db = env.migrator.clone();
        let tenant = env.tenant.as_uuid();
        async move {
            sqlx::query(
                "INSERT INTO rules.reference_entries (tenant_id, list_id, key, attributes, valid_until) VALUES ($1,$2,$3,'{\"a\":1}',$4)",
            )
            .bind(tenant)
            .bind(list)
            .bind(key)
            .bind(until)
            .execute(&db)
            .await
            .unwrap();
        }
    };
    entry(tenant_list, "k1", None).await;
    entry(project_list, "k2", None).await;
    entry(
        project_list,
        "expired",
        Some(Utc::now() - chrono::Duration::days(1)),
    )
    .await;
    entry(only_tenant, "x", None).await;

    let p = env.provider();
    assert_eq!(
        p.reference_lookup("bl", "k1").await.unwrap(),
        RefLookup::NotFound,
        "project list shadows tenant list"
    );
    assert!(matches!(
        p.reference_lookup("bl", "k2").await.unwrap(),
        RefLookup::Found { valid: true, .. }
    ));
    assert!(matches!(
        p.reference_lookup("bl", "expired").await.unwrap(),
        RefLookup::Found { valid: false, .. }
    ));
    assert!(matches!(
        p.reference_lookup("tl", "x").await.unwrap(),
        RefLookup::Found { valid: true, .. }
    ));
    assert_eq!(
        p.reference_lookup("missing", "x").await.unwrap(),
        RefLookup::UnknownList
    );
}

// ---------------------------------------------------------------------------------------------------------------
// API tests
// ---------------------------------------------------------------------------------------------------------------

/// Minimal graph-service stand-in: every metric = 1.
async fn fake_graph() -> GraphClient {
    let app = axum::Router::new().route(
        "/v1/projects/{pid}/metric",
        axum::routing::post(|| async { axum::Json(json!({ "value": 1.0 })) }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = ServiceClient::new(
        "graph-service",
        format!("http://{addr}"),
        Secret::new(INTERNAL),
        Duration::from_secs(2),
    )
    .unwrap();
    GraphClient::new(client, Duration::from_secs(2))
}

fn eval_body(event_id: Uuid, customer: Uuid, event: Value) -> Value {
    json!({
        "event_id": event_id, "occurred_at": anchor(), "event_type": "transaction", "customer_id": customer,
        "context": { "event": event, "features": {}, "customer": {"external_id": "C"}, "ml": {}, "graph": {} }
    })
}

fn find<'a>(results: &'a Value, code: &str) -> Vec<&'a Value> {
    results
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["rule_code"] == code)
        .collect()
}

#[tokio::test]
async fn bootstrap_then_evaluate_carding_scenario() {
    let graph = fake_graph().await;
    let Some(env) = env_with_graph(Some(graph)).await else {
        return;
    };
    let int = Auth::Service(None);

    let (s, report) = env
        .call(
            "POST",
            &format!("/v1/projects/{}/bootstrap", env.project),
            &int,
            Some(json!({"template": "pre_payment"})),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{report}");
    assert_eq!(report["rules_created"], 13);
    // Template rulesets serve immediately: version 1 approved by the system.
    let (_, rulesets) = env
        .call(
            "GET",
            &env.p("/rulesets"),
            &Auth::User(env.user_token(env.analyst, ProjectRole::Viewer)),
            None,
        )
        .await;
    assert_eq!(rulesets["total"], 4);
    for rs in rulesets["items"].as_array().unwrap() {
        assert_eq!(rs["serving"]["live_version"], 1, "{rs}");
        assert_eq!(rs["status"], "active");
    }
    assert_eq!(report["rulesets_created"], 4);
    // Idempotent.
    let (_, again) = env
        .call(
            "POST",
            &format!("/v1/projects/{}/bootstrap", env.project),
            &int,
            Some(json!({"template": "pre_payment"})),
        )
        .await;
    assert_eq!(again["rules_created"], 0);

    // Card used by three customers in 30 days; C's current transaction is the third.
    let (a, b, c) = (
        env.customer("A").await,
        env.customer("B").await,
        env.customer("C").await,
    );
    env.event(
        a,
        anchor() - chrono::Duration::days(5),
        json!({"amount": 10, "instrument_fingerprint": "card-X"}),
    )
    .await;
    env.event(
        b,
        anchor() - chrono::Duration::days(2),
        json!({"amount": 10, "instrument_fingerprint": "card-X"}),
    )
    .await;
    let current = env
        .event(
            c,
            anchor(),
            json!({"amount": 1000, "instrument_fingerprint": "card-X", "device_id": "d9"}),
        )
        .await;
    let event_json = json!({"event_type": "transaction", "amount": 1000, "instrument_fingerprint": "card-X", "device_id": "d9",
                            "occurred_at": anchor(), "customer_id": c});

    let started = std::time::Instant::now();
    let (s, resp) = env
        .call(
            "POST",
            &format!("/v1/projects/{}/evaluate", env.project),
            &int,
            Some(eval_body(current, c, event_json.clone())),
        )
        .await;
    let latency = started.elapsed();
    assert_eq!(s, StatusCode::OK, "{resp}");
    eprintln!(
        "evaluate latency (13 rules, cold caches): {latency:?}, engine {} ms",
        resp["duration_ms"]
    );
    let shared = find(&resp["rule_results"], "RL-PRE-CARD-SHARED");
    assert_eq!(shared.len(), 1);
    assert_eq!(shared[0]["outcome"], "match", "{}", shared[0]);
    let graph_rule = find(&resp["rule_results"], "RL-PRE-GRAPH-NEAR-FRAUD");
    assert_eq!(
        graph_rule[0]["outcome"], "match",
        "graph metric 1 <= 2: {}",
        graph_rule[0]
    );
    assert!(resp["rules_score"].as_f64().unwrap() > 0.0);
    assert!(!resp["actions"]["force_decline"].as_bool().unwrap());

    // Hits and counters were recorded (synchronous writes in tests).
    let hits: i64 =
        sqlx::query_scalar("SELECT count(*) FROM rules.rule_hits WHERE event_id = $1 AND outcome = 'match'")
            .bind(current)
            .fetch_one(&env.migrator)
            .await
            .unwrap();
    assert!(hits >= 2);

    // Blacklist the card through the API → force_decline on the next evaluation (reference cache invalidated).
    let analyst = Auth::User(env.user_token(env.analyst, ProjectRole::Analyst));
    let (_, lists) = env.call("GET", &env.p("/reference-lists"), &analyst, None).await;
    let card_bl = lists
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["name"] == "card_blacklist")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (s, up) = env
        .call(
            "POST",
            &env.p(&format!("/reference-lists/{card_bl}/entries")),
            &analyst,
            Some(json!({"entries": [{"key": "card-X", "reason": "chargeback"}]})),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{up}");
    let (_, resp) = env
        .call(
            "POST",
            &format!("/v1/projects/{}/evaluate", env.project),
            &int,
            Some(eval_body(current, c, event_json)),
        )
        .await;
    assert!(
        resp["actions"]["force_decline"].as_bool().unwrap(),
        "{}",
        resp["actions"]
    );

    // Warm evaluation latency (serving + catalogue cached): report it.
    let mut total = Duration::ZERO;
    for _ in 0..10 {
        let t = std::time::Instant::now();
        let (s, _) = env
            .call(
                "POST",
                &format!("/v1/projects/{}/evaluate", env.project),
                &int,
                Some(json!({
                    "event_id": current, "occurred_at": anchor(), "event_type": "transaction", "customer_id": c, "dry_run": true,
                    "context": {"event": {"amount": 1000, "instrument_fingerprint": "card-X", "device_id": "d9", "customer_id": c}}
                })),
            )
            .await;
        assert_eq!(s, StatusCode::OK);
        total += t.elapsed();
    }
    eprintln!(
        "evaluate latency warm avg over 10 (13 rules, dry_run): {:?}",
        total / 10
    );

    // Users may not call the internal endpoint.
    let (s, _) = env
        .call(
            "POST",
            &format!("/v1/projects/{}/evaluate", env.project),
            &analyst,
            Some(json!({})),
        )
        .await;
    assert!(
        s == StatusCode::FORBIDDEN || s == StatusCode::UNPROCESSABLE_ENTITY,
        "{s}"
    );
}

#[tokio::test]
async fn maker_checker_shadow_rules_and_live_version_during_edit() {
    let Some(env) = env().await else { return };
    let maker = Auth::User(env.user_token(env.analyst, ProjectRole::Approver));
    let checker = Auth::User(env.user_token(env.approver, ProjectRole::Approver));
    let rule = json!({
        "code": "RL-TEST-BIG", "name": "Big amount", "kind": "simple", "typologies": ["other"],
        "event_types": ["transaction"], "risk_score": 40,
        "definition": {"kind": "simple", "when": {"left": {"type": "field", "path": "event.amount"}, "op": "gt",
                                                  "right": {"type": "const", "value": 500}}}
    });
    let (s, created) = env
        .call("POST", &env.p("/rules"), &maker, Some(rule.clone()))
        .await;
    assert_eq!(s, StatusCode::CREATED, "{created}");
    let id = created["id"].as_str().unwrap().to_string();
    assert_eq!(created["status"], "draft");

    let (s, _) = env
        .call("POST", &env.p(&format!("/rules/{id}/submit")), &maker, None)
        .await;
    assert_eq!(s, StatusCode::OK);
    // Self-approval is refused.
    let (s, err) = env
        .call(
            "POST",
            &env.p(&format!("/rules/{id}/approve")),
            &maker,
            Some(json!({"target_status": "shadow"})),
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{err}");
    // A service token cannot approve either.
    let (s, _) = env
        .call(
            "POST",
            &env.p(&format!("/rules/{id}/approve")),
            &Auth::Service(Some(env.approver)),
            None,
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, approved) = env
        .call(
            "POST",
            &env.p(&format!("/rules/{id}/approve")),
            &checker,
            Some(json!({"target_status": "shadow"})),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{approved}");
    assert_eq!(approved["status"], "shadow");
    assert_eq!(approved["serving"]["shadow_version"], 1);

    // Ruleset with the rule, approved active by the checker.
    let (s, rs) = env
        .call(
            "POST",
            &env.p("/rulesets"),
            &maker,
            Some(json!({"code": "RS-TEST", "name": "Test", "event_types": ["transaction"]})),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{rs}");
    let rs_id = rs["id"].as_str().unwrap().to_string();
    let (s, _) = env
        .call(
            "PUT",
            &env.p(&format!("/rulesets/{rs_id}/rules")),
            &maker,
            Some(json!([{"rule_id": id, "weight": 1.0}])),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    env.call("POST", &env.p(&format!("/rulesets/{rs_id}/submit")), &maker, None)
        .await;
    let (s, rs) = env
        .call(
            "POST",
            &env.p(&format!("/rulesets/{rs_id}/approve")),
            &checker,
            None,
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{rs}");
    // Created as v1, membership change = v2; the approved (live) version is v2.
    assert_eq!(rs["serving"]["live_version"], 2, "{rs}");

    let c = env.customer("C").await;
    let ev = env.event(c, anchor(), json!({"amount": 900})).await;
    let int = Auth::Service(None);
    let body = || eval_body(ev, c, json!({"amount": 900}));
    let (_, resp) = env
        .call(
            "POST",
            &format!("/v1/projects/{}/evaluate", env.project),
            &int,
            Some(body()),
        )
        .await;
    let trace = find(&resp["rule_results"], "RL-TEST-BIG");
    assert_eq!(trace.len(), 1);
    assert_eq!(trace[0]["shadow"], true);
    assert_eq!(trace[0]["outcome"], "match");
    assert_eq!(resp["rules_score"], 0.0, "shadow rules never score");

    // Promote v1 to active.
    env.call("POST", &env.p(&format!("/rules/{id}/submit")), &maker, None)
        .await;
    let (s, r) = env
        .call(
            "POST",
            &env.p(&format!("/rules/{id}/approve")),
            &checker,
            Some(json!({"target_status": "active"})),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let (_, resp) = env
        .call(
            "POST",
            &format!("/v1/projects/{}/evaluate", env.project),
            &int,
            Some(body()),
        )
        .await;
    assert!(
        (resp["rules_score"].as_f64().unwrap() - 40.0).abs() < 1e-6,
        "{resp}"
    );

    // Edit (v2, threshold 5000): the rule goes back to draft but v1 keeps serving until v2 is approved.
    let mut v2 = rule.clone();
    v2["definition"]["when"]["right"]["value"] = json!(5000);
    let (s, edited) = env
        .call("PUT", &env.p(&format!("/rules/{id}")), &maker, Some(v2))
        .await;
    assert_eq!(s, StatusCode::OK, "{edited}");
    assert_eq!(edited["status"], "draft");
    assert_eq!(edited["current_version"], 2);
    assert_eq!(edited["serving"]["live_version"], 1);
    let (_, resp) = env
        .call(
            "POST",
            &format!("/v1/projects/{}/evaluate", env.project),
            &int,
            Some(body()),
        )
        .await;
    assert!(
        (resp["rules_score"].as_f64().unwrap() - 40.0).abs() < 1e-6,
        "v1 still live: {resp}"
    );

    // Detail endpoint exposes versions + approval ledger.
    let (_, detail) = env
        .call("GET", &env.p(&format!("/rules/{id}")), &checker, None)
        .await;
    assert_eq!(detail["versions"].as_array().unwrap().len(), 2);
    // Two submit → approve cycles = two decided ledger rows (the refused attempts left no trace).
    assert_eq!(
        detail["approvals"].as_array().unwrap().len(),
        2,
        "{}",
        detail["approvals"]
    );

    // Retire → nothing serves.
    let (s, _) = env
        .call("POST", &env.p(&format!("/rules/{id}/retire")), &checker, None)
        .await;
    assert_eq!(s, StatusCode::OK);
    let (_, resp) = env
        .call(
            "POST",
            &format!("/v1/projects/{}/evaluate", env.project),
            &int,
            Some(body()),
        )
        .await;
    assert!(find(&resp["rule_results"], "RL-TEST-BIG").is_empty());
}

#[tokio::test]
async fn backtest_precision_recall_on_labelled_events() {
    let Some(env) = env().await else { return };
    let analyst = Auth::User(env.user_token(env.analyst, ProjectRole::Analyst));
    let c = env.customer("C").await;
    let now = Utc::now();
    let mk = |amount: i64, hours: i64| {
        let env = &env;
        async move {
            env.event(c, now - chrono::Duration::hours(hours), json!({"amount": amount}))
                .await
        }
    };
    let e1 = mk(1000, 1).await;
    let e2 = mk(1000, 2).await;
    let e3 = mk(1000, 3).await;
    let e4 = mk(10, 4).await;
    let _e5 = mk(10, 5).await;
    env.label(e1, "fraud").await;
    env.label(e2, "fraud").await;
    env.label(e3, "legit").await;
    env.label(e4, "fraud").await;
    let rule = json!({
        "code": "RL-BT", "name": "BT", "kind": "simple", "event_types": ["transaction"], "risk_score": 10,
        "definition": {"kind": "simple", "when": {"left": {"type": "field", "path": "event.amount"}, "op": "gt",
                                                  "right": {"type": "const", "value": 500}}}
    });
    let (s, r) = env
        .call(
            "POST",
            &env.p("/rules/backtest"),
            &analyst,
            Some(json!({"rule": rule, "since_days": 2})),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["evaluated"], 5);
    assert_eq!(r["matched"], 3);
    assert!((r["precision"].as_f64().unwrap() - 2.0 / 3.0).abs() < 1e-9, "{r}");
    assert!((r["recall"].as_f64().unwrap() - 2.0 / 3.0).abs() < 1e-9, "{r}");
    assert_eq!(r["sample_matches"].as_array().unwrap().len(), 3);

    // Stored-rule backtest and ruleset backtest (histogram + decisions).
    let (_, created) = env.call("POST", &env.p("/rules"), &analyst, Some(rule)).await;
    let id = created["id"].as_str().unwrap().to_string();
    let (s, r) = env
        .call(
            "POST",
            &env.p(&format!("/rules/{id}/backtest")),
            &analyst,
            Some(json!({"since_days": 2})),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["matched"], 3);
    let (_, rs) = env
        .call(
            "POST",
            &env.p("/rulesets"),
            &analyst,
            Some(json!({"code": "RS-BT", "name": "BT", "aggregation": "sum"})),
        )
        .await;
    let rs_id = rs["id"].as_str().unwrap().to_string();
    env.call(
        "PUT",
        &env.p(&format!("/rulesets/{rs_id}/rules")),
        &analyst,
        Some(json!([{"rule_id": id, "weight": 6}])),
    )
    .await;
    let (s, r) = env
        .call(
            "POST",
            &env.p(&format!("/rulesets/{rs_id}/backtest")),
            &analyst,
            Some(json!({"since_days": 2})),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["decision_distribution"]["review"], 3, "{r}");
    assert_eq!(r["decision_distribution"]["approve"], 2, "{r}");
    assert_eq!(
        r["score_histogram"][6]["count"], 3,
        "score 60 lands in the 60-69 bucket: {r}"
    );

    // The project's decision_thresholds are used (decline >= 60) ...
    sqlx::query(
        "INSERT INTO core.project_settings (tenant_id, project_id, key, value) \
         VALUES ($1, $2, 'decision_thresholds', '{\"review\": 50, \"decline\": 60}')",
    )
    .bind(env.tenant.as_uuid())
    .bind(env.project.as_uuid())
    .execute(&env.migrator)
    .await
    .unwrap();
    let (_, r) = env
        .call(
            "POST",
            &env.p(&format!("/rulesets/{rs_id}/backtest")),
            &analyst,
            Some(json!({"since_days": 2})),
        )
        .await;
    assert_eq!(
        r["decision_distribution"]["decline"], 3,
        "project thresholds: {r}"
    );
    // ... unless the request overrides them.
    let (_, r) = env
        .call(
            "POST",
            &env.p(&format!("/rulesets/{rs_id}/backtest")),
            &analyst,
            Some(json!({"since_days": 2, "thresholds": {"review": 50, "decline": 80}})),
        )
        .await;
    assert_eq!(r["decision_distribution"]["review"], 3, "override: {r}");
}

#[tokio::test]
async fn csv_import_and_list_management() {
    let Some(env) = env().await else { return };
    let analyst = Auth::User(env.user_token(env.analyst, ProjectRole::Analyst));
    let (s, list) = env
        .call(
            "POST",
            &env.p("/reference-lists"),
            &analyst,
            Some(
                json!({"name": "merchant_caps", "list_type": "lookup", "key_kind": "merchant_id",
                        "columns": [{"name": "max_amount", "type": "number"}]}),
            ),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{list}");
    let id = list["id"].as_str().unwrap().to_string();

    let boundary = "XBOUNDARYX";
    let csv = "key,max_amount,reason\nM-1,5000000,cap\nM-2,abc,bad\nM-3,100,\n";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"caps.csv\"\r\nContent-Type: text/csv\r\n\r\n{csv}\r\n--{boundary}--\r\n"
    );
    let token = env.user_token(env.analyst, ProjectRole::Analyst);
    let req = Request::builder()
        .method("POST")
        .uri(env.p(&format!("/reference-lists/{id}/import")))
        .header("authorization", format!("Bearer {token}"))
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();
    let (s, out) = env.send(req).await;
    assert_eq!(s, StatusCode::OK, "{out}");
    assert_eq!(out["upserted"], 2);
    assert_eq!(out["errors"].as_array().unwrap().len(), 1);
    assert_eq!(out["errors"][0]["line"], 3);

    let (_, entries) = env
        .call(
            "GET",
            &env.p(&format!("/reference-lists/{id}/entries")),
            &analyst,
            None,
        )
        .await;
    assert_eq!(entries["total"], 2);
    assert_eq!(entries["items"][0]["attributes"]["max_amount"], 5_000_000.0);

    // Tenant-wide list: visible (read-only) from the project, writable only by a tenant admin.
    let admin = Auth::User(env.tenant_admin_token(env.approver));
    let (s, tl) = env
        .call(
            "POST",
            &format!("/api/v1/tenants/{}/reference-lists", env.tenant),
            &admin,
            Some(json!({"name": "company_bl", "list_type": "blacklist"})),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{tl}");
    let (s, _) = env
        .call(
            "POST",
            &format!("/api/v1/tenants/{}/reference-lists", env.tenant),
            &analyst,
            Some(json!({"name": "nope", "list_type": "blacklist"})),
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (_, lists) = env.call("GET", &env.p("/reference-lists"), &analyst, None).await;
    let scopes: Vec<&str> = lists
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["scope"].as_str().unwrap())
        .collect();
    assert!(scopes.contains(&"tenant") && scopes.contains(&"project"));
    let tl_id = tl["id"].as_str().unwrap();
    let (s, _) = env
        .call(
            "DELETE",
            &env.p(&format!("/reference-lists/{tl_id}")),
            &analyst,
            None,
        )
        .await;
    assert_eq!(
        s,
        StatusCode::NOT_FOUND,
        "a project cannot delete a tenant-wide list"
    );

    // A list referenced by a rule cannot be deleted.
    let rule = json!({"code": "RL-REF", "name": "ref", "kind": "reference", "risk_score": 10,
        "definition": {"kind": "reference", "list": "merchant_caps", "key": {"type": "field", "path": "event.merchant_id"}, "mode": "exists"}});
    let (s, r) = env.call("POST", &env.p("/rules"), &analyst, Some(rule)).await;
    assert_eq!(s, StatusCode::CREATED, "{r}");
    let (s, _) = env
        .call(
            "DELETE",
            &env.p(&format!("/reference-lists/{id}")),
            &analyst,
            None,
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
}

#[tokio::test]
async fn validate_formula_proposals_and_internal_reads() {
    let Some(env) = env().await else { return };
    let int = Auth::Service(Some(env.analyst));
    let checker = Auth::User(env.user_token(env.approver, ProjectRole::Approver));
    let author = Auth::User(env.user_token(env.analyst, ProjectRole::Approver));
    let good = json!({
        "code": "RL-LLM-1", "name": "Many cards per device", "kind": "velocity", "typologies": ["carding"],
        "event_types": ["transaction"], "risk_score": 50,
        "definition": {"kind": "velocity", "group_by": ["device_id"], "window": {"duration": "1h"},
                       "aggregate": {"fn": "distinct_count", "field": "instrument_fingerprint"},
                       "compare": {"op": "gte", "right": {"type": "const", "value": 4}}}
    });
    let (s, v) = env
        .call("POST", &env.p("/rules/validate"), &int, Some(good.clone()))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["valid"], true);
    let mut bad = good.clone();
    bad["definition"]["group_by"] = json!(["event.nope"]);
    let (s, v) = env
        .call("POST", &env.p("/rules/validate"), &int, Some(bad.clone()))
        .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(v["valid"], false);
    assert!(!v["errors"].as_array().unwrap().is_empty());

    let (s, f) = env
        .call(
            "POST",
            &env.p("/formulas/evaluate"),
            &int,
            Some(json!({"expr": "F(x,y,z) = 2x + 2^y / z^2", "variables": {"x": 10, "y": 3, "z": 2}})),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{f}");
    assert_eq!(f["value"], 22.0);
    let (s, f) = env
        .call(
            "POST",
            &env.p("/formulas/evaluate"),
            &int,
            Some(json!({"expr": "2 * (x +", "variables": {"x": 1}})),
        )
        .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(f["position"].as_u64().is_some());

    // LLM proposal (INT + X-Actor): stored with validation + backtest.
    let proposal = json!({
        "source": "llm", "proposal_type": "new_rule", "definition": good, "rationale": "POJK requires card testing controls",
        "citations": [{"code": "POJK-12-2024", "section": "Pasal 5", "excerpt": "..."}], "evidence": {"clusters": [3]},
        "report_id": Uuid::new_v4(), "llm_model": "qwen2.5:7b-instruct"
    });
    let (s, p) = env.call("POST", &env.p("/proposals"), &int, Some(proposal)).await;
    assert_eq!(s, StatusCode::CREATED, "{p}");
    assert!(p["id"].as_str().is_some());
    assert_eq!(p["validation"]["valid"], true);
    assert!(p["backtest"]["evaluated"].is_number(), "{p}");
    let pid = p["id"].as_str().unwrap().to_string();

    let (s, invalid) = env
        .call(
            "POST",
            &env.p("/proposals"),
            &int,
            Some(json!({"source": "llm", "proposal_type": "new_rule", "definition": bad, "rationale": "x"})),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(invalid["validation"]["valid"], false);
    assert!(invalid["backtest"].is_null());

    // Author (X-Actor) cannot approve their own proposal; another approver can → shadow rule.
    let (s, _) = env
        .call(
            "POST",
            &env.p(&format!("/proposals/{pid}/approve")),
            &author,
            None,
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, applied) = env
        .call(
            "POST",
            &env.p(&format!("/proposals/{pid}/approve")),
            &checker,
            None,
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{applied}");
    assert_eq!(applied["status"], "applied");
    let rule_id = applied["applied_rule_id"].as_str().unwrap().to_string();
    let (_, rule) = env
        .call("GET", &env.p(&format!("/rules/{rule_id}")), &int, None)
        .await;
    assert_eq!(rule["status"], "shadow");
    assert_eq!(rule["serving"]["shadow_version"], 1);
    assert!(
        rule["serving"]["live_version"].is_null(),
        "an LLM proposal never goes live directly"
    );

    // INT callers can read (llm-service tools).
    let (s, list) = env
        .call("GET", &env.p("/rules?page_size=200&status=shadow"), &int, None)
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(list["total"], 1);
    let (s, perf) = env
        .call("GET", &env.p("/rules/performance?since_days=7"), &int, None)
        .await;
    assert_eq!(s, StatusCode::OK, "{perf}");
    assert_eq!(perf["items"].as_array().unwrap().len(), 1);
    let (s, props) = env
        .call("GET", &env.p("/proposals?status=pending"), &int, None)
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(props["total"], 1);

    // OpenAPI document is served.
    let (s, doc) = env.call("GET", "/openapi.json", &int, None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(doc["paths"].as_object().unwrap().len() > 30);
}

/// Creates a simple rule `event.amount > threshold` and gets it approved live (maker ≠ checker).
async fn live_rule(env: &Env, maker: &Auth, checker: &Auth, code: &str, risk: f64, threshold: f64) -> String {
    let rule = json!({
        "code": code, "name": code, "kind": "simple", "event_types": ["transaction"], "risk_score": risk,
        "definition": {"kind": "simple", "when": {"left": {"type": "field", "path": "event.amount"}, "op": "gt",
                                                  "right": {"type": "const", "value": threshold}}}
    });
    let (s, r) = env.call("POST", &env.p("/rules"), maker, Some(rule)).await;
    assert_eq!(s, StatusCode::CREATED, "{r}");
    let id = r["id"].as_str().unwrap().to_string();
    env.call("POST", &env.p(&format!("/rules/{id}/submit")), maker, None)
        .await;
    let (s, r) = env
        .call("POST", &env.p(&format!("/rules/{id}/approve")), checker, None)
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    id
}

#[tokio::test]
async fn ruleset_edits_are_versioned_and_approval_gated() {
    let Some(env) = env().await else { return };
    let maker = Auth::User(env.user_token(env.analyst, ProjectRole::Approver));
    let checker = Auth::User(env.user_token(env.approver, ProjectRole::Approver));
    let int = Auth::Service(None);
    let r1 = live_rule(&env, &maker, &checker, "RL-RS-A", 40.0, 100.0).await;
    let r2 = live_rule(&env, &maker, &checker, "RL-RS-B", 30.0, 100.0).await;

    // v1 (create) → v2 (members [A]) → approved active.
    let (_, rs) = env
        .call(
            "POST",
            &env.p("/rulesets"),
            &maker,
            Some(json!({"code": "RS-VER", "name": "Versioned", "aggregation": "sum"})),
        )
        .await;
    let rs_id = rs["id"].as_str().unwrap().to_string();
    env.call(
        "PUT",
        &env.p(&format!("/rulesets/{rs_id}/rules")),
        &maker,
        Some(json!([{"rule_id": r1}])),
    )
    .await;
    env.call("POST", &env.p(&format!("/rulesets/{rs_id}/submit")), &maker, None)
        .await;
    // Self-approval refused.
    let (s, err) = env
        .call(
            "POST",
            &env.p(&format!("/rulesets/{rs_id}/approve")),
            &maker,
            None,
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{err}");
    let (s, rs) = env
        .call(
            "POST",
            &env.p(&format!("/rulesets/{rs_id}/approve")),
            &checker,
            None,
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{rs}");
    assert_eq!(rs["serving"]["live_version"], 2);

    let c = env.customer("C").await;
    let ev = env.event(c, anchor(), json!({"amount": 900})).await;
    let evaluate = || async {
        let (s, r) = env
            .call(
                "POST",
                &format!("/v1/projects/{}/evaluate", env.project),
                &int,
                Some(eval_body(ev, c, json!({"amount": 900}))),
            )
            .await;
        assert_eq!(s, StatusCode::OK, "{r}");
        r
    };
    let score = |r: &Value| r["rules_score"].as_f64().unwrap();
    assert!((score(&evaluate().await) - 40.0).abs() < 1e-6);

    // Edits on the live ruleset: members [A, B] (v3) and max_score 10 (v4). Nothing changes until approved.
    let (s, draft) = env
        .call(
            "PUT",
            &env.p(&format!("/rulesets/{rs_id}/rules")),
            &maker,
            Some(json!([{"rule_id": r1}, {"rule_id": r2}])),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{draft}");
    assert_eq!(draft["version"], 3);
    assert_eq!(draft["status"], "draft");
    assert_eq!(draft["serving"]["live_version"], 2, "v2 keeps serving");
    let (s, draft) = env
        .call(
            "PUT",
            &env.p(&format!("/rulesets/{rs_id}")),
            &maker,
            Some(json!({"code": "RS-VER", "name": "Versioned", "aggregation": "sum", "max_score": 10, "change_note": "cap"})),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{draft}");
    assert_eq!(draft["version"], 4);
    let r = evaluate().await;
    assert!((score(&r) - 40.0).abs() < 1e-6, "draft edits must not serve: {r}");
    assert!(
        find(&r["rule_results"], "RL-RS-B").is_empty(),
        "B is only in the draft"
    );

    // v4 approved into shadow: evaluated side by side, never scores.
    env.call("POST", &env.p(&format!("/rulesets/{rs_id}/submit")), &maker, None)
        .await;
    let (s, rs) = env
        .call(
            "POST",
            &env.p(&format!("/rulesets/{rs_id}/approve")),
            &checker,
            Some(json!({"target_status": "shadow"})),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{rs}");
    assert_eq!(rs["serving"]["live_version"], 2);
    assert_eq!(rs["serving"]["shadow_version"], 4);
    assert_eq!(rs["status"], "active", "champion still live");
    let r = evaluate().await;
    assert!((score(&r) - 40.0).abs() < 1e-6, "{r}");
    let scores = r["rulesets"].as_array().unwrap();
    assert_eq!(scores.len(), 2);
    let shadow = scores.iter().find(|s| s["shadow"] == true).unwrap();
    // rule-dsl.md "Shadow semantics": a shadow ruleset reports its own would-be score (A 40 + B 30, capped by
    // max_score 10) so the challenger can be compared, but it stays out of rules_score (still 40 above).
    assert!(
        (shadow["score"].as_f64().unwrap() - 10.0).abs() < 1e-6,
        "{shadow}"
    );
    assert!(
        find(&r["rule_results"], "RL-RS-A")
            .iter()
            .any(|t| t["shadow"] == true),
        "v4 traces A in shadow"
    );
    let b = find(&r["rule_results"], "RL-RS-B");
    assert_eq!(b.len(), 1);
    assert_eq!(b[0]["shadow"], true);

    // Promote v4 → live.
    env.call("POST", &env.p(&format!("/rulesets/{rs_id}/submit")), &maker, None)
        .await;
    let (s, rs) = env
        .call(
            "POST",
            &env.p(&format!("/rulesets/{rs_id}/approve")),
            &checker,
            None,
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{rs}");
    assert_eq!(rs["serving"]["live_version"], 4);
    assert!(rs["serving"]["shadow_version"].is_null());
    let r = evaluate().await;
    assert!((score(&r) - 10.0).abs() < 1e-6, "{r}");

    // Detail: 4 immutable snapshots, ledger of 3 approvals.
    let (_, detail) = env
        .call("GET", &env.p(&format!("/rulesets/{rs_id}")), &checker, None)
        .await;
    let versions = detail["versions"].as_array().unwrap();
    assert_eq!(versions.len(), 4);
    assert_eq!(versions[0]["version"], 4);
    assert_eq!(versions[0]["change_note"], "cap");
    assert_eq!(versions[0]["config"]["members"].as_array().unwrap().len(), 2);
    assert_eq!(
        versions[2]["config"]["members"].as_array().unwrap().len(),
        1,
        "v2 snapshot unchanged"
    );
    let approved = detail["approvals"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["decision"] == "approved")
        .count();
    assert_eq!(approved, 3);

    // Retire → nothing serves.
    env.call(
        "POST",
        &env.p(&format!("/rulesets/{rs_id}/retire")),
        &checker,
        None,
    )
    .await;
    let r = evaluate().await;
    assert_eq!(score(&r), 0.0);
    assert!(r["rulesets"].as_array().unwrap().is_empty());
}
