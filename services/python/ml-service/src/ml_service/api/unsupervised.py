"""Unsupervised results: clusters, 2-D projection, top anomalies."""

from __future__ import annotations

from typing import Annotated, Any
from uuid import UUID

from fastapi import APIRouter, Depends, Query, Request

from ml_service.api.deps import actor_type, request_id
from ml_service.api.schemas import ClusterPatch
from ml_service.auth import Principal, require_project_role
from ml_service.db import tenant_session
from ml_service.errors import not_found
from ml_service.repository import (
    audit,
    get_active_model,
    get_model,
    list_clusters,
    projection,
    top_anomalies,
    update_cluster_label,
)
from ml_service.serving.cache import get_cache, no_active_model

router = APIRouter(prefix="/api/v1/projects/{pid}/ml/unsupervised", tags=["unsupervised"])
Viewer = Annotated[Principal, Depends(require_project_role("viewer"))]
Analyst = Annotated[Principal, Depends(require_project_role("analyst"))]


def _model_id(conn: Any, pid: UUID, model_id: UUID | None) -> UUID:
    if model_id is not None:
        row = get_model(conn, pid, model_id)
        if row is None or row["kind"] != "unsupervised":
            raise not_found("unsupervised model")
        return model_id
    active = get_active_model(conn, pid, "unsupervised")
    if active is None:
        raise no_active_model("unsupervised")
    return UUID(str(active["id"]))


def _cluster_dict(r: Any) -> dict[str, Any]:
    return {**dict(r), "model_id": str(r["model_id"])}


@router.get("/clusters")
def clusters(pid: UUID, principal: Viewer, model_id: UUID | None = None) -> dict[str, Any]:
    assert principal.tenant_id is not None
    with tenant_session(principal.tenant_id) as conn:
        mid = _model_id(conn, pid, model_id)
        rows = list_clusters(conn, mid)
    return {"model_id": str(mid), "items": [_cluster_dict(r) for r in rows]}


@router.patch("/clusters/{model_id}/{cluster_id}")
def patch_cluster(
    pid: UUID, model_id: UUID, cluster_id: int, body: ClusterPatch, request: Request, principal: Analyst
) -> dict[str, Any]:
    assert principal.tenant_id is not None
    with tenant_session(principal.tenant_id) as conn:
        _model_id(conn, pid, model_id)
        row = update_cluster_label(conn, model_id, cluster_id, body.label, body.notes)
        if row is None:
            raise not_found("cluster")
        audit(
            conn,
            tenant_id=principal.tenant_id,
            project_id=pid,
            actor_id=principal.actor_id,
            actor_type=actor_type(principal),
            action="ml.cluster.label",
            subject_type="cluster",
            subject_id=f"{model_id}:{cluster_id}",
            after=body.model_dump(),
            request_id=request_id(request),
        )
    return _cluster_dict(row)


@router.get("/projection")
def get_projection(
    pid: UUID, principal: Viewer, model_id: UUID | None = None, limit: int = Query(2000, ge=1, le=5000)
) -> dict[str, Any]:
    assert principal.tenant_id is not None
    with tenant_session(principal.tenant_id) as conn:
        mid = _model_id(conn, pid, model_id)
        rows = projection(conn, mid, limit)
    return {"model_id": str(mid), "items": [{**dict(r), "event_id": str(r["event_id"])} for r in rows]}


@router.get("/anomalies")
def anomalies(
    pid: UUID,
    principal: Viewer,
    model_id: UUID | None = None,
    limit: int = Query(100, ge=1, le=1000),
    min_score: float = Query(0.9, ge=0, le=1),
) -> dict[str, Any]:
    assert principal.tenant_id is not None
    with tenant_session(principal.tenant_id) as conn:
        mid = _model_id(conn, pid, model_id)
        rows = top_anomalies(conn, pid, mid, limit, min_score)
    items = [
        {
            **dict(r),
            "event_id": str(r["event_id"]),
            "customer_id": str(r["customer_id"]),
            "amount": float(r["amount"]) if r["amount"] is not None else None,
        }
        for r in rows
    ]
    return {"model_id": str(mid), "items": items}


# keep the cache warm-able from here too (used by tests / admin tooling)
__all__ = ["get_cache", "router"]
