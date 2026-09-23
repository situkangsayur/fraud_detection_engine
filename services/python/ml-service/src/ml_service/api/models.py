"""Model registry: list, train, maker–checker activation."""

from __future__ import annotations

from typing import Annotated, Any
from uuid import UUID

from fastapi import APIRouter, Depends, Query, Request, status

from ml_service.api.deps import Page, actor_type, request_id
from ml_service.api.schemas import (
    DecisionRequest,
    TrainResponse,
    TrainSupervisedRequest,
    TrainUnsupervisedRequest,
)
from ml_service.auth import Principal, require_project_role
from ml_service.db import tenant_session
from ml_service.errors import ProblemError, conflict, not_found
from ml_service.features.catalog import FEATURE_SET_VERSION
from ml_service.ml_config import DEFAULT_ML_CONFIG, resolve_algorithm
from ml_service.plugins.registry import get_registry
from ml_service.repository import (
    archive_active,
    audit,
    cluster_fraud_rates,
    create_model,
    decide_approval,
    get_model,
    get_project,
    insert_approval_request,
    list_models,
    mark_interrupted,
    model_to_dict,
    update_model,
)
from ml_service.serving.cache import ModelCache, get_cache
from ml_service.training.jobs import PROCESS_STARTED_AT, get_runner
from ml_service.training.supervised import SupervisedJob, run_supervised
from ml_service.training.unsupervised import UnsupervisedJob, run_unsupervised

router = APIRouter(prefix="/api/v1/projects/{pid}/ml", tags=["models"])

Viewer = Annotated[Principal, Depends(require_project_role("viewer"))]
Analyst = Annotated[Principal, Depends(require_project_role("analyst"))]
Approver = Annotated[Principal, Depends(require_project_role("approver"))]


def _tenant(principal: Principal) -> UUID:
    if principal.tenant_id is None:
        raise ProblemError(403, "Forbidden", "tenant context required")
    return principal.tenant_id


def _project_ml_config(conn: Any, pid: UUID) -> dict[str, Any]:
    project = get_project(conn, pid)
    if project is None:
        raise not_found("project")
    if project["status"] != "active":
        raise conflict("project is archived", "project_archived")
    cfg = project["ml_config"] or {}
    return {key: {**DEFAULT_ML_CONFIG[key], **(cfg.get(key) or {})} for key in DEFAULT_ML_CONFIG}


def _raise_invalid(errors: list[dict[str, str]]) -> None:
    if errors:
        raise ProblemError(
            422,
            "Invalid algorithm configuration",
            "see errors",
            "invalid_ml_config",
            [{"field": e["path"], "message": e["message"]} for e in errors],
        )


@router.get("/models")
def models(
    pid: UUID,
    principal: Viewer,
    page: Annotated[Page, Depends()],
    kind: str | None = Query(None, pattern="^(supervised|unsupervised)$"),
    status_: str | None = Query(None, alias="status"),
) -> dict[str, Any]:
    tenant = _tenant(principal)
    with tenant_session(tenant) as conn:
        mark_interrupted(conn, pid, get_runner().running(), PROCESS_STARTED_AT)
        rows, total = list_models(conn, pid, kind, status_, page.page, page.page_size)
    return page.wrap([model_to_dict(r) for r in rows], total)


@router.get("/models/{model_id}")
def model_detail(pid: UUID, model_id: UUID, principal: Viewer) -> dict[str, Any]:
    with tenant_session(_tenant(principal)) as conn:
        mark_interrupted(conn, pid, get_runner().running(), PROCESS_STARTED_AT)
        row = get_model(conn, pid, model_id)
    if row is None:
        raise not_found("model")
    return model_to_dict(row)


@router.post("/supervised/train", status_code=status.HTTP_202_ACCEPTED, response_model=TrainResponse)
def train_supervised(
    pid: UUID, body: TrainSupervisedRequest, request: Request, principal: Analyst
) -> TrainResponse:
    tenant = _tenant(principal)
    registry = get_registry()
    with tenant_session(tenant) as conn:
        cfg = _project_ml_config(conn, pid)
        sup = cfg["supervised"]
        name = body.algorithm or sup["algorithm"]
        params = (
            body.params
            if body.params is not None
            else (sup.get("params") if name == sup["algorithm"] else {})
        )
        resolved, errors = resolve_algorithm(registry, name, params, "supervised", "algorithm")
        _raise_invalid(errors)
        assert resolved is not None
        entry = registry.get(resolved.name)
        row = create_model(
            conn,
            tenant_id=tenant,
            project_id=pid,
            kind="supervised",
            algorithms={"supervised": {"name": resolved.name, "version": entry.version if entry else "?"}},
            params={
                "supervised": resolved.params,
                "since_days": body.since_days,
                "label_maturity_days": body.label_maturity_days,
                "features": cfg["features"],
            },
            feature_set_version=FEATURE_SET_VERSION,
            created_by=principal.actor_id,
        )
        audit(
            conn,
            tenant_id=tenant,
            project_id=pid,
            actor_id=principal.actor_id,
            actor_type=actor_type(principal),
            action="ml.model.train",
            subject_type="model",
            subject_id=str(row["id"]),
            after={"kind": "supervised", "algorithm": resolved.name},
            request_id=request_id(request),
        )
    job = SupervisedJob(
        tenant,
        pid,
        row["id"],
        resolved.name,
        resolved.params,
        cfg["features"],
        body.since_days,
        body.label_maturity_days or None,
    )
    get_runner().submit(str(row["id"]), "supervised", run_supervised, job)
    return TrainResponse(model_id=row["id"], version=row["version"], status="training")


@router.post("/unsupervised/train", status_code=status.HTTP_202_ACCEPTED, response_model=TrainResponse)
def train_unsupervised(
    pid: UUID, body: TrainUnsupervisedRequest, request: Request, principal: Analyst
) -> TrainResponse:
    tenant = _tenant(principal)
    registry = get_registry()
    with tenant_session(tenant) as conn:
        cfg = _project_ml_config(conn, pid)
        uns = cfg["unsupervised"]
        a_name = body.anomaly_algorithm or uns["anomaly_algorithm"]
        a_params = (
            body.anomaly_params
            if body.anomaly_params is not None
            else (uns.get("anomaly_params") if a_name == uns["anomaly_algorithm"] else {})
        )
        c_name = body.clustering_algorithm or uns["clustering_algorithm"]
        c_params = (
            body.clustering_params
            if body.clustering_params is not None
            else (uns.get("clustering_params") if c_name == uns["clustering_algorithm"] else {})
        )
        anomaly, a_err = resolve_algorithm(registry, a_name, a_params, "anomaly", "anomaly_algorithm")
        clustering, c_err = resolve_algorithm(
            registry, c_name, c_params, "clustering", "clustering_algorithm"
        )
        _raise_invalid(a_err + c_err)
        assert anomaly is not None and clustering is not None
        a_entry, c_entry = registry.get(anomaly.name), registry.get(clustering.name)
        row = create_model(
            conn,
            tenant_id=tenant,
            project_id=pid,
            kind="unsupervised",
            algorithms={
                "anomaly": {"name": anomaly.name, "version": a_entry.version if a_entry else "?"},
                "clustering": {"name": clustering.name, "version": c_entry.version if c_entry else "?"},
            },
            params={
                "anomaly": anomaly.params,
                "clustering": clustering.params,
                "since_days": body.since_days,
                "features": cfg["features"],
            },
            feature_set_version=FEATURE_SET_VERSION,
            created_by=principal.actor_id,
        )
        audit(
            conn,
            tenant_id=tenant,
            project_id=pid,
            actor_id=principal.actor_id,
            actor_type=actor_type(principal),
            action="ml.model.train",
            subject_type="model",
            subject_id=str(row["id"]),
            after={"kind": "unsupervised", "anomaly": anomaly.name, "clustering": clustering.name},
            request_id=request_id(request),
        )
    job = UnsupervisedJob(
        tenant,
        pid,
        row["id"],
        anomaly.name,
        anomaly.params,
        clustering.name,
        clustering.params,
        cfg["features"],
        body.since_days,
    )
    get_runner().submit(str(row["id"]), "unsupervised", run_unsupervised, job)
    return TrainResponse(model_id=row["id"], version=row["version"], status="training")


@router.post("/models/{model_id}/submit")
def submit_model(
    pid: UUID, model_id: UUID, body: DecisionRequest, request: Request, principal: Analyst
) -> dict[str, Any]:
    tenant = _tenant(principal)
    if principal.actor_id is None:
        raise ProblemError(400, "Bad Request", "an actor (user) is required for maker–checker actions")
    with tenant_session(tenant) as conn:
        row = get_model(conn, pid, model_id)
        if row is None:
            raise not_found("model")
        if row["status"] != "ready":
            raise conflict(
                f"only 'ready' models can be submitted (status is '{row['status']}')", "invalid_status"
            )
        update_model(conn, model_id, status="pending_approval", submitted_by=principal.actor_id)
        insert_approval_request(conn, tenant, pid, model_id, int(row["version"]), principal.actor_id)
        audit(
            conn,
            tenant_id=tenant,
            project_id=pid,
            actor_id=principal.actor_id,
            actor_type=actor_type(principal),
            action="ml.model.submit",
            subject_type="model",
            subject_id=str(model_id),
            before={"status": row["status"]},
            after={"status": "pending_approval"},
            metadata={"comment": body.comment} if body.comment else None,
            request_id=request_id(request),
        )
        updated = get_model(conn, pid, model_id)
    assert updated is not None
    return model_to_dict(updated)


@router.post("/models/{model_id}/approve")
def approve_model(
    pid: UUID, model_id: UUID, body: DecisionRequest, request: Request, principal: Approver
) -> dict[str, Any]:
    tenant = _tenant(principal)
    if principal.actor_id is None:
        raise ProblemError(400, "Bad Request", "an actor (user) is required for maker–checker actions")
    with tenant_session(tenant) as conn:
        row = get_model(conn, pid, model_id)
        if row is None:
            raise not_found("model")
        if row["status"] != "pending_approval":
            raise conflict(f"model is not pending approval (status is '{row['status']}')", "invalid_status")
        if row["submitted_by"] is not None and row["submitted_by"] == principal.actor_id:
            raise ProblemError(
                403,
                "Forbidden",
                "the approver must differ from the submitter (four-eyes)",
                type_="four_eyes_violation",
            )
        kind = str(row["kind"])
        rates = cluster_fraud_rates(conn, model_id) if kind == "unsupervised" else {}
    # Load before switching so an unloadable artifact never becomes the active model.
    loaded = ModelCache._instantiate(kind, str(model_id), int(row["version"]), _artifact(row), rates)
    with tenant_session(tenant) as conn:
        archive_active(conn, pid, kind)
        update_model(conn, model_id, status="active", activated_at="now")
        decide_approval(conn, model_id, principal.actor_id, "approved", body.comment)
        audit(
            conn,
            tenant_id=tenant,
            project_id=pid,
            actor_id=principal.actor_id,
            actor_type=actor_type(principal),
            action="ml.model.approve",
            subject_type="model",
            subject_id=str(model_id),
            before={"status": "pending_approval"},
            after={"status": "active"},
            metadata={"comment": body.comment} if body.comment else None,
            request_id=request_id(request),
        )
        updated = get_model(conn, pid, model_id)
    get_cache()._put((str(pid), kind), loaded)  # hot swap
    assert updated is not None
    return model_to_dict(updated)


@router.post("/models/{model_id}/reject")
def reject_model(
    pid: UUID, model_id: UUID, body: DecisionRequest, request: Request, principal: Approver
) -> dict[str, Any]:
    tenant = _tenant(principal)
    with tenant_session(tenant) as conn:
        row = get_model(conn, pid, model_id)
        if row is None:
            raise not_found("model")
        if row["status"] != "pending_approval":
            raise conflict(f"model is not pending approval (status is '{row['status']}')", "invalid_status")
        update_model(conn, model_id, status="ready")
        decide_approval(conn, model_id, principal.actor_id, "rejected", body.comment)
        audit(
            conn,
            tenant_id=tenant,
            project_id=pid,
            actor_id=principal.actor_id,
            actor_type=actor_type(principal),
            action="ml.model.reject",
            subject_type="model",
            subject_id=str(model_id),
            before={"status": "pending_approval"},
            after={"status": "ready"},
            metadata={"comment": body.comment} if body.comment else None,
            request_id=request_id(request),
        )
        updated = get_model(conn, pid, model_id)
    assert updated is not None
    return model_to_dict(updated)


def _artifact(row: Any) -> Any:
    from pathlib import Path

    if not row["artifact_path"]:
        raise conflict("model has no artifacts", "no_artifacts")
    return Path(row["artifact_path"])
