"""Shared API helpers: pagination envelope, call-context construction."""

from __future__ import annotations

from typing import Any

from fastapi import Query
from pydantic import BaseModel

from llm_service.auth import ProjectScope
from llm_service.clients.platform import CallContext


class PageParams(BaseModel):
    page: int
    page_size: int

    @property
    def offset(self) -> int:
        return (self.page - 1) * self.page_size


def page_params(page: int = Query(1, ge=1), page_size: int = Query(50, ge=1, le=200)) -> PageParams:
    return PageParams(page=page, page_size=page_size)


def paged(items: list[Any], total: int, p: PageParams) -> dict[str, Any]:
    return {"items": items, "total": total, "page": p.page, "page_size": p.page_size}


def call_ctx(scope: ProjectScope, request_id: str | None = None) -> CallContext:
    return CallContext(
        tenant_id=scope.tenant_id, project_id=scope.project_id, actor_id=scope.principal.user_id, request_id=request_id
    )
