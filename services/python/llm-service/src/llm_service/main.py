"""FastAPI application factory for llm-service."""

from __future__ import annotations

import time
import uuid
from collections.abc import AsyncIterator, Awaitable, Callable
from contextlib import asynccontextmanager

import structlog
from fastapi import FastAPI, Request, Response

from llm_service.api import assistant, health, internal, regulations
from llm_service.config import Settings, get_settings
from llm_service.container import Container
from llm_service.errors import install_error_handlers
from llm_service.logging import configure_logging, get_logger
from llm_service.metrics import HTTP_LATENCY, HTTP_REQUESTS

log = get_logger(__name__)


def create_app(settings: Settings | None = None, container: Container | None = None) -> FastAPI:
    settings = settings or get_settings()
    configure_logging(settings.log_level)

    @asynccontextmanager
    async def lifespan(app: FastAPI) -> AsyncIterator[None]:
        app.state.container = container or Container.build(settings)
        log.info(
            "llm_service_started",
            provider=settings.llm_provider,
            chat_model=settings.chat_model,
            embed_model=settings.ollama_embed_model,
        )
        yield
        await app.state.container.aclose()

    app = FastAPI(
        title="llm-service",
        version="0.1.0",
        lifespan=lifespan,
        description="Regulation/policy RAG, analyst chat with tools, rule analyses and proposals. "
        "The LLM never activates anything: it only creates pending proposals.",
    )
    install_error_handlers(app)

    @app.middleware("http")
    async def request_context(request: Request, call_next: Callable[[Request], Awaitable[Response]]) -> Response:
        rid = request.headers.get("x-request-id") or str(uuid.uuid4())
        structlog.contextvars.clear_contextvars()
        structlog.contextvars.bind_contextvars(request_id=rid)
        start = time.perf_counter()
        response = await call_next(request)
        route = request.scope.get("route")
        path = getattr(route, "path", "unmatched")
        HTTP_REQUESTS.labels(request.method, path, str(response.status_code)).inc()
        HTTP_LATENCY.labels(request.method, path).observe(time.perf_counter() - start)
        response.headers["x-request-id"] = rid
        return response

    app.include_router(health.router)
    app.include_router(regulations.router)
    app.include_router(assistant.router)
    app.include_router(internal.router)
    return app


app = create_app()
