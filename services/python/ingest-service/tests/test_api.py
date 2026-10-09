from __future__ import annotations

import time
import uuid
from pathlib import Path
from typing import Any

import pytest
from fastapi.testclient import TestClient

from app.main import create_app
from tests.conftest import PROJECT, SOURCE, TENANT, make_jwt

BASE = f"/api/v1/projects/{PROJECT}/data-sources/{SOURCE}"


@pytest.fixture
def client(fake_db: dict[str, Any], tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> TestClient:
    from app.config import get_settings

    monkeypatch.setattr(get_settings(), "upload_dir", tmp_path)
    app = create_app(start_background=False)
    with TestClient(app) as c:
        yield c


def test_health(client: TestClient) -> None:
    assert client.get("/health/live").json() == {"status": "ok"}


def test_auth_required(client: TestClient) -> None:
    r = client.post(f"{BASE}/inspect", json={"records": [{"a": 1}]})
    assert r.status_code == 401
    assert r.headers["content-type"].startswith("application/problem+json")
    bad = make_jwt(secret="wrong-secret-0123456789abcdefghij")
    assert (
        client.post(
            f"{BASE}/inspect", json={"records": [{"a": 1}]}, headers={"Authorization": f"Bearer {bad}"}
        ).status_code
        == 401
    )


def test_viewer_cannot_inspect_but_can_list(client: TestClient, monkeypatch: pytest.MonkeyPatch) -> None:
    from app import repo

    monkeypatch.setattr(repo, "list_jobs", lambda c, p, s, pg, ps: ([], 0))
    h = {"Authorization": f"Bearer {make_jwt('viewer')}"}
    assert client.post(f"{BASE}/inspect", json={"records": [{"a": 1}]}, headers=h).status_code == 403
    assert client.get(f"{BASE}/jobs", headers=h).json()["total"] == 0


def test_other_project_forbidden(client: TestClient) -> None:
    h = {"Authorization": f"Bearer {make_jwt('analyst')}"}
    other = f"/api/v1/projects/{uuid.uuid4()}/data-sources/{SOURCE}/inspect"
    assert client.post(other, json={"records": [{"a": 1}]}, headers=h).status_code == 403


def test_tenant_admin_passes_project_check(client: TestClient, fake_db: dict[str, Any]) -> None:
    h = {"Authorization": f"Bearer {make_jwt(None, trole='tenant_admin')}"}
    r = client.post(
        f"{BASE}/inspect",
        json={"records": [{"order_id": "1", "created_at": "2026-09-01T00:00:00Z"}]},
        headers=h,
    )
    assert r.status_code == 200


def test_internal_token_requires_tenant_header(client: TestClient) -> None:
    h = {"Authorization": "Bearer test-internal-token"}
    assert client.post(f"{BASE}/inspect", json={"records": [{"a": 1}]}, headers=h).status_code == 400
    h["X-Tenant-Id"] = str(TENANT)
    assert client.post(f"{BASE}/inspect", json={"records": [{"a": 1}]}, headers=h).status_code == 200


def test_inspect_file_then_job(
    client: TestClient, fake_db: dict[str, Any], fixtures: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    h = {"Authorization": f"Bearer {make_jwt('analyst')}"}
    with (fixtures / "transaksi_id.csv").open("rb") as fh:
        r = client.post(f"{BASE}/inspect", files={"file": ("transaksi_id.csv", fh, "text/csv")}, headers=h)
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["file_format"] == "csv" and body["upload_id"]
    assert body["suggested_mapping"]["event"]["amount"]["from"] == "nominal"
    assert len(body["preview"]) == 5
    assert fake_db["schema"]["sampled_rows"] == 5
    submitted: list[Any] = []
    monkeypatch.setattr(client.app.state.runner, "submit", lambda *a: submitted.append(a))
    r = client.post(f"{BASE}/jobs", json={"upload_id": body["upload_id"], "mode": "load_only"}, headers=h)
    assert r.status_code == 202, r.text
    assert submitted and submitted[0][0]["mode"] == "load_only"


def test_job_needs_active_mapping(client: TestClient, fake_db: dict[str, Any]) -> None:
    fake_db["mapping"] = None
    h = {"Authorization": f"Bearer {make_jwt('analyst')}"}
    r = client.post(f"{BASE}/jobs", json={"upload_id": "x"}, headers=h)
    assert r.status_code == 409


def test_upload_size_limit(client: TestClient, monkeypatch: pytest.MonkeyPatch) -> None:
    from app.config import get_settings

    monkeypatch.setattr(get_settings(), "max_upload_mb", 0)
    h = {"Authorization": f"Bearer {make_jwt('analyst')}"}
    r = client.post(f"{BASE}/inspect", files={"file": ("a.csv", b"a,b\n1,2\n", "text/csv")}, headers=h)
    assert r.status_code == 413


def test_expired_token(client: TestClient) -> None:
    import jwt as pyjwt

    tok = pyjwt.encode(
        {"sub": "x", "tid": str(TENANT), "exp": int(time.time()) - 5},
        "test-jwt-secret-0123456789abcdef",
        algorithm="HS256",
    )
    assert client.get(f"{BASE}/jobs", headers={"Authorization": f"Bearer {tok}"}).status_code == 401
