"""Authentication & authorisation.

* User calls carry a JWT issued by core-api (claims: sub, tid, trole, padmin, prj{pid: role}).
* Service calls carry ``Authorization: Bearer <INTERNAL_API_TOKEN>`` plus ``X-Tenant-Id`` / ``X-Project-Id``
  / ``X-Actor`` headers.
"""

from __future__ import annotations

import hmac
import uuid
from dataclasses import dataclass, field
from typing import Annotated, Literal

import jwt
from fastapi import Depends, Header, Path, Request

from llm_service.config import Settings, get_settings
from llm_service.errors import ProblemError

ProjectRole = Literal["project_admin", "approver", "analyst", "viewer"]
_ROLE_RANK: dict[str, int] = {"viewer": 1, "analyst": 2, "approver": 3, "project_admin": 4}


@dataclass(frozen=True)
class Principal:
    """Who is calling. ``user_id`` is None for service calls without X-Actor."""

    kind: Literal["user", "service"]
    tenant_id: uuid.UUID | None
    user_id: uuid.UUID | None = None
    tenant_role: str | None = None
    platform_admin: bool = False
    projects: dict[str, str] = field(default_factory=dict)
    project_header: uuid.UUID | None = None

    def project_role_rank(self, project_id: uuid.UUID) -> int:
        if self.kind == "service":
            return 99
        if self.tenant_role == "tenant_admin":
            return _ROLE_RANK["project_admin"]
        return _ROLE_RANK.get(self.projects.get(str(project_id), ""), 0)

    def require_project(self, project_id: uuid.UUID, min_role: ProjectRole) -> None:
        if self.kind == "service":
            if self.project_header is not None and self.project_header != project_id:
                raise ProblemError(403, "Forbidden", "X-Project-Id does not match path")
            return
        rank = self.project_role_rank(project_id)
        if rank == 0:
            # Not a member (incl. platform admins: privacy by default) → do not reveal the project exists.
            raise ProblemError(404, "Not Found", "project not found")
        if rank < _ROLE_RANK[min_role]:
            raise ProblemError(403, "Forbidden", f"requires role {min_role} on project")

    def require_tenant(self, tenant_id: uuid.UUID, *, admin: bool = False) -> None:
        # Platform admins have no implicit access to tenant data (multi-tenancy.md §5).
        if self.tenant_id != tenant_id:
            raise ProblemError(403, "Forbidden", "tenant mismatch")
        if admin and self.kind == "user" and self.tenant_role != "tenant_admin":
            raise ProblemError(403, "Forbidden", "requires tenant_admin")


def _parse_uuid(value: str | None, name: str) -> uuid.UUID | None:
    if not value:
        return None
    try:
        return uuid.UUID(value)
    except ValueError as exc:
        raise ProblemError(400, "Bad Request", f"invalid {name}") from exc


def decode_principal(
    authorization: str | None,
    x_tenant_id: str | None,
    x_project_id: str | None,
    x_actor: str | None,
    settings: Settings,
) -> Principal:
    if not authorization or not authorization.lower().startswith("bearer "):
        raise ProblemError(401, "Unauthorized", "missing bearer token")
    token = authorization[7:].strip()
    if hmac.compare_digest(token.encode(), settings.internal_api_token.encode()):
        return Principal(
            kind="service",
            tenant_id=_parse_uuid(x_tenant_id, "X-Tenant-Id"),
            project_header=_parse_uuid(x_project_id, "X-Project-Id"),
            user_id=_parse_uuid(x_actor, "X-Actor"),
        )
    try:
        claims = jwt.decode(
            token, settings.jwt_secret, algorithms=[settings.jwt_algorithm], options={"require": ["exp", "sub"]}
        )
    except jwt.PyJWTError as exc:
        raise ProblemError(401, "Unauthorized", "invalid token") from exc
    return Principal(
        kind="user",
        tenant_id=_parse_uuid(claims.get("tid"), "tid"),
        user_id=_parse_uuid(str(claims["sub"]), "sub"),
        tenant_role=claims.get("trole"),
        platform_admin=bool(claims.get("padmin", False)),
        projects={str(k): str(v) for k, v in (claims.get("prj") or {}).items()},
    )


def _app_settings(request: Request) -> Settings:
    container = getattr(request.app.state, "container", None)
    return container.settings if container is not None else get_settings()


async def get_principal(
    request: Request,
    authorization: Annotated[str | None, Header()] = None,
    x_tenant_id: Annotated[str | None, Header()] = None,
    x_project_id: Annotated[str | None, Header()] = None,
    x_actor: Annotated[str | None, Header()] = None,
) -> Principal:
    return decode_principal(authorization, x_tenant_id, x_project_id, x_actor, _app_settings(request))


async def require_internal(principal: Annotated[Principal, Depends(get_principal)]) -> Principal:
    if principal.kind != "service":
        raise ProblemError(403, "Forbidden", "internal endpoint")
    return principal


@dataclass(frozen=True)
class ProjectScope:
    tenant_id: uuid.UUID
    project_id: uuid.UUID
    principal: Principal


def project_scope(min_role: ProjectRole):  # type: ignore[no-untyped-def]
    """Dependency factory: resolves tenant for a project-scoped route and enforces the role."""

    async def _dep(
        pid: Annotated[uuid.UUID, Path()],
        principal: Annotated[Principal, Depends(get_principal)],
    ) -> ProjectScope:
        principal.require_project(pid, min_role)
        if principal.tenant_id is None:
            raise ProblemError(400, "Bad Request", "tenant context missing")
        return ProjectScope(tenant_id=principal.tenant_id, project_id=pid, principal=principal)

    return _dep
