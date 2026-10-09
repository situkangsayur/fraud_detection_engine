"""Shared endpoint helpers."""

from __future__ import annotations

from typing import Any

from fastapi import Query, Request

from ml_service.auth import Principal


def request_id(request: Request) -> str | None:
    return getattr(request.state, "request_id", None)


def actor_type(principal: Principal) -> str:
    return "service" if principal.is_internal and principal.actor_id is None else "user"


class Page:
    def __init__(self, page: int = Query(1, ge=1), page_size: int = Query(50, ge=1, le=200)) -> None:
        self.page = page
        self.page_size = page_size

    def wrap(self, items: list[Any], total: int) -> dict[str, Any]:
        return {"items": items, "total": total, "page": self.page, "page_size": self.page_size}
