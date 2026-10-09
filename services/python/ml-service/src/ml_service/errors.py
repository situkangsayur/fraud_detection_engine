"""RFC 7807 problem+json errors and FastAPI exception handlers."""

from __future__ import annotations

from typing import Any

from fastapi import FastAPI, Request
from fastapi.exceptions import RequestValidationError
from fastapi.responses import JSONResponse
from starlette.exceptions import HTTPException as StarletteHTTPException

from ml_service.logging import get_logger

PROBLEM_JSON = "application/problem+json"
log = get_logger(__name__)


class ProblemError(Exception):
    """Raise anywhere to return an RFC 7807 response."""

    def __init__(
        self,
        status: int,
        title: str,
        detail: str | None = None,
        type_: str = "about:blank",
        errors: list[dict[str, Any]] | None = None,
    ) -> None:
        super().__init__(detail or title)
        self.status = status
        self.title = title
        self.detail = detail
        self.type = type_
        self.errors = errors


def not_found(what: str) -> ProblemError:
    return ProblemError(404, "Not Found", f"{what} not found", type_="not_found")


def conflict(detail: str, type_: str = "conflict") -> ProblemError:
    return ProblemError(409, "Conflict", detail, type_=type_)


def problem_response(
    status: int,
    title: str,
    detail: str | None = None,
    type_: str = "about:blank",
    errors: list[dict[str, Any]] | None = None,
) -> JSONResponse:
    body: dict[str, Any] = {"type": type_, "title": title, "status": status}
    if detail:
        body["detail"] = detail
    if errors:
        body["errors"] = errors
    return JSONResponse(body, status_code=status, media_type=PROBLEM_JSON)


def install_error_handlers(app: FastAPI) -> None:
    @app.exception_handler(ProblemError)
    async def _problem(_: Request, exc: ProblemError) -> JSONResponse:
        return problem_response(exc.status, exc.title, exc.detail, exc.type, exc.errors)

    @app.exception_handler(RequestValidationError)
    async def _validation(_: Request, exc: RequestValidationError) -> JSONResponse:
        errors = [
            {
                "field": ".".join(str(p) for p in err.get("loc", []) if p != "body"),
                "message": err.get("msg", ""),
            }
            for err in exc.errors()
        ]
        return problem_response(
            422, "Unprocessable Entity", "request validation failed", "validation_error", errors
        )

    @app.exception_handler(StarletteHTTPException)
    async def _http(_: Request, exc: StarletteHTTPException) -> JSONResponse:
        return problem_response(exc.status_code, str(exc.detail), None)

    @app.exception_handler(Exception)
    async def _unhandled(request: Request, exc: Exception) -> JSONResponse:
        log.exception("unhandled_error", path=request.url.path, error=str(exc))
        return problem_response(500, "Internal Server Error", "unexpected error", "internal_error")
