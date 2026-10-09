"""AuthN/AuthZ: user JWTs (issued by core-api) and the internal service token.

JWT claims (architecture §5): sub, tid, trole, padmin, prj{project_id: role}, exp, iat, jti.
Project roles are ordered project_admin > approver > analyst > viewer; tenant admins pass every
project check of their tenant.
"""

from __future__ import annotations

import hmac
import uuid
from dataclasses import dataclass, field
from typing import Annotated

import jwt
from fastapi import Depends, Header, Path, Request

from app.config import Settings, get_settings
from app.errors import ProblemError

ROLE_RANK = {"viewer": 1, "analyst": 2, "approver": 3, "project_admin": 4}


@dataclass(frozen=True)
class Principal:
    user_id: str | None
    tenant_id: uuid.UUID | None
    is_service: bool = False
    tenant_role: str | None = None
    platform_admin: bool = False
    projects: dict[str, str] = field(default_factory=dict)

    def project_role_rank(self, project_id: uuid.UUID) -> int:
        if self.is_service or self.tenant_role == "tenant_admin":
            return ROLE_RANK["project_admin"]
        return ROLE_RANK.get(self.projects.get(str(project_id), ""), 0)


def _bearer(request: Request) -> str:
    auth = request.headers.get("authorization", "")
    if not auth.lower().startswith("bearer "):
        raise ProblemError(401, "Unauthorized", "missing bearer token")
    return auth[7:].strip()


def authenticate(
    request: Request,
    settings: Annotated[Settings, Depends(get_settings)],
    x_tenant_id: Annotated[str | None, Header()] = None,
    x_actor: Annotated[str | None, Header()] = None,
) -> Principal:
    token = _bearer(request)
    if hmac.compare_digest(token.encode(), settings.internal_api_token.encode()):
        if not x_tenant_id:
            raise ProblemError(400, "Bad request", "X-Tenant-Id header required for internal calls")
        try:
            tid = uuid.UUID(x_tenant_id)
        except ValueError as e:
            raise ProblemError(400, "Bad request", "invalid X-Tenant-Id") from e
        return Principal(user_id=x_actor, tenant_id=tid, is_service=True)
    try:
        claims = jwt.decode(
            token,
            settings.jwt_secret,
            algorithms=[settings.jwt_algorithm],
            options={"require": ["exp", "sub"]},
        )
    except jwt.PyJWTError as e:
        raise ProblemError(401, "Unauthorized", "invalid token") from e
    raw_tid = claims.get("tid")
    return Principal(
        user_id=str(claims["sub"]),
        tenant_id=uuid.UUID(str(raw_tid)) if raw_tid else None,
        tenant_role=claims.get("trole"),
        platform_admin=bool(claims.get("padmin", False)),
        projects={str(k): str(v) for k, v in (claims.get("prj") or {}).items()},
    )


class RequireProjectRole:
    """Dependency: principal must hold at least `role` on the path's project."""

    def __init__(self, role: str) -> None:
        self.min_rank = ROLE_RANK[role]

    def __call__(
        self,
        pid: Annotated[uuid.UUID, Path()],
        principal: Annotated[Principal, Depends(authenticate)],
    ) -> Principal:
        if principal.tenant_id is None:
            raise ProblemError(403, "Forbidden", "no tenant context")
        if principal.project_role_rank(pid) < self.min_rank:
            raise ProblemError(403, "Forbidden", "insufficient project role")
        return principal


require_viewer = RequireProjectRole("viewer")
require_analyst = RequireProjectRole("analyst")
