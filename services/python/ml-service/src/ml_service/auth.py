"""Authentication & authorisation.

Two caller types:
* users — `Authorization: Bearer <JWT>` issued by core-api (claims: sub, tid, trole, padmin, prj{pid: role}).
* services — `Authorization: Bearer ${INTERNAL_API_TOKEN}` + `X-Tenant-Id`, `X-Project-Id`, optional `X-Actor`.

Project role order: project_admin > approver > analyst > viewer. A tenant admin passes every project check of
their own tenant (project ownership is verified against the database under RLS).
"""

from __future__ import annotations

import hmac
from collections.abc import Callable
from dataclasses import dataclass
from typing import Annotated, Any
from uuid import UUID

import jwt
from fastapi import Depends, Header, Path
from sqlalchemy import text

from ml_service.config import get_settings
from ml_service.db import tenant_session
from ml_service.errors import ProblemError

ROLE_RANK: dict[str, int] = {"viewer": 1, "analyst": 2, "approver": 3, "project_admin": 4}


@dataclass(frozen=True)
class Principal:
    """Who is calling, and in which tenant/project context."""

    tenant_id: UUID | None
    project_id: UUID | None
    actor_id: UUID | None  # user id (JWT sub or X-Actor); None for anonymous service calls
    role: str | None  # effective project role (None for tenant-less principals)
    is_internal: bool = False
    is_platform_admin: bool = False
    is_tenant_admin: bool = False

    def has_role(self, minimum: str) -> bool:
        if self.is_internal:
            return True
        return ROLE_RANK.get(self.role or "", 0) >= ROLE_RANK[minimum]


def _unauthorized(detail: str) -> ProblemError:
    return ProblemError(401, "Unauthorized", detail, type_="unauthorized")


def _forbidden(detail: str) -> ProblemError:
    return ProblemError(403, "Forbidden", detail, type_="forbidden")


def _bearer(authorization: str | None) -> str:
    if not authorization or not authorization.lower().startswith("bearer "):
        raise _unauthorized("missing bearer token")
    return authorization[7:].strip()


def _is_internal_token(token: str) -> bool:
    expected = get_settings().internal_api_token
    return bool(expected) and hmac.compare_digest(token.encode(), expected.encode())


def decode_jwt(token: str) -> dict[str, Any]:
    settings = get_settings()
    try:
        claims: dict[str, Any] = jwt.decode(
            token,
            settings.jwt_secret,
            algorithms=[settings.jwt_algorithm],
            options={"require": ["sub", "exp"]},
        )
    except jwt.ExpiredSignatureError as exc:
        raise _unauthorized("token expired") from exc
    except jwt.PyJWTError as exc:
        raise _unauthorized("invalid token") from exc
    return claims


def _parse_uuid(value: str | None, what: str) -> UUID | None:
    if value is None or value == "":
        return None
    try:
        return UUID(str(value))
    except ValueError as exc:
        raise ProblemError(400, "Bad Request", f"invalid {what}") from exc


def _project_in_tenant(tenant_id: UUID, project_id: UUID) -> bool:
    with tenant_session(tenant_id) as conn:
        row = conn.execute(text("SELECT 1 FROM core.projects WHERE id = :p"), {"p": str(project_id)}).first()
    return row is not None


def _internal_principal(
    x_tenant_id: str | None, x_project_id: str | None, x_actor: str | None, path_project: UUID | None
) -> Principal:
    tenant = _parse_uuid(x_tenant_id, "X-Tenant-Id")
    project = _parse_uuid(x_project_id, "X-Project-Id") or path_project
    if path_project is not None:
        if tenant is None:
            raise ProblemError(400, "Bad Request", "X-Tenant-Id required for project calls")
        if project != path_project:
            raise _forbidden("X-Project-Id does not match path")
    return Principal(
        tenant_id=tenant,
        project_id=project,
        actor_id=_parse_uuid(x_actor, "X-Actor"),
        role="project_admin",
        is_internal=True,
    )


def _user_principal(claims: dict[str, Any], path_project: UUID | None) -> Principal:
    tenant = _parse_uuid(claims.get("tid"), "tid claim")
    is_padmin = bool(claims.get("padmin", False))
    is_tadmin = claims.get("trole") == "tenant_admin"
    actor = _parse_uuid(claims.get("sub"), "sub claim")
    role: str | None = None
    if path_project is not None:
        if tenant is None:
            # platform admins have no implicit access to project data (multi-tenancy.md §5)
            raise ProblemError(404, "Not Found", "project not found", type_="not_found")
        prj = claims.get("prj") or {}
        role = prj.get(str(path_project)) if isinstance(prj, dict) else None
        if role is None and is_tadmin and _project_in_tenant(tenant, path_project):
            role = "project_admin"
        if role is None:
            raise _forbidden("not a member of this project")
    return Principal(
        tenant_id=tenant,
        project_id=path_project,
        actor_id=actor,
        role=role,
        is_platform_admin=is_padmin,
        is_tenant_admin=is_tadmin,
    )


def _authenticate(
    authorization: str | None,
    x_tenant_id: str | None,
    x_project_id: str | None,
    x_actor: str | None,
    path_project: UUID | None,
) -> Principal:
    token = _bearer(authorization)
    if _is_internal_token(token):
        return _internal_principal(x_tenant_id, x_project_id, x_actor, path_project)
    return _user_principal(decode_jwt(token), path_project)


AuthHeader = Annotated[str | None, Header(alias="Authorization")]
TenantHeader = Annotated[str | None, Header(alias="X-Tenant-Id")]
ProjectHeader = Annotated[str | None, Header(alias="X-Project-Id")]
ActorHeader = Annotated[str | None, Header(alias="X-Actor")]


def require_project_role(minimum: str, *, allow_internal: bool = True) -> Callable[..., Principal]:
    """Dependency: caller must have at least `minimum` role on the `{pid}` path project."""

    def dependency(
        pid: Annotated[UUID, Path()],
        authorization: AuthHeader = None,
        x_tenant_id: TenantHeader = None,
        x_project_id: ProjectHeader = None,
        x_actor: ActorHeader = None,
    ) -> Principal:
        principal = _authenticate(authorization, x_tenant_id, x_project_id, x_actor, pid)
        if principal.is_internal and not allow_internal:
            raise _forbidden("internal token not accepted here")
        if not principal.has_role(minimum):
            raise _forbidden(f"requires role {minimum}")
        return principal

    return dependency


def require_internal_project() -> Callable[..., Principal]:
    """Dependency: internal service call scoped to the `{pid}` path project."""

    def dependency(
        pid: Annotated[UUID, Path()],
        authorization: AuthHeader = None,
        x_tenant_id: TenantHeader = None,
        x_project_id: ProjectHeader = None,
        x_actor: ActorHeader = None,
    ) -> Principal:
        token = _bearer(authorization)
        if not _is_internal_token(token):
            raise _forbidden("internal endpoint")
        return _internal_principal(x_tenant_id, x_project_id, x_actor, pid)

    return dependency


def require_internal(
    authorization: AuthHeader = None,
    x_tenant_id: TenantHeader = None,
    x_project_id: ProjectHeader = None,
    x_actor: ActorHeader = None,
) -> Principal:
    token = _bearer(authorization)
    if not _is_internal_token(token):
        raise _forbidden("internal endpoint")
    return _internal_principal(x_tenant_id, x_project_id, x_actor, None)


def require_authenticated(
    authorization: AuthHeader = None,
    x_tenant_id: TenantHeader = None,
    x_project_id: ProjectHeader = None,
    x_actor: ActorHeader = None,
) -> Principal:
    return _authenticate(authorization, x_tenant_id, x_project_id, x_actor, None)


def require_platform_admin_or_internal(
    principal: Annotated[Principal, Depends(require_authenticated)],
) -> Principal:
    if not (principal.is_internal or principal.is_platform_admin):
        raise _forbidden("platform admin only")
    return principal
