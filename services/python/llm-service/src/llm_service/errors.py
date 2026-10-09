"""RFC 7807 problem+json errors."""

from __future__ import annotations

from typing import Any

from fastapi import FastAPI, Request
from fastapi.exceptions import RequestValidationError
from fastapi.responses import JSONResponse
from starlette.exceptions import HTTPException as StarletteHTTPException

PROBLEM_JSON = "application/problem+json"


class ProblemError(Exception):
    """Domain error rendered as problem+json."""

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
    return ProblemError(404, "Not Found", f"{what} not found")


def _problem(
    status: int, title: str, detail: str | None, type_: str = "about:blank", errors: list[dict[str, Any]] | None = None
) -> JSONResponse:
    body: dict[str, Any] = {"type": type_, "title": title, "status": status}
    if detail:
        body["detail"] = detail
    if errors:
        body["errors"] = errors
    return JSONResponse(body, status_code=status, media_type=PROBLEM_JSON)


def install_error_handlers(app: FastAPI) -> None:
    @app.exception_handler(ProblemError)
    async def _problem_handler(_: Request, exc: ProblemError) -> JSONResponse:
        return _problem(exc.status, exc.title, exc.detail, exc.type, exc.errors)

    @app.exception_handler(StarletteHTTPException)
    async def _http_handler(_: Request, exc: StarletteHTTPException) -> JSONResponse:
        return _problem(exc.status_code, str(exc.detail) if exc.status_code < 500 else "Error", None)

    @app.exception_handler(RequestValidationError)
    async def _validation_handler(_: Request, exc: RequestValidationError) -> JSONResponse:
        errors = [
            {"field": ".".join(str(p) for p in e.get("loc", ())), "message": e.get("msg", "")} for e in exc.errors()
        ]
        return _problem(422, "Validation failed", None, errors=errors)
