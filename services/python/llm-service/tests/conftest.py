from __future__ import annotations

import time
import uuid
from typing import Any

import jwt
import pytest

from llm_service.config import Settings

TENANT = uuid.UUID("11111111-1111-1111-1111-111111111111")
PROJECT = uuid.UUID("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")
OTHER_PROJECT = uuid.UUID("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb")
USER = uuid.UUID("99999999-9999-9999-9999-999999999999")


@pytest.fixture
def settings(tmp_path: Any) -> Settings:
    return Settings(
        database_url="postgresql+psycopg://llm_service:l@127.0.0.1:1/none",
        internal_api_token="test-internal-token",
        jwt_secret="test-jwt-secret-with-enough-length",
        core_api_url="http://core.test",
        rule_service_url="http://rule.test",
        graph_service_url="http://graph.test",
        ml_service_url="http://ml.test",
        ollama_url="http://ollama.test",
        opensearch_url="http://opensearch.test:9200",
        regulation_dir=str(tmp_path / "regs"),
        tool_timeout_s=2.0,
    )


def make_jwt(
    settings: Settings,
    role: str | None = "analyst",
    *,
    tenant_role: str = "member",
    project: uuid.UUID = PROJECT,
    tenant: uuid.UUID = TENANT,
) -> str:
    claims: dict[str, Any] = {
        "sub": str(USER),
        "tid": str(tenant),
        "trole": tenant_role,
        "padmin": False,
        "prj": {str(project): role} if role else {},
        "exp": int(time.time()) + 600,
        "iat": int(time.time()),
        "jti": str(uuid.uuid4()),
    }
    return jwt.encode(claims, settings.jwt_secret, algorithm="HS256")


def auth(settings: Settings, **kw: Any) -> dict[str, str]:
    return {"Authorization": f"Bearer {make_jwt(settings, **kw)}"}


def internal_headers(settings: Settings, project: uuid.UUID = PROJECT) -> dict[str, str]:
    return {
        "Authorization": f"Bearer {settings.internal_api_token}",
        "X-Tenant-Id": str(TENANT),
        "X-Project-Id": str(project),
        "X-Actor": str(USER),
    }
