"""Graph communities (Louvain) — computed here, consumed by graph-service (`community_fraud_rate`)."""

from __future__ import annotations

from typing import Annotated, Any
from uuid import UUID

from fastapi import APIRouter, Depends, Query, Request

from ml_service.api.deps import actor_type, request_id
from ml_service.auth import Principal, require_project_role
from ml_service.db import tenant_session
from ml_service.graph.communities import recompute
from ml_service.repository import audit, list_communities

router = APIRouter(prefix="/api/v1/projects/{pid}/ml/graph-communities", tags=["graph"])
Viewer = Annotated[Principal, Depends(require_project_role("viewer"))]
Analyst = Annotated[Principal, Depends(require_project_role("analyst"))]


@router.get("")
def communities(
    pid: UUID, principal: Viewer, min_size: int = Query(2, ge=1), limit: int = Query(200, ge=1, le=2000)
) -> dict[str, Any]:
    assert principal.tenant_id is not None
    with tenant_session(principal.tenant_id) as conn:
        items = list_communities(conn, pid, min_size, limit)
    return {"items": items}


@router.post("/recompute")
def recompute_communities(pid: UUID, request: Request, principal: Analyst) -> dict[str, Any]:
    assert principal.tenant_id is not None
    result = recompute(principal.tenant_id, pid)
    with tenant_session(principal.tenant_id) as conn:
        audit(
            conn,
            tenant_id=principal.tenant_id,
            project_id=pid,
            actor_id=principal.actor_id,
            actor_type=actor_type(principal),
            action="ml.graph_communities.recompute",
            subject_type="project",
            subject_id=str(pid),
            after=result,
            request_id=request_id(request),
        )
    return result
