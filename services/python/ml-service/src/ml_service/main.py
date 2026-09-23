"""FastAPI application factory."""

from __future__ import annotations

import time
import uuid
from collections.abc import AsyncIterator, Awaitable, Callable
from contextlib import asynccontextmanager

import structlog
from fastapi import FastAPI, Request, Response
from prometheus_client import Counter, Histogram

from ml_service import __version__
from ml_service.api import algorithms, graph, health, inference, models, unsupervised
from ml_service.config import get_settings
from ml_service.errors import install_error_handlers
from ml_service.logging import configure_logging, get_logger
from ml_service.plugins.registry import get_registry
from ml_service.training.jobs import get_runner

log = get_logger(__name__)
REQUESTS = Counter("http_requests_total", "HTTP requests", ["method", "route", "status"])
LATENCY = Histogram("http_request_duration_seconds", "HTTP request latency", ["method", "route"])


@asynccontextmanager
async def lifespan(_: FastAPI) -> AsyncIterator[None]:
    settings = get_settings()
    configure_logging(settings.log_level)
    registry = get_registry()
    try:
        registry.sync_to_db()
    except Exception as exc:  # DB may still be starting; readiness reports it
        log.warning("algorithm_sync_failed", error=str(exc))
    log.info("ml_service_started", version=__version__, algorithms=len(registry.entries()))
    yield
    get_runner().shutdown()


def create_app() -> FastAPI:
    app = FastAPI(
        title="ml-service",
        version=__version__,
        description="Algorithm plugins, training, model registry and serving (fraud platform).",
        lifespan=lifespan,
    )
    install_error_handlers(app)

    @app.middleware("http")
    async def request_context(
        request: Request, call_next: Callable[[Request], Awaitable[Response]]
    ) -> Response:
        rid = request.headers.get("x-request-id") or str(uuid.uuid4())
        request.state.request_id = rid
        structlog.contextvars.clear_contextvars()
        structlog.contextvars.bind_contextvars(request_id=rid)
        started = time.perf_counter()
        response = await call_next(request)
        route = request.scope.get("route")
        path = getattr(route, "path", "unmatched")
        REQUESTS.labels(request.method, path, str(response.status_code)).inc()
        LATENCY.labels(request.method, path).observe(time.perf_counter() - started)
        response.headers["x-request-id"] = rid
        return response

    for router in (
        health.router,
        algorithms.router,
        models.router,
        inference.router,
        unsupervised.router,
        graph.router,
    ):
        app.include_router(router)
    return app


app = create_app()
