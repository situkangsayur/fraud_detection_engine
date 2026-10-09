from __future__ import annotations

import uuid

import pytest

from llm_service.auth import decode_principal
from llm_service.errors import ProblemError
from tests.conftest import PROJECT, TENANT, make_jwt


def test_user_jwt_roles(settings) -> None:  # type: ignore[no-untyped-def]
    p = decode_principal(f"Bearer {make_jwt(settings, 'analyst')}", None, None, None, settings)
    assert p.kind == "user" and p.tenant_id == TENANT
    p.require_project(PROJECT, "viewer")
    p.require_project(PROJECT, "analyst")
    with pytest.raises(ProblemError) as e:
        p.require_project(PROJECT, "approver")
    assert e.value.status == 403
    with pytest.raises(ProblemError) as nf:
        p.require_project(uuid.uuid4(), "viewer")
    assert nf.value.status == 404  # non-members do not learn that the project exists


def test_platform_admin_has_no_implicit_tenant_access(settings) -> None:  # type: ignore[no-untyped-def]
    import jwt as pyjwt

    token = pyjwt.encode(
        {"sub": str(uuid.uuid4()), "tid": None, "padmin": True, "prj": {}, "exp": 9999999999},
        settings.jwt_secret,
        algorithm="HS256",
    )
    p = decode_principal(f"Bearer {token}", None, None, None, settings)
    with pytest.raises(ProblemError):
        p.require_tenant(TENANT)
    with pytest.raises(ProblemError) as e:
        p.require_project(PROJECT, "viewer")
    assert e.value.status == 404


def test_tenant_admin_is_project_admin_everywhere(settings) -> None:  # type: ignore[no-untyped-def]
    p = decode_principal(f"Bearer {make_jwt(settings, None, tenant_role='tenant_admin')}", None, None, None, settings)
    p.require_project(uuid.uuid4(), "project_admin")
    p.require_tenant(TENANT, admin=True)
    with pytest.raises(ProblemError):
        p.require_tenant(uuid.uuid4())


def test_internal_token_and_project_header(settings) -> None:  # type: ignore[no-untyped-def]
    p = decode_principal(f"Bearer {settings.internal_api_token}", str(TENANT), str(PROJECT), None, settings)
    assert p.kind == "service"
    p.require_project(PROJECT, "project_admin")
    with pytest.raises(ProblemError):
        p.require_project(uuid.uuid4(), "viewer")


def test_rejects_bad_tokens(settings) -> None:  # type: ignore[no-untyped-def]
    for header in (None, "Basic abc", "Bearer not-a-jwt"):
        with pytest.raises(ProblemError) as e:
            decode_principal(header, None, None, None, settings)
        assert e.value.status == 401
    forged = make_jwt(settings.model_copy(update={"jwt_secret": "another-secret-with-enough-length"}))
    with pytest.raises(ProblemError):
        decode_principal(f"Bearer {forged}", None, None, None, settings)
