from __future__ import annotations

import os
import tempfile
import time
import uuid
from collections.abc import Iterator
from pathlib import Path
from typing import Any

import jwt
import pytest

_TMP = Path(tempfile.mkdtemp(prefix="ml-service-tests-"))
os.environ.setdefault("JWT_SECRET", "test-jwt-secret")
os.environ.setdefault("INTERNAL_API_TOKEN", "test-internal-token")
os.environ["MODEL_DIR"] = str(_TMP / "models")
os.environ.setdefault("PLUGIN_DIR", str(_TMP / "plugins-empty"))
if os.environ.get("TEST_DATABASE_URL"):
    os.environ["DATABASE_URL"] = os.environ["TEST_DATABASE_URL"]

JWT_SECRET = os.environ["JWT_SECRET"]
INTERNAL_TOKEN = os.environ["INTERNAL_API_TOKEN"]
REPO_ROOT = Path(__file__).resolve().parents[4]


def make_jwt(
    *,
    sub: str | None = None,
    tid: str | None = None,
    prj: dict[str, str] | None = None,
    trole: str = "member",
    padmin: bool = False,
    exp_delta: int = 3600,
) -> str:
    now = int(time.time())
    claims: dict[str, Any] = {
        "sub": sub or str(uuid.uuid4()),
        "tid": tid,
        "trole": trole,
        "padmin": padmin,
        "prj": prj or {},
        "iat": now,
        "exp": now + exp_delta,
        "jti": str(uuid.uuid4()),
    }
    return jwt.encode(claims, JWT_SECRET, algorithm="HS256")


def bearer(token: str) -> dict[str, str]:
    return {"Authorization": f"Bearer {token}"}


def internal_headers(
    tenant_id: str | None = None, project_id: str | None = None, actor: str | None = None
) -> dict[str, str]:
    headers = {"Authorization": f"Bearer {INTERNAL_TOKEN}"}
    if tenant_id:
        headers["X-Tenant-Id"] = tenant_id
    if project_id:
        headers["X-Project-Id"] = project_id
    if actor:
        headers["X-Actor"] = actor
    return headers


@pytest.fixture
def client() -> Iterator[Any]:
    from fastapi.testclient import TestClient

    from ml_service.main import create_app

    with TestClient(create_app()) as c:
        yield c
