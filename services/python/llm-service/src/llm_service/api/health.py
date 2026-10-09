"""Liveness, readiness and Prometheus metrics."""

from __future__ import annotations

from typing import Annotated, Any

from fastapi import APIRouter, Depends, Response
from fastapi.responses import JSONResponse
from prometheus_client import CONTENT_TYPE_LATEST, generate_latest
from starlette.concurrency import run_in_threadpool

from llm_service.container import Container, get_container
from llm_service.db import ping

router = APIRouter(tags=["health"])


@router.get("/health/live")
async def live() -> dict[str, str]:
    return {"status": "ok"}


@router.get("/health/ready")
async def ready(c: Annotated[Container, Depends(get_container)]) -> JSONResponse:
    checks: dict[str, Any] = {
        "db": await run_in_threadpool(ping),
        "ollama": await c.ollama.ping(),
        "opensearch": await c.store.ping(),
    }
    ok = all(checks.values())
    return JSONResponse(
        {"status": "ok" if ok else "degraded", **checks, "jobs_running": c.jobs.running}, status_code=200 if ok else 503
    )


@router.get("/metrics")
async def metrics() -> Response:
    return Response(generate_latest(), media_type=CONTENT_TYPE_LATEST)
