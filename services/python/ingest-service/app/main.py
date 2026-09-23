"""ingest-service entrypoint (FastAPI app factory)."""

from __future__ import annotations

import asyncio
import time
import uuid
from collections.abc import AsyncIterator, Awaitable, Callable
from contextlib import asynccontextmanager

import structlog
from fastapi import FastAPI, Request, Response
from fastapi.responses import JSONResponse
from prometheus_client import CONTENT_TYPE_LATEST, generate_latest

from app.api.routes import router
from app.config import get_settings
from app.connectors.scheduler import cleanup_loop, connector_loop
from app.db import ping
from app.errors import install_error_handlers, problem
from app.jobs.runner import JobRunner
from app.logging import configure_logging, log
from app.metrics import HTTP_LATENCY


def create_app(start_background: bool = True) -> FastAPI:
    settings = get_settings()
    configure_logging(settings.log_level)

    @asynccontextmanager
    async def lifespan(app: FastAPI) -> AsyncIterator[None]:
        app.state.runner = JobRunner(settings)
        stop = asyncio.Event()
        tasks: list[asyncio.Task[None]] = []
        if start_background:
            settings.upload_dir.mkdir(parents=True, exist_ok=True)
            tasks.append(asyncio.create_task(cleanup_loop(settings, stop)))
            if settings.connector_poll_enabled:
                tasks.append(asyncio.create_task(connector_loop(settings, app.state.runner.client, stop)))
        log.info("ingest_service_started")
        yield
        stop.set()
        for t in tasks:
            t.cancel()
        app.state.runner.shutdown()

    app = FastAPI(
        title="ingest-service",
        version="0.1.0",
        lifespan=lifespan,
        description="Schema inference, mapping suggestion, file/SQL import jobs, pull connectors",
    )
    install_error_handlers(app)

    @app.middleware("http")
    async def request_context(
        request: Request, call_next: Callable[[Request], Awaitable[Response]]
    ) -> Response:
        rid = request.headers.get("x-request-id") or str(uuid.uuid4())
        structlog.contextvars.bind_contextvars(request_id=rid)
        start = time.perf_counter()
        try:
            response = await call_next(request)
        except Exception:
            log.exception("unhandled_error", path=request.url.path)
            response = problem(500, "Internal Server Error")
        route = getattr(request.scope.get("route"), "path", request.url.path)
        HTTP_LATENCY.labels(request.method, route, str(response.status_code)).observe(
            time.perf_counter() - start
        )
        response.headers["x-request-id"] = rid
        structlog.contextvars.clear_contextvars()
        return response

    @app.get("/health/live", include_in_schema=False)
    def live() -> dict[str, str]:
        return {"status": "ok"}

    @app.get("/health/ready", include_in_schema=False)
    def ready() -> Response:
        ok = ping()
        return JSONResponse({"status": "ok" if ok else "degraded", "db": ok}, status_code=200 if ok else 503)

    @app.get("/metrics", include_in_schema=False)
    def metrics() -> Response:
        return Response(generate_latest(), media_type=CONTENT_TYPE_LATEST)

    app.include_router(router)
    return app


app = create_app()
