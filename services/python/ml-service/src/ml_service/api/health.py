"""Liveness, readiness and Prometheus metrics."""

from __future__ import annotations

from fastapi import APIRouter, Response
from fastapi.responses import JSONResponse
from prometheus_client import CONTENT_TYPE_LATEST, generate_latest

from ml_service.db import ping
from ml_service.plugins.registry import get_registry
from ml_service.serving.cache import get_cache

router = APIRouter(tags=["health"])


@router.get("/health/live")
def live() -> dict[str, str]:
    return {"status": "ok"}


@router.get("/health/ready")
def ready() -> JSONResponse:
    db_ok = ping()
    entries = get_registry().entries()
    body = {
        "status": "ok" if db_ok else "unavailable",
        "db": db_ok,
        "algorithms_available": sum(e.status == "available" for e in entries),
        "models_loaded": get_cache().stats()["loaded"],
    }
    return JSONResponse(body, status_code=200 if db_ok else 503)


@router.get("/metrics")
def metrics() -> Response:
    return Response(generate_latest(), media_type=CONTENT_TYPE_LATEST)
