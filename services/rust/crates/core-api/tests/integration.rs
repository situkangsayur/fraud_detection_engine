//! Integration tests: real Postgres (RLS, migrations, grants) + wiremock fakes for the engines.
//!
//! Run with `tests/run-integration.sh` (starts a throwaway Postgres). Without the
//! `CORE_API_TEST_*` env vars every test is skipped (so `cargo test` stays hermetic).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use core_api::state::{http_engines, AppState, Caches, RuntimeConfig};
use http_body_util::BodyExt;
use platform::auth::{AuthState, PgProjectDirectory};
use platform::config::Secret;
use serde_json::{json, Value};
use sqlx::PgPool;
use tokio::sync::OnceCell;
use tower::ServiceExt;
use uuid::Uuid;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

const INTERNAL: &str = "internal-test-token";
const PASSWORD: &str = "correct-horse-battery";

static MIGRATED: OnceCell<()> = OnceCell::const_new();

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

async fn migrate_once() {
    MIGRATED
        .get_or_init(|| async {
            let url = env("CORE_API_TEST_MIGRATOR_URL").expect("migrator url");
            let dir = env("CORE_API_TEST_MIGRATIONS_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../db/migrations")
                });
            let pool = platform::db::connect_url(&url, 2)
                .await
                .expect("migrator connect");
            platform::db::run_migrations(&pool, &dir)
                .await
                .expect("migrations");
        })
        .await;
}

struct Harness {
    app: Router,
    state: AppState,
    pool: PgPool,
    mocks: MockServer,
}

/// `None` when the integration environment is not configured.
async fn harness() -> Option<Harness> {
    let app_url = env("CORE_API_TEST_APP_URL")?;
    migrate_once().await;
    let pool = platform::db::connect_url(&app_url, 5).await.expect("app connect");
    let mocks = MockServer::start().await;
    let token = Secret::new(INTERNAL);
    let engines = http_engines(&token, &mocks.uri(), &mocks.uri(), &mocks.uri()).expect("engines");
    let state = AppState {
        pool: pool.clone(),
        auth: AuthState::new(
            &Secret::new("jwt-secret-for-tests-jwt-secret-for-tests"),
            &token,
            Arc::new(PgProjectDirectory::new(pool.clone())),
        ),
        cfg: Arc::new(RuntimeConfig {
            jwt_ttl: Duration::from_secs(600),
            refresh_ttl_days: 1,
            pii_pepper: Secret::new("pepper"),
            internal_token: token,
        }),
        engines,
        caches: Arc::new(Caches::default()),
    };
    Some(Harness {
        app: core_api::router(state.clone()),
        state,
        pool,
        mocks,
    })
}

macro_rules! harness_or_skip {
    () => {
        match harness().await {
            Some(h) => h,
            None => {
                eprintln!("skipped: CORE_API_TEST_APP_URL not set (use tests/run-integration.sh)");
                return;
            }
        }
    };
}

impl Harness {
    async fn call(
        &self,
        method: &str,
        uri: &str,
        auth: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        self.call_h(
            method,
            uri,
            auth.map(|t| ("authorization", format!("Bearer {t}"))),
            body,
        )
        .await
    }

    async fn call_h(
        &self,
        method: &str,
        uri: &str,
        header: Option<(&str, String)>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut req = Request::builder().method(method).uri(uri);
        if let Some((k, v)) = header {
            req = req.header(k, v);
        }
        let req = match body {
            Some(b) => req
                .header("content-type", "application/json")
                .body(Body::from(b.to_string()))
                .unwrap(),
            None => req.body(Body::empty()).unwrap(),
        };
        let resp = self.app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let v = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, v)
    }

    async fn login(&self, email: &str) -> (String, String) {
        let (s, v) = self
            .call(
                "POST",
                "/api/v1/auth/login",
                None,
                Some(json!({ "email": email, "password": PASSWORD })),
            )
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        (
            v["access_token"].as_str().unwrap().to_string(),
            v["refresh_token"].as_str().unwrap().to_string(),
        )
    }

    /// Platform admin + tenant with tenant admin; returns the tenant admin's token and tenant id.
    async fn tenant(&self, tag: &str) -> (String, String) {
        let pa = format!("pa-{tag}@it.local");
        core_api::bootstrap::ensure_platform_admin(&self.state, &pa, &Secret::new(PASSWORD))
            .await
            .unwrap();
        let (pa_token, _) = self.login(&pa).await;
        let ta = format!("ta-{tag}@it.local");
        let (s, v) = self
            .call(
                "POST",
                "/api/v1/tenants",
                Some(&pa_token),
                Some(json!({ "slug": format!("t-{tag}"), "name": "IT tenant",
                             "admin": { "email": ta, "full_name": "Tenant Admin", "password": PASSWORD } })),
            )
            .await;
        assert_eq!(s, StatusCode::CREATED, "{v}");
        let (ta_token, _) = self.login(&ta).await;
        (ta_token, v["id"].as_str().unwrap().to_string())
    }

    async fn project(&self, token: &str, slug: &str) -> String {
        let (s, v) = self
            .call(
                "POST",
                "/api/v1/projects",
                Some(token),
                Some(json!({ "slug": slug, "name": "Checkout", "stage": "pre_payment", "template": "none" })),
            )
            .await;
        assert_eq!(s, StatusCode::CREATED, "{v}");
        v["id"].as_str().unwrap().to_string()
    }

    async fn count(&self, tenant: &str, sql: &str) -> i64 {
        let mut tx = platform::db::TenantTx::begin(&self.pool, platform::TenantId(tenant.parse().unwrap()))
            .await
            .unwrap();
        let (n,): (i64,) = sqlx::query_as(sql).fetch_one(&mut **tx).await.unwrap();
        tx.commit().await.unwrap();
        n
    }
}

fn tag() -> String {
    Uuid::new_v4().simple().to_string()[..10].to_string()
}

fn graph_metrics() -> Value {
    json!({ "distance_to_fraud": 2, "fraud_neighbors_1": 0, "fraud_neighbors_2": 1, "component_size": 3,
            "shared_entity_count": 1, "degree": 1, "community_fraud_rate": null, "shared_with_fraud_kinds": [] })
}

fn evaluate_response(score: f64) -> Value {
    json!({ "rules_score": score, "rulesets": [], "rule_results": [],
            "actions": { "force_decline": false, "force_approve": false, "force_review": false },
            "reasons": [ { "code": "RL-IT-1", "engine": "rules", "contribution": score, "message": "test rule" } ],
            "duration_ms": 1.0 })
}

/// Healthy graph + rules; ML predict 0.7, no active unsupervised model.
async fn mount_happy_engines(m: &MockServer, rules_score: f64) {
    Mock::given(method("POST"))
        .and(path_regex(r"^/v1/projects/[^/]+/links$"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "entity_ids": [1], "new_links": 1, "similarity_links_created": 0 })),
        )
        .mount(m)
        .await;
    Mock::given(method("POST"))
        .and(path_regex(r"^/v1/projects/[^/]+/metrics$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(graph_metrics()))
        .mount(m)
        .await;
    Mock::given(method("POST"))
        .and(path_regex(r"^/v1/projects/[^/]+/supervised/predict$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "event_id": null, "model_id": Uuid::nil(), "model_version": 3, "algorithm": "mlp_backprop",
            "fraud_probability": 0.7, "top_features": [] })))
        .mount(m)
        .await;
    Mock::given(method("POST"))
        .and(path_regex(r"^/v1/projects/[^/]+/unsupervised/score$"))
        .respond_with(
            ResponseTemplate::new(404).set_body_json(
                json!({ "type": "no_active_model", "title": "no active model", "status": 404 }),
            ),
        )
        .mount(m)
        .await;
    Mock::given(method("POST"))
        .and(path_regex(r"^/v1/projects/[^/]+/evaluate$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(evaluate_response(rules_score)))
        .mount(m)
        .await;
}

fn custom_mapping() -> Value {
    json!({
        "event_type": { "from": "jenis", "value_map": { "PEMBELIAN": "transaction", "RETUR": "refund" }, "default": "transaction" },
        "event": {
            "external_id": { "from": "no_ref" },
            "occurred_at": { "from": "waktu", "transform": [{ "fn": "parse_datetime", "format": "%d/%m/%Y %H:%M:%S", "timezone": "Asia/Jakarta" }] },
            "customer_external_id": { "from": "pengguna.id", "transform": [{ "fn": "to_string" }] },
            "amount": { "from": "nominal", "transform": [{ "fn": "to_number", "locale": "id" }] },
            "instrument_fingerprint": { "from": "pembayaran.no_kartu", "transform": [{ "fn": "hash_pan" }] },
            "card_bin": { "from": "pembayaran.no_kartu", "transform": [{ "fn": "pan_bin", "length": 6 }] },
            "card_last4": { "from": "pembayaran.no_kartu", "transform": [{ "fn": "pan_last4" }] },
            "device_id": { "from": "perangkat.id" },
            "ip_address": { "from": "perangkat.ip" }
        },
        "customer": {
            "email": { "from": "pengguna.email", "transform": [{ "fn": "normalize_email" }] },
            "phone": { "from": "pengguna.no_hp", "transform": [{ "fn": "normalize_phone", "default_country": "ID" }] }
        },
        "drop_fields": ["pembayaran.no_kartu", "pembayaran.cvv"]
    })
}

fn custom_record(no_ref: &str) -> Value {
    json!({
        "no_ref": no_ref, "waktu": "23/09/2026 14:30:00", "jenis": "PEMBELIAN", "nominal": "1.500.000,00",
        "pengguna": { "id": "U-1", "email": "Budi.S+x@gmail.com", "no_hp": "0812-3456-7890" },
        "pembayaran": { "no_kartu": "4111 1111 1111 1111", "cvv": "123" },
        "perangkat": { "id": "dev-1", "ip": "36.72.10.4" },
        "alasan_retur": "barang rusak"
    })
}

/// Creates a webhook source with the custom mapping activated; returns (source_id, api_key, slug).
async fn webhook_source(h: &Harness, token: &str, pid: &str) -> (String, String, String) {
    let slug = format!("wh-{}", tag());
    let (s, v) = h
        .call(
            "POST",
            &format!("/api/v1/projects/{pid}/data-sources"),
            Some(token),
            Some(json!({ "slug": slug, "name": "Webhook", "kind": "webhook" })),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let sid = v["id"].as_str().unwrap().to_string();
    let key = v["api_key"].as_str().unwrap().to_string();
    let (s, v) = h
        .call(
            "POST",
            &format!("/api/v1/projects/{pid}/data-sources/{sid}/mappings"),
            Some(token),
            Some(json!({ "mapping": custom_mapping() })),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    assert_eq!(v["version"], json!(1));
    let (s, v) = h
        .call(
            "POST",
            &format!("/api/v1/projects/{pid}/data-sources/{sid}/mappings/1/activate"),
            Some(token),
            None,
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    (sid, key, slug)
}

// ---------------------------------------------------------------------------------------------

#[tokio::test]
async fn login_refresh_rotation_and_reuse_detection() {
    let h = harness_or_skip!();
    let pa = format!("pa-{}@it.local", tag());
    core_api::bootstrap::ensure_platform_admin(&h.state, &pa, &Secret::new(PASSWORD))
        .await
        .unwrap();
    let (s, _) = h
        .call(
            "POST",
            "/api/v1/auth/login",
            None,
            Some(json!({ "email": pa, "password": "wrong-password" })),
        )
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (access, refresh1) = h.login(&pa).await;
    let (s, me) = h.call("GET", "/api/v1/me", Some(&access), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(me["user"]["is_platform_admin"], json!(true));

    let (s, v) = h
        .call(
            "POST",
            "/api/v1/auth/refresh",
            None,
            Some(json!({ "refresh_token": refresh1 })),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let refresh2 = v["refresh_token"].as_str().unwrap().to_string();
    assert_ne!(refresh1, refresh2);

    // reuse of the rotated token → 401 and the whole family is revoked
    let (s, _) = h
        .call(
            "POST",
            "/api/v1/auth/refresh",
            None,
            Some(json!({ "refresh_token": refresh1 })),
        )
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = h
        .call(
            "POST",
            "/api/v1/auth/refresh",
            None,
            Some(json!({ "refresh_token": refresh2 })),
        )
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn projects_are_isolated_between_tenants() {
    let h = harness_or_skip!();
    let (ta_a, tenant_a) = h.tenant(&tag()).await;
    let (ta_b, _tenant_b) = h.tenant(&tag()).await;
    let pid = h.project(&ta_a, "checkout").await;

    let (s, v) = h
        .call("GET", &format!("/api/v1/projects/{pid}"), Some(&ta_a), None)
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["stage"], json!("pre_payment"));
    // the other tenant's admin cannot see it (404, not 403)
    let (s, _) = h
        .call("GET", &format!("/api/v1/projects/{pid}"), Some(&ta_b), None)
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (_, list_b) = h.call("GET", "/api/v1/projects", Some(&ta_b), None).await;
    assert_eq!(list_b["total"], json!(0));
    // settings defaults + canonical source were created
    let (_, settings) = h
        .call(
            "GET",
            &format!("/api/v1/projects/{pid}/settings"),
            Some(&ta_a),
            None,
        )
        .await;
    assert_eq!(settings["decision_thresholds"]["review"], json!(50.0));
    assert_eq!(
        h.count(
            &tenant_a,
            &format!("SELECT count(*) FROM core.data_sources WHERE project_id = '{pid}'")
        )
        .await,
        1
    );
    // RLS: without a tenant context the app role sees nothing
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM core.projects")
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
    // invalid settings are rejected with field errors
    let (s, v) = h
        .call(
            "PUT",
            &format!("/api/v1/projects/{pid}/settings/decision_thresholds"),
            Some(&ta_a),
            Some(json!({ "review": 90, "decline": 10 })),
        )
        .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
}

#[tokio::test]
async fn webhook_ingest_with_custom_mapping_scores_opens_case_and_dedupes() {
    let h = harness_or_skip!();
    mount_happy_engines(&h.mocks, 60.0).await;
    let (ta, tenant) = h.tenant(&tag()).await;
    let pid = h.project(&ta, "checkout").await;
    let (sid, key, slug) = webhook_source(&h, &ta, &pid).await;

    let (s, d) = h
        .call_h(
            "POST",
            &format!("/api/v1/ingest/{slug}"),
            Some(("x-api-key", key.clone())),
            Some(custom_record("CHK-1")),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{d}");
    // noisy-OR (default): e = w/w_max = (1, 2/3, 1/3) for rules 60, supervised 70, graph 70
    // (unsupervised absent) → 100·(1 − 0.4·0.3^(2/3)·0.3^(1/3)) = 100·(1 − 0.4·0.3) = 88
    eprintln!(
        "pipeline latency_ms (debug build, local wiremock engines): {}",
        d["latency_ms"]
    );
    assert_eq!(d["final_score"], json!(88.0));
    assert_eq!(d["decision"], json!("decline"));
    assert_eq!(d["degraded"], json!([]));
    assert!(d["case_id"].is_string(), "{d}");
    assert_eq!(d["ml"]["supervised_model"]["version"], json!(3));
    assert!(d["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["code"] == "RL-IT-1"));

    // PII: raw card number / cvv never stored; unmapped field kept as source.*
    let event_id = d["event_id"].as_str().unwrap();
    let (s, detail) = h
        .call(
            "GET",
            &format!("/api/v1/projects/{pid}/events/{event_id}"),
            Some(&ta),
            None,
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{detail}");
    let dump = detail.to_string();
    assert!(!dump.contains("4111 1111 1111 1111") && !dump.contains("4111111111111111"));
    assert!(!dump.contains("\"cvv\""));
    assert_eq!(detail["source"]["alasan_retur"], json!("barang rusak"));
    assert_eq!(detail["event"]["card_bin"], json!("411111"));
    assert_eq!(detail["event"]["amount"], json!(1500000.0));
    assert_eq!(detail["features"]["is_new_device"], json!(1));
    assert_eq!(detail["features"]["graph_distance_to_fraud"], json!(2));
    // auto-registered source field
    let (_, cat) = h
        .call(
            "GET",
            &format!("/api/v1/projects/{pid}/field-catalog?q=alasan"),
            Some(&ta),
            None,
        )
        .await;
    assert!(
        cat["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["path"] == "source.alasan_retur"),
        "{cat}"
    );

    // dedupe: same record again → same event and decision, still one event
    let (s, d2) = h
        .call_h(
            "POST",
            &format!("/api/v1/ingest/{slug}"),
            Some(("x-api-key", key.clone())),
            Some(custom_record("CHK-1")),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(d2["event_id"], d["event_id"]);
    assert_eq!(
        h.count(
            &tenant,
            &format!("SELECT count(*) FROM core.events WHERE data_source_id = '{sid}'")
        )
        .await,
        1
    );
    // second decision on the same customer attaches to the open case
    let (_, d3) = h
        .call_h(
            "POST",
            &format!("/api/v1/ingest/{slug}"),
            Some(("x-api-key", key.clone())),
            Some(custom_record("CHK-2")),
        )
        .await;
    assert_eq!(d3["case_id"], d["case_id"]);

    // wrong key / wrong slug
    let (s, _) = h
        .call_h(
            "POST",
            &format!("/api/v1/ingest/{slug}"),
            Some(("x-api-key", "fdk_nope-nope-nope".into())),
            Some(custom_record("X")),
        )
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    // invalid record → 422 + dead letter
    let (s, v) = h
        .call_h(
            "POST",
            &format!("/api/v1/ingest/{slug}"),
            Some(("x-api-key", key.clone())),
            Some(json!({ "jenis": "PEMBELIAN" })),
        )
        .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    assert_eq!(
        h.count(
            &tenant,
            &format!("SELECT count(*) FROM core.ingest_errors WHERE data_source_id = '{sid}'")
        )
        .await,
        1
    );

    // batch in load_only mode: event ids, null decisions
    let (s, b) = h
        .call_h(
            "POST",
            &format!("/api/v1/ingest/{slug}/batch"),
            Some(("x-api-key", key.clone())),
            Some(json!({ "records": [custom_record("CHK-10"), json!({"bad": true}), custom_record("CHK-11")], "mode": "load_only" })),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["accepted"], json!(2));
    assert_eq!(b["rejected"], json!(1));
    assert_eq!(b["errors"][0]["index"], json!(1));
    assert!(b["decisions"][0]["event_id"].is_string());
    assert_eq!(b["decisions"][0]["decision"], Value::Null);
}

#[tokio::test]
async fn ml_down_is_degraded_and_rules_down_falls_back_to_review() {
    let h = harness_or_skip!();
    let (ta, tenant) = h.tenant(&tag()).await;
    let pid = h.project(&ta, "checkout").await;
    // graph ok, ML 500, rules 500
    Mock::given(method("POST"))
        .and(path_regex(r"^/v1/projects/[^/]+/links$"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "entity_ids": [], "new_links": 0, "similarity_links_created": 0 })),
        )
        .mount(&h.mocks)
        .await;
    Mock::given(method("POST"))
        .and(path_regex(r"^/v1/projects/[^/]+/metrics$"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "distance_to_fraud": null, "fraud_neighbors_1": 0,
            "fraud_neighbors_2": 0, "component_size": 1, "shared_entity_count": 0, "degree": 0 })),
        )
        .mount(&h.mocks)
        .await;
    Mock::given(path_regex(r"^/v1/projects/[^/]+/(supervised|unsupervised)/"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&h.mocks)
        .await;
    Mock::given(path_regex(r"^/v1/projects/[^/]+/evaluate$"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&h.mocks)
        .await;

    let ev = json!({ "external_id": "E-1", "event_type": "transaction", "occurred_at": "2026-09-23T10:00:00Z",
                     "customer": { "external_id": "C-9" }, "amount": 1000, "card_number": "4111111111111111" });
    let (s, d) = h
        .call(
            "POST",
            &format!("/api/v1/projects/{pid}/events"),
            Some(&ta),
            Some(ev),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{d}");
    let degraded: Vec<&str> = d["degraded"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    for e in ["supervised", "unsupervised", "rules"] {
        assert!(degraded.contains(&e), "{d}");
    }
    assert_eq!(d["decision"], json!("review"), "fallback decision");
    assert_eq!(d["engine_scores"]["graph"], json!(0.0));
    assert_eq!(
        h.count(
            &tenant,
            &format!("SELECT count(*) FROM core.events WHERE project_id = '{pid}' AND needs_rescore")
        )
        .await,
        1
    );
}

#[tokio::test]
async fn simulate_does_not_persist_and_customer_labels_reach_graph() {
    let h = harness_or_skip!();
    mount_happy_engines(&h.mocks, 10.0).await;
    Mock::given(method("PUT"))
        .and(path_regex(r"^/v1/projects/[^/]+/customers/[^/]+/label$"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&h.mocks)
        .await;
    let (ta, tenant) = h.tenant(&tag()).await;
    let pid = h.project(&ta, "checkout").await;
    let ev = json!({ "external_id": "S-1", "event_type": "login", "occurred_at": "2026-09-23T10:00:00Z",
                     "customer": { "external_id": "C-1" }, "login_success": false, "device_id": "d1" });
    let (s, d) = h
        .call(
            "POST",
            &format!("/api/v1/projects/{pid}/score/simulate"),
            Some(&ta),
            Some(json!({ "event": ev })),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{d}");
    assert_eq!(d["persisted"], json!(false));
    let q = format!("SELECT count(*) FROM core.events WHERE project_id = '{pid}'");
    assert_eq!(h.count(&tenant, &q).await, 0);

    // real event, then a customer label → graph-service notified
    let (_, d) = h
        .call(
            "POST",
            &format!("/api/v1/projects/{pid}/events"),
            Some(&ta),
            Some(ev),
        )
        .await;
    // supervised 0.70 + 2 hops from fraud accumulate under noisy-OR (final 73) → review
    assert_eq!(d["decision"], json!("review"), "{d}");
    let (_, cust) = h
        .call(
            "GET",
            &format!("/api/v1/projects/{pid}/customers?q=C-1"),
            Some(&ta),
            None,
        )
        .await;
    let cid = cust["items"][0]["id"].as_str().unwrap().to_string();
    let (s, l) = h
        .call(
            "POST",
            &format!("/api/v1/projects/{pid}/labels"),
            Some(&ta),
            Some(
                json!({ "subject_type": "customer", "subject_id": cid, "label": "fraud",
                         "fraud_type": "account_takeover", "source": "chargeback" }),
            ),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{l}");
    let (_, c) = h
        .call(
            "GET",
            &format!("/api/v1/projects/{pid}/customers/{cid}"),
            Some(&ta),
            None,
        )
        .await;
    assert_eq!(c["risk_label"], json!("fraud"));
    // audit trail has the label
    let (s, a) = h
        .call(
            "GET",
            &format!("/api/v1/projects/{pid}/audit?action=label."),
            Some(&ta),
            None,
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{a}");
    assert_eq!(a["total"], json!(1));
    // wiremock verifies `expect(1)` for the PUT on drop
}

#[tokio::test]
async fn internal_batch_and_catalog_accept_service_token() {
    let h = harness_or_skip!();
    mount_happy_engines(&h.mocks, 90.0).await;
    let (ta, tenant) = h.tenant(&tag()).await;
    let pid = h.project(&ta, "checkout").await;
    let (sid, _key, _slug) = webhook_source(&h, &ta, &pid).await;
    let req = |uri: String, body: Option<Value>| {
        let mut b = Request::builder()
            .method(if body.is_some() { "POST" } else { "GET" })
            .uri(uri)
            .header("authorization", format!("Bearer {INTERNAL}"))
            .header("x-tenant-id", tenant.clone())
            .header("x-project-id", pid.clone())
            .header("x-actor", "ingest-service:poller");
        if body.is_some() {
            b = b.header("content-type", "application/json");
        }
        b.body(
            body.map(|v| Body::from(v.to_string()))
                .unwrap_or_else(Body::empty),
        )
        .unwrap()
    };
    let resp = h
        .app
        .clone()
        .oneshot(req(
            format!("/v1/internal/projects/{pid}/sources/{sid}/batch"),
            Some(json!({ "records": [custom_record("J-1")], "mode": "score", "job_id": Uuid::new_v4() })),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v: Value = serde_json::from_slice(&resp.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(v["decisions"][0]["decision"], json!("decline"), "{v}");

    let resp = h
        .app
        .clone()
        .oneshot(req(format!("/v1/internal/projects/{pid}/field-catalog"), None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v: Value = serde_json::from_slice(&resp.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert!(v["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["path"] == "event.amount"));

    // wrong token → 401
    let resp = h
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/internal/projects/{pid}/field-catalog"))
                .header("authorization", "Bearer nope")
                .header("x-tenant-id", tenant.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
