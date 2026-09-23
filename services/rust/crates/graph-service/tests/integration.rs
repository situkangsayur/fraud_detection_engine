//! Integration tests against a real Postgres with the platform schema, roles and RLS.
//!
//! Setup rows are written as `migrator` (schema owner, bypasses RLS). The service runs as
//! `graph_service`, a non-owner role, so RLS is enforced exactly as in production.
//!
//! Run (from `services/rust`):
//! ```text
//! eval "$(crates/graph-service/scripts/test-db.sh)"
//! cargo test -p graph-service
//! crates/graph-service/scripts/test-db.sh --stop
//! ```
//! Without `GRAPH_TEST_MIGRATOR_URL` / `GRAPH_TEST_SERVICE_URL` the tests are skipped.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::Router;
use chrono::{TimeZone, Utc};
use graph_service::{router, AppState};
use platform::auth::{Claims, JwtKeys, ProjectRole, TenantRole};
use platform::config::Secret;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

const JWT: &str = "integration-test-jwt-secret-0123456789";
const INT: &str = "integration-internal-token";

struct Env {
    admin: PgPool,
    service: PgPool,
    app: Router,
}

async fn env() -> Option<Env> {
    let (Ok(m), Ok(s)) = (
        std::env::var("GRAPH_TEST_MIGRATOR_URL"),
        std::env::var("GRAPH_TEST_SERVICE_URL"),
    ) else {
        eprintln!("skipping: GRAPH_TEST_MIGRATOR_URL / GRAPH_TEST_SERVICE_URL not set");
        return None;
    };
    let admin = platform::db::connect_url(&m, 4).await.expect("migrator pool");
    let service = platform::db::connect_url(&s, 8).await.expect("service pool");
    let state = AppState::new(service.clone(), &Secret::new(JWT), &Secret::new(INT));
    Some(Env {
        admin,
        service,
        app: router(state),
    })
}

/// Creates a tenant with one project; returns `(tenant, project)`.
async fn tenant_with_project(admin: &PgPool) -> (Uuid, Uuid) {
    let (t, p) = (Uuid::new_v4(), Uuid::new_v4());
    let slug = format!("t-{}", &t.simple().to_string()[..12]);
    sqlx::query("INSERT INTO core.tenants (id, slug, name) VALUES ($1, $2, 'T')")
        .bind(t)
        .bind(&slug)
        .execute(admin)
        .await
        .unwrap();
    sqlx::query("INSERT INTO core.projects (id, tenant_id, slug, name, stage) VALUES ($1, $2, 'checkout', 'P', 'pre_payment')")
        .bind(p)
        .bind(t)
        .execute(admin)
        .await
        .unwrap();
    (t, p)
}

async fn call(
    app: &Router,
    method: Method,
    uri: &str,
    headers: &[(&str, String)],
    body: Option<Value>,
) -> (StatusCode, Value, String) {
    let mut b = Request::builder().method(method).uri(uri);
    for (k, v) in headers {
        b = b.header(*k, v);
    }
    let req = match body {
        Some(v) => b
            .header("content-type", "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => b.body(Body::empty()).unwrap(),
    };
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 10 << 20).await.unwrap();
    let text = String::from_utf8_lossy(&bytes).to_string();
    let json = serde_json::from_str(&text).unwrap_or(Value::Null);
    (status, json, text)
}

fn internal(t: Uuid, p: Uuid) -> Vec<(&'static str, String)> {
    vec![
        ("authorization", format!("Bearer {INT}")),
        ("x-tenant-id", t.to_string()),
        ("x-project-id", p.to_string()),
    ]
}

fn user(t: Uuid, prj: HashMap<Uuid, ProjectRole>) -> Vec<(&'static str, String)> {
    let claims = Claims::new(
        Uuid::new_v4(),
        Some(t),
        TenantRole::Member,
        false,
        prj,
        Duration::from_secs(300),
    );
    let token = JwtKeys::new(&Secret::new(JWT)).issue(&claims).unwrap();
    vec![("authorization", format!("Bearer {token}"))]
}

fn links_body(
    customer: Uuid,
    ext: &str,
    label: &str,
    extra_customer: Value,
    event: Value,
    minute: u32,
) -> Value {
    let mut c = json!({"id": customer, "external_id": ext, "risk_label": label});
    if let (Some(obj), Some(extra)) = (c.as_object_mut(), extra_customer.as_object()) {
        obj.extend(extra.clone());
    }
    let mut e = json!({
        "id": Uuid::new_v4(),
        "occurred_at": Utc.with_ymd_and_hms(2026, 9, 1, 10, minute, 0).unwrap(),
    });
    if let (Some(obj), Some(extra)) = (e.as_object_mut(), event.as_object()) {
        obj.extend(extra.clone());
    }
    json!({"customer": c, "event": e})
}

async fn post_links(app: &Router, t: Uuid, p: Uuid, body: Value) -> Value {
    let (s, v, text) = call(
        app,
        Method::POST,
        &format!("/v1/projects/{p}/links"),
        &internal(t, p),
        Some(body),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{text}");
    v
}

#[tokio::test]
async fn links_are_idempotent_and_create_similarity_edges() {
    let Some(env) = env().await else { return };
    let (t, p) = tenant_with_project(&env.admin).await;
    let (a, b, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());

    let body_a = links_body(
        a,
        "A",
        "unknown",
        json!({"email": "budi@gmail.com", "phone": "081234567890"}),
        json!({"device_id": "dev-1", "instrument_fingerprint": "fp1", "card_bin": "411111", "card_last4": "1111"}),
        0,
    );
    let r1 = post_links(&env.app, t, p, body_a.clone()).await;
    assert_eq!(r1["entity_ids"].as_array().unwrap().len(), 4);
    assert_eq!(r1["new_links"], 4);

    // Same event again: no new links, same entities, event_count unchanged.
    let r2 = post_links(&env.app, t, p, body_a).await;
    assert_eq!(r2["new_links"], 0);
    assert_eq!(r2["entity_ids"], r1["entity_ids"]);
    let (max_count,): (i32,) =
        sqlx::query_as("SELECT max(event_count) FROM graph.entity_links WHERE project_id = $1")
            .bind(p)
            .fetch_one(&env.admin)
            .await
            .unwrap();
    assert_eq!(max_count, 1);

    // B shares the card and has a phone one digit away → exact link + similarity edge.
    let r3 = post_links(
        &env.app,
        t,
        p,
        links_body(
            b,
            "B",
            "unknown",
            json!({"phone": "+62 812-3456-7891"}),
            json!({"instrument_fingerprint": "fp1"}),
            1,
        ),
    )
    .await;
    assert_eq!(r3["new_links"], 2);
    assert_eq!(r3["similarity_links_created"], 1);

    // C: same email local part on another domain → email similarity edge.
    let r4 = post_links(
        &env.app,
        t,
        p,
        links_body(
            c,
            "C",
            "unknown",
            json!({"email": "budi@yahoo.com"}),
            json!({}),
            2,
        ),
    )
    .await;
    assert_eq!(r4["similarity_links_created"], 1);

    let rows: Vec<(String, i32)> = sqlx::query_as(
        "SELECT kind, customer_count FROM graph.entities WHERE project_id = $1 ORDER BY kind, customer_count",
    )
    .bind(p)
    .fetch_all(&env.admin)
    .await
    .unwrap();
    assert!(rows.contains(&("card".into(), 2)), "{rows:?}");
    let methods: Vec<(String,)> =
        sqlx::query_as("SELECT method FROM graph.entity_similarity WHERE project_id = $1 ORDER BY method")
            .bind(p)
            .fetch_all(&env.admin)
            .await
            .unwrap();
    assert_eq!(
        methods,
        vec![("email_local".to_string(),), ("phone_edit1".to_string(),)]
    );
}

#[tokio::test]
async fn metrics_distance_labels_and_single_metric() {
    let Some(env) = env().await else { return };
    let (t, p) = tenant_with_project(&env.admin).await;
    let (a, b, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    // c —device— a —card— b(fraud)
    post_links(
        &env.app,
        t,
        p,
        links_body(
            a,
            "A",
            "unknown",
            json!({}),
            json!({"instrument_fingerprint": "fpX", "device_id": "devX"}),
            0,
        ),
    )
    .await;
    post_links(
        &env.app,
        t,
        p,
        links_body(
            b,
            "B",
            "fraud",
            json!({}),
            json!({"instrument_fingerprint": "fpX"}),
            1,
        ),
    )
    .await;
    post_links(
        &env.app,
        t,
        p,
        links_body(c, "C", "unknown", json!({}), json!({"device_id": "devX"}), 2),
    )
    .await;

    let (s, m, text) = call(
        &env.app,
        Method::POST,
        &format!("/v1/projects/{p}/metrics"),
        &internal(t, p),
        Some(json!({"customer_id": c})),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{text}");
    assert_eq!(m["distance_to_fraud"], 2);
    assert_eq!(m["fraud_neighbors_1"], 0);
    assert_eq!(m["fraud_neighbors_2"], 1);
    assert_eq!(m["degree"], 1);
    assert_eq!(m["component_size"], 3);
    assert_eq!(m["shared_entity_count"], 1);

    let (_, m_a, _) = call(
        &env.app,
        Method::POST,
        &format!("/v1/projects/{p}/metrics"),
        &internal(t, p),
        Some(json!({"customer_id": a})),
    )
    .await;
    assert_eq!(m_a["distance_to_fraud"], 1);
    assert_eq!(m_a["shared_with_fraud_kinds"], json!(["card"]));

    let metric = |kinds: Value| json!({"customer_id": c, "metric": "distance_to_fraud", "link_kinds": kinds, "include_similar": false, "max_depth": 3});
    let (_, v, _) = call(
        &env.app,
        Method::POST,
        &format!("/v1/projects/{p}/metric"),
        &internal(t, p),
        Some(metric(json!(["card", "device"]))),
    )
    .await;
    assert_eq!(v["value"], 2.0);
    let (_, v, _) = call(
        &env.app,
        Method::POST,
        &format!("/v1/projects/{p}/metric"),
        &internal(t, p),
        Some(metric(json!(["device"]))),
    )
    .await;
    assert_eq!(v["value"], Value::Null);

    // Label change is reflected immediately.
    let (s, _, _) = call(
        &env.app,
        Method::PUT,
        &format!("/v1/projects/{p}/customers/{b}/label"),
        &internal(t, p),
        Some(json!({"risk_label": "legit"})),
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, m, _) = call(
        &env.app,
        Method::POST,
        &format!("/v1/projects/{p}/metrics"),
        &internal(t, p),
        Some(json!({"customer_id": c})),
    )
    .await;
    assert_eq!(m["distance_to_fraud"], Value::Null);

    let (s, _, _) = call(
        &env.app,
        Method::PUT,
        &format!("/v1/projects/{p}/customers/{}/label", Uuid::new_v4()),
        &internal(t, p),
        Some(json!({"risk_label": "fraud"})),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _, _) = call(
        &env.app,
        Method::PUT,
        &format!("/v1/projects/{p}/customers/{b}/label"),
        &internal(t, p),
        Some(json!({"risk_label": "bogus"})),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn rls_isolates_tenants() {
    let Some(env) = env().await else { return };
    let (t1, p1) = tenant_with_project(&env.admin).await;
    let (t2, _p2) = tenant_with_project(&env.admin).await;
    let a = Uuid::new_v4();
    post_links(
        &env.app,
        t1,
        p1,
        links_body(a, "A", "unknown", json!({}), json!({"device_id": "dev-rls"}), 0),
    )
    .await;

    // Tenant 2 presenting tenant 1's project: the project is invisible under RLS → 404, nothing written.
    let (s, _, _) = call(
        &env.app,
        Method::POST,
        &format!("/v1/projects/{p1}/links"),
        &internal(t2, p1),
        Some(links_body(
            Uuid::new_v4(),
            "X",
            "unknown",
            json!({}),
            json!({"device_id": "dev-rls"}),
            1,
        )),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _, _) = call(
        &env.app,
        Method::POST,
        &format!("/v1/projects/{p1}/metrics"),
        &internal(t2, p1),
        Some(json!({"customer_id": a})),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // Direct proof at the SQL level with the service role.
    let count_as = |tenant: Uuid| {
        let pool = env.service.clone();
        async move {
            let mut tx = platform::db::TenantTx::begin(&pool, platform::TenantId(tenant))
                .await
                .unwrap();
            let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM graph.entities WHERE project_id = $1")
                .bind(p1)
                .fetch_one(&mut **tx)
                .await
                .unwrap();
            tx.commit().await.unwrap();
            n
        }
    };
    assert_eq!(count_as(t1).await, 1);
    assert_eq!(count_as(t2).await, 0);
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM graph.entities")
        .fetch_one(&env.service)
        .await
        .unwrap();
    assert_eq!(n, 0, "no tenant set → RLS fails closed");
}

#[tokio::test]
async fn user_endpoints_auth_and_payloads() {
    let Some(env) = env().await else { return };
    let (t, p) = tenant_with_project(&env.admin).await;
    let (a, b, f) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    post_links(
        &env.app,
        t,
        p,
        links_body(
            a,
            "CUST-A",
            "unknown",
            json!({"phone": "081111111111"}),
            json!({"device_id": "dev-ui"}),
            0,
        ),
    )
    .await;
    post_links(
        &env.app,
        t,
        p,
        links_body(
            b,
            "CUST-B",
            "unknown",
            json!({}),
            json!({"device_id": "dev-ui", "shipping_address": "Jl. Melati No. 5"}),
            1,
        ),
    )
    .await;
    post_links(
        &env.app,
        t,
        p,
        links_body(
            f,
            "CUST-F",
            "fraud",
            json!({}),
            json!({"shipping_address": "jalan melati nomor 5"}),
            2,
        ),
    )
    .await;

    let viewer = user(t, HashMap::from([(p, ProjectRole::Viewer)]));
    let outsider = user(t, HashMap::new());

    // No token / service-only endpoint with a user token / user without membership.
    let (s, _, _) = call(
        &env.app,
        Method::GET,
        &format!("/api/v1/projects/{p}/graph/stats"),
        &[],
        None,
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _, _) = call(
        &env.app,
        Method::POST,
        &format!("/v1/projects/{p}/metrics"),
        &viewer,
        Some(json!({"customer_id": a})),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _, _) = call(
        &env.app,
        Method::GET,
        &format!("/api/v1/projects/{p}/graph/stats"),
        &outsider,
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // Neighbourhood: a —device— b —address(normalised same)— f
    let (s, n, text) = call(
        &env.app,
        Method::GET,
        &format!("/api/v1/projects/{p}/graph/customers/{a}/neighborhood?depth=2"),
        &viewer,
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{text}");
    let ids: Vec<&str> = n["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["id"].as_str().unwrap())
        .collect();
    assert!(
        ids.contains(&a.to_string().as_str()) && ids.contains(&f.to_string().as_str()),
        "{ids:?}"
    );
    assert!(n["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|x| x["type"] == "entity" && x["kind"] == "device"));
    assert!(!n["edges"].as_array().unwrap().is_empty());

    let (s, px, _) = call(
        &env.app,
        Method::GET,
        &format!("/api/v1/projects/{p}/graph/customers/{a}/fraud-proximity?max_depth=3"),
        &viewer,
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(px["distance"], 2);
    assert_eq!(px["nearest_fraud_customer_id"], f.to_string());
    assert_eq!(px["fraud_within"], json!({"1": 0, "2": 1, "3": 1}));
    let path_types: Vec<&str> = px["path"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["type"].as_str().unwrap())
        .collect();
    assert_eq!(
        path_types,
        vec!["customer", "entity", "customer", "entity", "customer"]
    );

    let (_, comps, _) = call(
        &env.app,
        Method::GET,
        &format!("/api/v1/projects/{p}/graph/components?min_size=2&only_with_fraud=true"),
        &viewer,
        None,
    )
    .await;
    assert_eq!(comps[0]["size"], 3);
    assert_eq!(comps[0]["fraud_count"], 1);

    let (_, st, _) = call(
        &env.app,
        Method::GET,
        &format!("/api/v1/projects/{p}/graph/stats"),
        &viewer,
        None,
    )
    .await;
    assert_eq!(st["customers"], 3);
    assert_eq!(st["fraud_customers"], 1);

    let (_, found, _) = call(
        &env.app,
        Method::GET,
        &format!("/api/v1/projects/{p}/graph/search?q=cust-"),
        &viewer,
        None,
    )
    .await;
    assert_eq!(found["customers"].as_array().unwrap().len(), 3);
    let (_, found, _) = call(
        &env.app,
        Method::GET,
        &format!("/api/v1/projects/{p}/graph/search?q=0811-1111-1111"),
        &viewer,
        None,
    )
    .await;
    assert_eq!(found["entities"][0]["kind"], "phone");
    assert_eq!(found["entities"][0]["customer_ids"], json!([a]));

    // /api/v1 read endpoints also accept internal service callers (llm-service tools).
    let mut svc = internal(t, p);
    svc.push(("x-actor", Uuid::new_v4().to_string()));
    for path in [
        format!("/api/v1/projects/{p}/graph/components?min_size=2&only_with_fraud=false"),
        format!("/api/v1/projects/{p}/graph/stats"),
        format!("/api/v1/projects/{p}/graph/customers/{a}/neighborhood"),
        format!("/api/v1/projects/{p}/graph/customers/{a}/fraud-proximity"),
    ] {
        let (s, _, text) = call(&env.app, Method::GET, &path, &svc, None).await;
        assert_eq!(s, StatusCode::OK, "{path}: {text}");
    }
    // A service call whose X-Project-Id differs from the path is rejected.
    let (s, _, _) = call(
        &env.app,
        Method::GET,
        &format!("/api/v1/projects/{p}/graph/stats"),
        &internal(t, Uuid::new_v4()),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // Export (service only): NDJSON with source/target/weight.
    let (s, _, text) = call(
        &env.app,
        Method::GET,
        &format!("/v1/projects/{p}/export"),
        &internal(t, p),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let lines: Vec<Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines.len(), 2, "{text}");
    for l in &lines {
        assert!(l["source"].is_string() && l["target"].is_string() && l["weight"].as_f64().unwrap() > 0.0);
    }

    let (s, doc, _) = call(&env.app, Method::GET, "/openapi.json", &[], None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(doc["paths"]["/v1/projects/{pid}/links"].is_object());
}

/// Latency benchmark (ignored by default):
/// `cargo test -p graph-service --release --test integration -- --ignored --nocapture`
/// 3 000 customers sharing devices/cards/addresses at random (≈ 3 entities each), 5 % fraud.
#[tokio::test]
#[ignore]
async fn bench_metrics_latency() {
    let Some(env) = env().await else { return };
    let (t, p) = tenant_with_project(&env.admin).await;
    let n = 3000usize;
    let customers: Vec<Uuid> = (0..n).map(|_| Uuid::new_v4()).collect();
    // Deterministic pseudo-random sharing: ~1 000 devices, ~1 500 cards, ~1 200 addresses.
    let mut x: u64 = 42;
    let mut next = |m: u64| {
        x = x
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (x >> 33) % m
    };
    let start = std::time::Instant::now();
    for (i, c) in customers.iter().enumerate() {
        let label = if next(100) < 5 { "fraud" } else { "unknown" };
        let body = links_body(
            *c,
            &format!("B{i}"),
            label,
            json!({"phone": format!("0812{:08}", next(100_000_000))}),
            json!({"device_id": format!("dev-{}", next(1000)), "instrument_fingerprint": format!("fp-{}", next(1500)),
                   "shipping_address": format!("jalan mawar {} rt 1 rw 2", next(1200))}),
            u32::try_from(i % 60).unwrap(),
        );
        post_links(&env.app, t, p, body).await;
    }
    eprintln!(
        "ingest: {n} links calls in {:?} ({:?}/call)",
        start.elapsed(),
        start.elapsed() / n as u32
    );

    let mut lat = Vec::new();
    for c in customers.iter().take(300) {
        let s = std::time::Instant::now();
        let (st, _, _) = call(
            &env.app,
            Method::POST,
            &format!("/v1/projects/{p}/metrics"),
            &internal(t, p),
            Some(json!({"customer_id": c})),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        lat.push(s.elapsed());
    }
    lat.sort();
    eprintln!(
        "metrics (depth 3 + component≤1000): p50 {:?} p95 {:?} max {:?}",
        lat[lat.len() / 2],
        lat[lat.len() * 95 / 100],
        lat[lat.len() - 1]
    );
    let mut lat = Vec::new();
    for c in customers.iter().take(300) {
        let s = std::time::Instant::now();
        let body = json!({"customer_id": c, "metric": "distance_to_fraud", "link_kinds": ["device", "card", "address"], "include_similar": true, "max_depth": 3});
        call(
            &env.app,
            Method::POST,
            &format!("/v1/projects/{p}/metric"),
            &internal(t, p),
            Some(body),
        )
        .await;
        lat.push(s.elapsed());
    }
    lat.sort();
    eprintln!(
        "metric distance_to_fraud (early exit): p50 {:?} p95 {:?}",
        lat[lat.len() / 2],
        lat[lat.len() * 95 / 100]
    );
}
