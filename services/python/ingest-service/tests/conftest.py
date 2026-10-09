from __future__ import annotations

import os
import time
import uuid
from contextlib import contextmanager
from pathlib import Path
from typing import Any

import jwt
import pytest

os.environ.setdefault("INTERNAL_API_TOKEN", "test-internal-token")
os.environ.setdefault("JWT_SECRET", "test-jwt-secret-0123456789abcdef")
os.environ.setdefault("CORE_API_URL", "http://core.test")
os.environ.setdefault("LLM_SERVICE_URL", "http://llm.test")

FIXTURES = Path(__file__).parent / "fixtures"
TENANT = uuid.UUID("11111111-1111-1111-1111-111111111111")
PROJECT = uuid.UUID("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")
SOURCE = uuid.UUID("cccccccc-cccc-cccc-cccc-cccccccccccc")


def make_jwt(
    role: str | None = "analyst", tenant: uuid.UUID = TENANT, trole: str = "member", secret: str | None = None
) -> str:
    claims: dict[str, Any] = {
        "sub": str(uuid.uuid4()),
        "tid": str(tenant),
        "trole": trole,
        "padmin": False,
        "prj": {str(PROJECT): role} if role else {},
        "exp": int(time.time()) + 600,
        "iat": int(time.time()),
    }
    return jwt.encode(claims, secret or os.environ["JWT_SECRET"], algorithm="HS256")


@pytest.fixture
def fixtures() -> Path:
    return FIXTURES


class FakeConn:
    """Stands in for a tenant-scoped connection; repo functions are monkeypatched per test."""


@contextmanager
def fake_session(tenant_id: uuid.UUID, engine: Any = None) -> Any:
    yield FakeConn()


@pytest.fixture
def fake_db(monkeypatch: pytest.MonkeyPatch) -> dict[str, Any]:
    """In-memory replacement for app.repo + tenant_session used by routes and the job runner."""
    import app.api.routes as routes
    import app.jobs.runner as runner
    from app import repo

    state: dict[str, Any] = {
        "source": {
            "id": SOURCE,
            "tenant_id": TENANT,
            "project_id": PROJECT,
            "slug": "src",
            "name": "src",
            "kind": "file",
            "default_event_type": "transaction",
            "mode": "score",
            "connection": {},
            "cursor_state": {},
            "is_active": True,
        },
        "mapping": {
            "version": 1,
            "mapping": {
                "event": {
                    "occurred_at": {
                        "from": "ts",
                        "transform": [{"fn": "parse_datetime", "format": "rfc3339"}],
                    }
                }
            },
        },
        "jobs": {},
        "uploads": {},
        "schema": None,
    }

    def get_data_source(c: Any, p: uuid.UUID, s: uuid.UUID) -> dict[str, Any] | None:
        return state["source"] if s == SOURCE and p == PROJECT else None

    def insert_job(c: Any, **kw: Any) -> dict[str, Any]:
        job = {
            "id": uuid.uuid4(),
            "tenant_id": kw["tenant_id"],
            "project_id": kw["project_id"],
            "data_source_id": kw["source_id"],
            "mode": kw["mode"],
            "upload_id": kw["upload_id"],
            "status": "queued",
            "total_rows": kw["total_rows"],
            "processed_rows": 0,
            "accepted_rows": 0,
            "rejected_rows": 0,
            "error": None,
        }
        state["jobs"][job["id"]] = job
        return job

    def update_job(c: Any, job_id: uuid.UUID, **f: Any) -> None:
        state["jobs"].setdefault(
            job_id,
            {"id": job_id, "status": "queued", "processed_rows": 0, "accepted_rows": 0, "rejected_rows": 0},
        ).update(f)

    def add_job_counters(c: Any, job_id: uuid.UUID, processed: int, accepted: int, rejected: int) -> str:
        j = state["jobs"][job_id]
        j["processed_rows"] += processed
        j["accepted_rows"] += accepted
        j["rejected_rows"] += rejected
        return str(j["status"])

    monkeypatch.setattr(repo, "get_data_source", get_data_source)
    monkeypatch.setattr(repo, "get_active_mapping", lambda c, s: state["mapping"])
    monkeypatch.setattr(repo, "update_inferred_schema", lambda c, s, sc: state.__setitem__("schema", sc))
    monkeypatch.setattr(repo, "insert_upload", lambda c, **u: state["uploads"].__setitem__(u["id"], u))
    monkeypatch.setattr(repo, "get_upload", lambda c, p, u: state["uploads"].get(u))
    monkeypatch.setattr(repo, "insert_job", insert_job)
    monkeypatch.setattr(repo, "update_job", update_job)
    monkeypatch.setattr(repo, "add_job_counters", add_job_counters)
    monkeypatch.setattr(repo, "get_job", lambda c, p, j: state["jobs"].get(j))
    monkeypatch.setattr(routes, "tenant_session", fake_session)
    monkeypatch.setattr(runner, "tenant_session", fake_session)
    return state
