"""End-to-end tests against a real Postgres with the platform migrations applied (RLS active).

Run with:
  TEST_DATABASE_URL=postgresql+psycopg://ml_service:<pw>@host:port/fraud           (service role, RLS applies)
  TEST_ADMIN_DATABASE_URL=postgresql+psycopg://migrator:<pw>@host:port/fraud      (schema owner, seeds data)
Skipped when those variables are absent.
"""

from __future__ import annotations

import json
import os
import uuid
from collections.abc import Iterator
from datetime import UTC, datetime, timedelta
from pathlib import Path
from typing import Any

import numpy as np
import pytest
from sqlalchemy import create_engine, text

from tests.conftest import bearer, internal_headers, make_jwt

pytestmark = pytest.mark.skipif(
    not (os.environ.get("TEST_DATABASE_URL") and os.environ.get("TEST_ADMIN_DATABASE_URL")),
    reason="TEST_DATABASE_URL / TEST_ADMIN_DATABASE_URL not set",
)

ANALYST = str(uuid.uuid4())
APPROVER = str(uuid.uuid4())


class InlineRunner:
    """Runs training synchronously so tests are deterministic."""

    def submit(self, model_id: str, kind: str, fn: Any, *args: Any) -> None:
        fn(*args)

    def running(self) -> set[str]:
        return set()


def _seed_project(
    conn: Any, tenant: str, project: str, n_events: int, labelled_ratio: float, seed: int
) -> None:
    rng = np.random.default_rng(seed)
    conn.execute(
        text("INSERT INTO core.tenants (id, slug, name) VALUES (:t, :s, :s) ON CONFLICT DO NOTHING"),
        {"t": tenant, "s": f"t-{tenant[:8]}"},
    )
    conn.execute(
        text(
            "INSERT INTO core.projects (id, tenant_id, slug, name, stage) "
            "VALUES (:p, :t, :s, 'Checkout', 'pre_payment')"
        ),
        {"p": project, "t": tenant, "s": f"p-{project[:8]}"},
    )
    source = str(uuid.uuid4())
    conn.execute(
        text(
            "INSERT INTO core.data_sources (id, tenant_id, project_id, slug, name, kind) "
            "VALUES (:id, :t, :p, 'canonical', 'Canonical', 'internal')"
        ),
        {"id": source, "t": tenant, "p": project},
    )
    customers = [str(uuid.uuid4()) for _ in range(40)]
    for i, c in enumerate(customers):
        conn.execute(
            text(
                "INSERT INTO core.customers (id, tenant_id, project_id, external_id, risk_label) "
                "VALUES (:id, :t, :p, :e, :r)"
            ),
            {"id": c, "t": tenant, "p": project, "e": f"C{i}", "r": "fraud" if i < 4 else "unknown"},
        )
    start = datetime.now(UTC) - timedelta(days=20)
    for i in range(n_events):
        fraud = bool(rng.random() < 0.15)
        event = str(uuid.uuid4())
        occurred = start + timedelta(minutes=int(i * 60))
        features = {
            "amount": float(rng.normal(8_000_000 if fraud else 500_000, 200_000)),
            "is_new_device": int(fraud or rng.random() < 0.05),
            "cust_cnt_1h": int(rng.poisson(6 if fraud else 1)),
            "hour_of_day": int(rng.integers(0, 6) if fraud else rng.integers(8, 22)),
            "event_type": "transaction",
            "channel": "web" if rng.random() < 0.5 else "mobile_app",
        }
        conn.execute(
            text(
                "INSERT INTO core.events (id, tenant_id, project_id, data_source_id, external_id, "
                "event_type, customer_id, occurred_at, amount, payload) VALUES "
                "(:id, :t, :p, :s, :e, 'transaction', :c, :o, :a, CAST(:payload AS jsonb))"
            ),
            {
                "id": event,
                "t": tenant,
                "p": project,
                "s": source,
                "e": f"E{i}",
                "c": customers[int(rng.integers(0, 40))],
                "o": occurred,
                "a": features["amount"],
                "payload": json.dumps({"order": {"items": int(rng.integers(1, 5))}}),
            },
        )
        conn.execute(
            text(
                "INSERT INTO core.event_features (event_id, tenant_id, project_id, feature_set_version, "
                "features) VALUES (:e, :t, :p, 1, CAST(:f AS jsonb))"
            ),
            {"e": event, "t": tenant, "p": project, "f": json.dumps(features)},
        )
        if rng.random() < labelled_ratio:
            conn.execute(
                text(
                    "INSERT INTO core.labels (tenant_id, project_id, subject_type, subject_id, label, "
                    "source) VALUES (:t, :p, 'event', :e, :l, 'dataset')"
                ),
                {"t": tenant, "p": project, "e": event, "l": "fraud" if fraud else "legit"},
            )


@pytest.fixture(scope="module")
def world() -> Iterator[dict[str, str]]:
    admin = create_engine(os.environ["TEST_ADMIN_DATABASE_URL"])
    ids = {
        "tenant": str(uuid.uuid4()),
        "project": str(uuid.uuid4()),
        "small": str(uuid.uuid4()),
        "other_tenant": str(uuid.uuid4()),
        "other_project": str(uuid.uuid4()),
    }
    with admin.begin() as conn:
        _seed_project(conn, ids["tenant"], ids["project"], 400, 0.85, seed=1)
        for user, email in ((ANALYST, "analyst"), (APPROVER, "approver")):
            conn.execute(
                text(
                    "INSERT INTO core.app_users (id, tenant_id, email, full_name, password_hash) "
                    "VALUES (:id, :t, :e, :n, 'x')"
                ),
                {"id": user, "t": ids["tenant"], "e": f"{email}-{user[:8]}@test.local", "n": email},
            )
        _seed_project(conn, ids["tenant"], ids["small"], 12, 1.0, seed=2)
        _seed_project(conn, ids["other_tenant"], ids["other_project"], 30, 1.0, seed=3)
    yield ids
    with admin.begin() as conn:
        for t in (ids["tenant"], ids["other_tenant"]):
            # core.approvals.requested_by/decided_by → app_users have no ON DELETE action, so they block the
            # tenant → app_users cascade; remove them first (schema gap reported to the migration owner).
            conn.execute(text("DELETE FROM core.approvals WHERE tenant_id = :t"), {"t": t})
            conn.execute(text("DELETE FROM core.tenants WHERE id = :t"), {"t": t})
    admin.dispose()


@pytest.fixture
def api(client: Any, monkeypatch: pytest.MonkeyPatch) -> Any:
    import ml_service.api.models as models_api

    monkeypatch.setattr(models_api, "get_runner", lambda: InlineRunner())
    return client


def _h(world: dict[str, str], user: str, role: str, project_key: str = "project") -> dict[str, str]:
    return bearer(make_jwt(sub=user, tid=world["tenant"], prj={world[project_key]: role}))


def _train_and_activate(api: Any, world: dict[str, str], kind: str, body: dict[str, Any]) -> dict[str, Any]:
    pid = world["project"]
    r = api.post(f"/api/v1/projects/{pid}/ml/{kind}/train", json=body, headers=_h(world, ANALYST, "analyst"))
    assert r.status_code == 202, r.text
    model_id = r.json()["model_id"]
    detail = api.get(
        f"/api/v1/projects/{pid}/ml/models/{model_id}", headers=_h(world, ANALYST, "viewer")
    ).json()
    assert detail["status"] == "ready", detail.get("error")
    r = api.post(
        f"/api/v1/projects/{pid}/ml/models/{model_id}/submit", json={}, headers=_h(world, ANALYST, "analyst")
    )
    assert r.json()["status"] == "pending_approval"
    r = api.post(
        f"/api/v1/projects/{pid}/ml/models/{model_id}/approve",
        json={"comment": "ok"},
        headers=_h(world, APPROVER, "approver"),
    )
    assert r.status_code == 200, r.text
    assert r.json()["status"] == "active"
    return r.json()


def test_algorithms_synced_to_db(api: Any) -> None:
    admin = create_engine(os.environ["TEST_ADMIN_DATABASE_URL"])
    with admin.connect() as conn:
        names = {
            r[0] for r in conn.execute(text("SELECT name FROM ml.algorithms WHERE status = 'available'"))
        }
    assert {"mlp_backprop", "isolation_forest", "hdbscan", "kmeans"} <= names


def test_supervised_lifecycle_with_four_eyes_and_hot_swap(api: Any, world: dict[str, str]) -> None:
    pid, tid = world["project"], world["tenant"]
    r = api.post(
        f"/api/v1/projects/{pid}/ml/supervised/train",
        json={"algorithm": "mlp_backprop", "params": {"epochs": 15, "hidden_layers": [16]}},
        headers=_h(world, ANALYST, "analyst"),
    )
    assert r.status_code == 202 and r.json()["status"] == "training"
    model_id = r.json()["model_id"]
    detail = api.get(
        f"/api/v1/projects/{pid}/ml/models/{model_id}", headers=_h(world, ANALYST, "viewer")
    ).json()
    assert detail["status"] == "ready", detail["error"]
    m = detail["metrics"]
    assert m["pr_auc"] is not None and m["pr_auc"] > 0.8
    assert {t["threshold"] for t in m["thresholds"]} == {0.3, 0.5, 0.7}
    assert m["feature_importance"] and m["class_balance"]["fraud"] > 0
    assert Path(detail["artifact_path"], "pipeline.joblib").exists()
    assert detail["algorithms"]["supervised"]["name"] == "mlp_backprop"

    submit = api.post(
        f"/api/v1/projects/{pid}/ml/models/{model_id}/submit", json={}, headers=_h(world, ANALYST, "analyst")
    )
    assert submit.json()["status"] == "pending_approval"
    # four-eyes: submitter cannot approve, even as approver
    same = api.post(
        f"/api/v1/projects/{pid}/ml/models/{model_id}/approve",
        json={},
        headers=_h(world, ANALYST, "approver"),
    )
    assert same.status_code == 403 and same.json()["type"] == "four_eyes_violation"
    # analyst role cannot approve
    assert (
        api.post(
            f"/api/v1/projects/{pid}/ml/models/{model_id}/approve",
            json={},
            headers=_h(world, APPROVER, "analyst"),
        ).status_code
        == 403
    )
    ok = api.post(
        f"/api/v1/projects/{pid}/ml/models/{model_id}/approve",
        json={},
        headers=_h(world, APPROVER, "approver"),
    )
    assert ok.status_code == 200 and ok.json()["status"] == "active"

    headers = internal_headers(tid, pid)
    fraudy = {
        "amount": 8_100_000,
        "is_new_device": 1,
        "cust_cnt_1h": 7,
        "hour_of_day": 2,
        "event_type": "transaction",
    }
    normal = {
        "amount": 450_000,
        "is_new_device": 0,
        "cust_cnt_1h": 1,
        "hour_of_day": 14,
        "event_type": "transaction",
    }
    p1 = api.post(f"/v1/projects/{pid}/supervised/predict", json={"features": fraudy}, headers=headers).json()
    p0 = api.post(f"/v1/projects/{pid}/supervised/predict", json={"features": normal}, headers=headers).json()
    assert p1["model_id"] == model_id and p1["fraud_probability"] > 0.8 > p0["fraud_probability"]

    # second model → approve archives the first and hot-swaps serving
    second = _train_and_activate(api, world, "supervised", {"algorithm": "logistic_regression"})
    first = api.get(
        f"/api/v1/projects/{pid}/ml/models/{model_id}", headers=_h(world, ANALYST, "viewer")
    ).json()
    assert first["status"] == "archived"
    p2 = api.post(f"/v1/projects/{pid}/supervised/predict", json={"features": fraudy}, headers=headers).json()
    assert p2["model_id"] == second["id"] and p2["algorithm"] == "logistic_regression"

    admin = create_engine(os.environ["TEST_ADMIN_DATABASE_URL"])
    with admin.connect() as conn:
        approvals = conn.execute(
            text(
                "SELECT requested_by::text, decided_by::text, decision FROM core.approvals "
                "WHERE subject_id = :m"
            ),
            {"m": model_id},
        ).all()
        actions = {
            r[0]
            for r in conn.execute(
                text("SELECT action FROM core.audit_log WHERE subject_id = :m"), {"m": model_id}
            )
        }
    assert approvals == [(ANALYST, APPROVER, "approved")]
    assert {"ml.model.train", "ml.model.submit", "ml.model.approve"} <= actions


def test_reject_returns_model_to_ready(api: Any, world: dict[str, str]) -> None:
    pid = world["project"]
    r = api.post(
        f"/api/v1/projects/{pid}/ml/supervised/train",
        json={"algorithm": "random_forest", "params": {"n_estimators": 20}},
        headers=_h(world, ANALYST, "analyst"),
    )
    mid = r.json()["model_id"]
    api.post(f"/api/v1/projects/{pid}/ml/models/{mid}/submit", json={}, headers=_h(world, ANALYST, "analyst"))
    rej = api.post(
        f"/api/v1/projects/{pid}/ml/models/{mid}/reject",
        json={"comment": "overfits"},
        headers=_h(world, APPROVER, "approver"),
    )
    assert rej.json()["status"] == "ready"
    again = api.post(
        f"/api/v1/projects/{pid}/ml/models/{mid}/approve", json={}, headers=_h(world, APPROVER, "approver")
    )
    assert again.status_code == 409


def test_training_with_too_few_labels_fails_cleanly(api: Any, world: dict[str, str]) -> None:
    pid = world["small"]
    r = api.post(
        f"/api/v1/projects/{pid}/ml/supervised/train", json={}, headers=_h(world, ANALYST, "analyst", "small")
    )
    detail = api.get(
        f"/api/v1/projects/{pid}/ml/models/{r.json()['model_id']}",
        headers=_h(world, ANALYST, "viewer", "small"),
    ).json()
    assert detail["status"] == "failed" and "not enough labelled data" in detail["error"]


def test_label_maturity_adds_implicit_legit_rows(api: Any, world: dict[str, str]) -> None:
    """Unlabelled events older than label_maturity_days count as legit; 0 disables it."""
    pid = world["project"]

    def train(body: dict[str, Any]) -> dict[str, Any]:
        r = api.post(
            f"/api/v1/projects/{pid}/ml/supervised/train",
            json={"algorithm": "logistic_regression", **body},
            headers=_h(world, ANALYST, "analyst"),
        )
        assert r.status_code == 202, r.text
        detail: dict[str, Any] = api.get(
            f"/api/v1/projects/{pid}/ml/models/{r.json()['model_id']}", headers=_h(world, ANALYST, "viewer")
        ).json()
        assert detail["status"] == "ready", detail.get("error")
        return dict(detail["metrics"]["class_balance"])

    matured = train({"label_maturity_days": 14})
    labelled_only = train({"label_maturity_days": 0})
    assert matured["implicit_legit"] > 0 and matured["label_maturity_days"] == 14
    assert labelled_only["implicit_legit"] == 0 and labelled_only["label_maturity_days"] is None
    assert matured["legit"] == labelled_only["legit"] + matured["implicit_legit"]
    assert matured["fraud"] == labelled_only["fraud"]


def test_invalid_algorithm_rejected_before_training(api: Any, world: dict[str, str]) -> None:
    pid = world["project"]
    r = api.post(
        f"/api/v1/projects/{pid}/ml/supervised/train",
        json={"algorithm": "kmeans"},
        headers=_h(world, ANALYST, "analyst"),
    )
    assert r.status_code == 422 and r.json()["type"] == "invalid_ml_config"
    r = api.post(
        f"/api/v1/projects/{pid}/ml/supervised/train",
        json={"algorithm": "mlp_backprop", "params": {"dropout": 3}},
        headers=_h(world, ANALYST, "analyst"),
    )
    assert r.status_code == 422


@pytest.mark.parametrize(
    ("anomaly", "clustering"), [("isolation_forest", "kmeans"), ("autoencoder", "hdbscan")]
)
def test_unsupervised_lifecycle(api: Any, world: dict[str, str], anomaly: str, clustering: str) -> None:
    pid, tid = world["project"], world["tenant"]
    params: dict[str, Any] = {
        "anomaly_algorithm": anomaly,
        "clustering_algorithm": clustering,
        "since_days": 60,
    }
    if anomaly == "autoencoder":
        params["anomaly_params"] = {"epochs": 5}
    if clustering == "kmeans":
        params["clustering_params"] = {"n_clusters": 3}
    model = _train_and_activate(api, world, "unsupervised", params)
    assert model["metrics"]["n_rows"] == 400
    assert model["algorithms"]["clustering"]["name"] == clustering

    viewer = _h(world, ANALYST, "viewer")
    clusters = api.get(f"/api/v1/projects/{pid}/ml/unsupervised/clusters", headers=viewer).json()
    assert clusters["model_id"] == model["id"] and clusters["items"]
    first = clusters["items"][0]
    assert {"size", "fraud_rate", "profile", "top_features"} <= set(first)
    assert "amount" in first["profile"]

    proj = api.get(f"/api/v1/projects/{pid}/ml/unsupervised/projection?limit=50", headers=viewer).json()
    assert len(proj["items"]) == 50 and {"x", "y", "cluster_id", "anomaly_score"} <= set(proj["items"][0])
    top = api.get(
        f"/api/v1/projects/{pid}/ml/unsupervised/anomalies?limit=10&min_score=0.5", headers=viewer
    ).json()
    assert top["items"] and top["items"][0]["anomaly_score"] >= top["items"][-1]["anomaly_score"]

    patched = api.patch(
        f"/api/v1/projects/{pid}/ml/unsupervised/clusters/{model['id']}/{first['cluster_id']}",
        json={"label": "promo farm", "notes": "shared devices"},
        headers=_h(world, ANALYST, "analyst"),
    )
    assert patched.json()["label"] == "promo farm"

    score = api.post(
        f"/v1/projects/{pid}/unsupervised/score",
        headers=internal_headers(tid, pid),
        json={"features": {"amount": 99_000_000, "is_new_device": 1, "cust_cnt_1h": 30}},
    ).json()
    assert score["model_id"] == model["id"] and score["anomaly_score"] > 0.95
    assert isinstance(score["cluster_id"], int)


def test_graph_communities_recompute(
    api: Any, world: dict[str, str], monkeypatch: pytest.MonkeyPatch
) -> None:
    import ml_service.graph.communities as comm

    pid, tid = world["project"], world["tenant"]
    admin = create_engine(os.environ["TEST_ADMIN_DATABASE_URL"])
    with admin.connect() as conn:
        rows = conn.execute(
            text(
                "SELECT id::text, risk_label FROM core.customers WHERE project_id = :p ORDER BY external_id"
            ),
            {"p": pid},
        ).all()
    fraud = [r[0] for r in rows if r[1] == "fraud"]
    legit = [r[0] for r in rows if r[1] != "fraud"][:6]
    edges = [
        (fraud[0], fraud[1]),
        (fraud[1], fraud[2]),
        (fraud[0], fraud[2]),
        (legit[0], legit[1]),
        (legit[1], legit[2]),
        (legit[0], legit[2]),
    ]
    lines = [json.dumps({"source": a, "target": b, "weight": 1}) for a, b in edges]
    monkeypatch.setattr(comm, "fetch_export", lambda t, p: lines)
    r = api.post(
        f"/api/v1/projects/{pid}/ml/graph-communities/recompute", headers=_h(world, ANALYST, "analyst")
    )
    assert r.status_code == 200 and r.json()["communities"] == 2
    listed = api.get(
        f"/api/v1/projects/{pid}/ml/graph-communities?min_size=2", headers=_h(world, ANALYST, "viewer")
    ).json()["items"]
    assert listed[0]["fraud_count"] == 3 and listed[0]["fraud_rate"] == 1.0
    assert set(listed[0]["customer_ids"]) == set(fraud[:3])
    # internal token also accepted
    assert (
        api.get(
            f"/api/v1/projects/{pid}/ml/graph-communities", headers=internal_headers(tid, pid)
        ).status_code
        == 200
    )


def test_tenant_isolation_and_tenant_admin(api: Any, world: dict[str, str]) -> None:
    pid = world["project"]
    # a token of another tenant that (maliciously) claims the project sees nothing through RLS
    foreign = bearer(make_jwt(tid=world["other_tenant"], prj={pid: "project_admin"}))
    listed = api.get(f"/api/v1/projects/{pid}/ml/models", headers=foreign).json()
    assert listed["total"] == 0
    # tenant admin without explicit membership passes for own-tenant projects only
    tadmin = bearer(make_jwt(tid=world["tenant"], trole="tenant_admin"))
    assert api.get(f"/api/v1/projects/{pid}/ml/models", headers=tadmin).json()["total"] > 0
    assert api.get(f"/api/v1/projects/{world['other_project']}/ml/models", headers=tadmin).status_code == 403


def test_interrupted_training_marked_failed(api: Any, world: dict[str, str]) -> None:
    pid, tid = world["project"], world["tenant"]
    admin = create_engine(os.environ["TEST_ADMIN_DATABASE_URL"])
    stale = str(uuid.uuid4())
    with admin.begin() as conn:
        conn.execute(
            text(
                "INSERT INTO ml.models (id, tenant_id, project_id, kind, version, algorithms, "
                "feature_set_version, status, training_started_at) VALUES (:id, :t, :p, 'supervised', "
                "999, '{}', 1, 'training', now() - interval '1 day')"
            ),
            {"id": stale, "t": tid, "p": pid},
        )
    detail = api.get(f"/api/v1/projects/{pid}/ml/models/{stale}", headers=_h(world, ANALYST, "viewer")).json()
    assert detail["status"] == "failed" and "restart" in detail["error"]


def test_no_active_model_returns_404(api: Any, world: dict[str, str]) -> None:
    pid, tid = world["small"], world["tenant"]
    r = api.post(
        f"/v1/projects/{pid}/supervised/predict", json={"features": {}}, headers=internal_headers(tid, pid)
    )
    assert r.status_code == 404 and r.json()["type"] == "no_active_model"
