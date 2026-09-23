"""Plugin catalogue, hot reload and ml_config validation."""

from __future__ import annotations

from typing import Annotated, Any

from fastapi import APIRouter, Depends, Request

from ml_service.api.deps import actor_type, request_id
from ml_service.api.schemas import ValidateConfigRequest, ValidateConfigResponse
from ml_service.auth import (
    Principal,
    require_authenticated,
    require_internal,
    require_platform_admin_or_internal,
)
from ml_service.db import platform_session
from ml_service.logging import get_logger
from ml_service.ml_config import validate_ml_config
from ml_service.plugins.registry import get_registry
from ml_service.repository import audit
from ml_service.serving.cache import get_cache

router = APIRouter(tags=["algorithms"])
log = get_logger(__name__)


@router.get("/api/v1/ml/algorithms")
def list_algorithms(
    _: Annotated[Principal, Depends(require_authenticated)], kind: str | None = None
) -> list[dict[str, Any]]:
    return [e.public() for e in get_registry().entries() if kind is None or e.kind == kind]


@router.post("/api/v1/ml/algorithms/reload")
def reload_algorithms(
    request: Request, principal: Annotated[Principal, Depends(require_platform_admin_or_internal)]
) -> dict[str, Any]:
    registry = get_registry()
    registry.load_all()
    registry.sync_to_db()
    get_cache().invalidate()  # models are rebuilt from (possibly reloaded) plugin classes
    invalid = [
        {"module": e.module, "name": e.name, "error": e.error}
        for e in registry.entries()
        if e.status == "invalid"
    ]
    invalid += [{"module": m.module, "name": None, "error": m.error} for m in registry.invalid_modules()]
    result = {"loaded": sum(e.status == "available" for e in registry.entries()), "invalid": invalid}
    with platform_session() as conn:
        audit(
            conn,
            tenant_id=principal.tenant_id,
            project_id=None,
            actor_id=principal.actor_id,
            actor_type=actor_type(principal),
            action="ml.algorithms.reload",
            subject_type="ml_algorithms",
            subject_id="*",
            after={"loaded": result["loaded"], "invalid": len(invalid)},
            request_id=request_id(request),
        )
    return result


@router.post("/v1/algorithms/validate-config", response_model=ValidateConfigResponse)
def validate_config(
    body: ValidateConfigRequest, _: Annotated[Principal, Depends(require_internal)]
) -> ValidateConfigResponse:
    errors = validate_ml_config(get_registry(), body.ml_config)
    return ValidateConfigResponse(valid=not errors, errors=errors)
