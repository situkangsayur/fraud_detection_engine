"""API tests without a database (auth, validation, inference via an injected in-memory model)."""

from __future__ import annotations

import uuid
from pathlib import Path
from typing import Any

import numpy as np
import pytest

from ml_service.features.pipeline import FeaturePipeline
from ml_service.plugins.builtin.anomaly import IsolationForestPlugin
from ml_service.plugins.builtin.clustering import KMeansPlugin
from ml_service.plugins.builtin.supervised import LogisticRegressionPlugin
from ml_service.serving.cache import get_cache
from ml_service.serving.loaded import LoadedSupervised, LoadedUnsupervised
from ml_service.training.unsupervised import ClusterAssigner
from tests.conftest import bearer, internal_headers, make_jwt

TENANT = str(uuid.uuid4())
PROJECT = str(uuid.uuid4())


def _training_rows(n: int = 200) -> tuple[list[dict[str, Any]], np.ndarray]:
    rng = np.random.default_rng(0)
    y = (rng.random(n) < 0.2).astype(int)
    rows = [
        {
            "amount": float(rng.normal(1000 + 5000 * yi, 200)),
            "is_new_device": int(yi),
            "event_type": "transaction",
        }
        for yi in y
    ]
    return rows, y


@pytest.fixture
def injected_models() -> None:
    rows, y = _training_rows()
    pipe = FeaturePipeline.from_config({"include": ["amount", "is_new_device", "event_type"]})
    X = pipe.fit_transform(rows)
    sup = LogisticRegressionPlugin()
    sup.fit(X, y, X, y, pipe.feature_names)
    anomaly = IsolationForestPlugin({"n_estimators": 20})
    anomaly.fit(X, pipe.feature_names)
    km = KMeansPlugin({"n_clusters": 2})
    km.fit(X, pipe.feature_names)
    cache = get_cache()
    cache._put(
        (PROJECT, "supervised"), LoadedSupervised(str(uuid.uuid4()), 3, "logistic_regression", pipe, sup)
    )
    cache._put(
        (PROJECT, "unsupervised"),
        LoadedUnsupervised(
            str(uuid.uuid4()), 1, pipe, anomaly, km, ClusterAssigner.fit(X, km.labels()), {0: 0.05, 1: 0.6}
        ),
    )
    yield
    cache.invalidate()


def test_health_live(client: Any) -> None:
    assert client.get("/health/live").json() == {"status": "ok"}
    assert "x-request-id" in client.get("/health/live").headers
    assert client.get("/metrics").status_code == 200


def test_algorithms_require_auth_and_list_catalogue(client: Any) -> None:
    r = client.get("/api/v1/ml/algorithms")
    assert r.status_code == 401 and r.headers["content-type"].startswith("application/problem+json")
    r = client.get("/api/v1/ml/algorithms", headers=bearer(make_jwt(tid=TENANT)))
    assert r.status_code == 200
    names = {a["name"] for a in r.json()}
    assert {"mlp_backprop", "isolation_forest", "hdbscan"} <= names
    mlp = next(a for a in r.json() if a["name"] == "mlp_backprop")
    assert mlp["defaults"]["hidden_layers"] == [64, 32] and mlp["param_schema"]["type"] == "object"
    only = client.get("/api/v1/ml/algorithms?kind=clustering", headers=bearer(make_jwt(tid=TENANT))).json()
    assert {a["kind"] for a in only} == {"clustering"}


def test_expired_and_forged_tokens_rejected(client: Any) -> None:
    assert client.get("/api/v1/ml/algorithms", headers=bearer(make_jwt(exp_delta=-10))).status_code == 401
    import jwt as pyjwt

    forged = pyjwt.encode({"sub": "x", "exp": 9999999999}, "other-secret", algorithm="HS256")
    assert client.get("/api/v1/ml/algorithms", headers=bearer(forged)).status_code == 401


def test_reload_requires_platform_admin(client: Any) -> None:
    r = client.post(
        "/api/v1/ml/algorithms/reload", headers=bearer(make_jwt(tid=TENANT, trole="tenant_admin"))
    )
    assert r.status_code == 403


def test_validate_config_internal_only(client: Any) -> None:
    body = {"ml_config": {"supervised": {"algorithm": "random_forest", "params": {"n_estimators": 0}}}}
    assert (
        client.post(
            "/v1/algorithms/validate-config", json=body, headers=bearer(make_jwt(tid=TENANT))
        ).status_code
        == 403
    )
    r = client.post("/v1/algorithms/validate-config", json=body, headers=internal_headers())
    assert r.status_code == 200
    out = r.json()
    assert out["valid"] is False and out["errors"][0]["path"] == "supervised.algorithm.params"
    ok = client.post("/v1/algorithms/validate-config", json={"ml_config": {}}, headers=internal_headers())
    assert ok.json() == {"valid": True, "errors": []}


def test_project_routes_enforce_membership(client: Any) -> None:
    url = f"/api/v1/projects/{PROJECT}/ml/unsupervised/clusters"
    other = str(uuid.uuid4())
    r = client.get(url, headers=bearer(make_jwt(tid=TENANT, prj={other: "project_admin"})))
    assert r.status_code == 403
    viewer = make_jwt(tid=TENANT, prj={PROJECT: "viewer"})
    r = client.post(f"/api/v1/projects/{PROJECT}/ml/supervised/train", json={}, headers=bearer(viewer))
    assert r.status_code == 403 and "analyst" in r.json()["detail"]


def test_internal_inference_requires_internal_token_and_tenant(client: Any, injected_models: None) -> None:
    url = f"/v1/projects/{PROJECT}/supervised/predict"
    body = {"features": {"amount": 6000, "is_new_device": 1, "event_type": "transaction"}}
    assert (
        client.post(
            url, json=body, headers=bearer(make_jwt(tid=TENANT, prj={PROJECT: "project_admin"}))
        ).status_code
        == 403
    )
    assert client.post(url, json=body, headers=internal_headers()).status_code == 400
    assert client.post(url, json=body, headers=internal_headers(TENANT, str(uuid.uuid4()))).status_code == 403


def test_predict_and_score(client: Any, injected_models: None) -> None:
    headers = internal_headers(TENANT, PROJECT)
    event_id = str(uuid.uuid4())
    high = client.post(
        f"/v1/projects/{PROJECT}/supervised/predict",
        headers=headers,
        json={
            "event_id": event_id,
            "features": {"amount": 6000, "is_new_device": 1, "event_type": "transaction"},
        },
    )
    low = client.post(
        f"/v1/projects/{PROJECT}/supervised/predict",
        headers=headers,
        json={"features": {"amount": 900, "is_new_device": 0}, "explain": False},
    )
    assert high.status_code == 200, high.text
    hb = high.json()
    assert (
        hb["event_id"] == event_id and hb["model_version"] == 3 and hb["algorithm"] == "logistic_regression"
    )
    assert hb["fraud_probability"] > low.json()["fraud_probability"]
    assert hb["top_features"] and {"name", "contribution"} <= set(hb["top_features"][0])
    assert low.json()["top_features"] == []

    batch = client.post(
        f"/v1/projects/{PROJECT}/supervised/predict/batch",
        headers=headers,
        json={"items": [{"features": {"amount": 6000}}, {"features": {"amount": 800}}]},
    )
    assert batch.status_code == 200 and len(batch.json()["results"]) == 2

    s = client.post(
        f"/v1/projects/{PROJECT}/unsupervised/score",
        headers=headers,
        json={"features": {"amount": 1_000_000, "is_new_device": 1}},
    )
    assert s.status_code == 200, s.text
    sb = s.json()
    assert 0.0 <= sb["anomaly_score"] <= 1.0 and sb["anomaly_score"] > 0.9
    assert sb["cluster_id"] in (0, 1) and sb["cluster_fraud_rate"] in (0.05, 0.6)


def test_predict_single_row_latency(client: Any, injected_models: None) -> None:
    import time

    headers = internal_headers(TENANT, PROJECT)
    body = {"features": {"amount": 6000, "is_new_device": 1, "event_type": "transaction"}}
    client.post(f"/v1/projects/{PROJECT}/supervised/predict", headers=headers, json=body)  # warm
    started = time.perf_counter()
    for _ in range(20):
        client.post(f"/v1/projects/{PROJECT}/supervised/predict", headers=headers, json=body)
    assert (time.perf_counter() - started) / 20 < 0.02


def test_validation_errors_are_problem_json(client: Any, injected_models: None) -> None:
    r = client.post(
        f"/v1/projects/{PROJECT}/supervised/predict",
        headers=internal_headers(TENANT, PROJECT),
        json={"features": "not-an-object"},
    )
    assert r.status_code == 422
    body = r.json()
    assert body["type"] == "validation_error" and body["errors"][0]["field"] == "features"


def test_models_are_loadable_from_saved_artifacts(tmp_path: Path) -> None:
    """Artifact layout produced by training is what LoadedSupervised.load expects."""
    import json

    rows, y = _training_rows()
    pipe = FeaturePipeline.from_config({"include": ["amount"]})
    X = pipe.fit_transform(rows)
    plugin = LogisticRegressionPlugin()
    plugin.fit(X, y, X, y, pipe.feature_names)
    pipe.save(tmp_path / "pipeline.joblib")
    plugin.save(tmp_path / "supervised")
    (tmp_path / "meta.json").write_text(json.dumps({"algorithm": {"name": "logistic_regression"}}))
    loaded = LoadedSupervised.load("m", 1, tmp_path)
    p, _ = loaded.predict({"amount": 7000}, None, explain=True)
    assert 0 <= p <= 1
