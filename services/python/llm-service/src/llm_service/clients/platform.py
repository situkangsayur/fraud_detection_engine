"""HTTP client for sibling services (core-api, rule-service, graph-service, ml-service).

All calls use the internal token plus tenant/project/actor headers, so the callee applies the same tenant
isolation and audit attribution as for a user call.
"""

from __future__ import annotations

import uuid
from dataclasses import dataclass
from datetime import UTC, datetime, timedelta
from typing import Any

import httpx
import structlog

from llm_service.config import Settings


class PlatformError(RuntimeError):
    def __init__(self, service: str, status: int | None, detail: str) -> None:
        super().__init__(f"{service}: {status} {detail}")
        self.service = service
        self.status = status
        self.detail = detail


@dataclass(frozen=True)
class CallContext:
    tenant_id: uuid.UUID
    project_id: uuid.UUID
    actor_id: uuid.UUID | None = None
    request_id: str | None = None


class PlatformClient:
    def __init__(self, settings: Settings, http: httpx.AsyncClient | None = None) -> None:
        self._s = settings
        self._http = http or httpx.AsyncClient(timeout=httpx.Timeout(settings.tool_timeout_s, connect=3.0))
        self._bases = {
            "core": settings.core_api_url.rstrip("/"),
            "rule": settings.rule_service_url.rstrip("/"),
            "graph": settings.graph_service_url.rstrip("/"),
            "ml": settings.ml_service_url.rstrip("/"),
        }

    async def aclose(self) -> None:
        await self._http.aclose()

    def _headers(self, ctx: CallContext) -> dict[str, str]:
        h = {
            "Authorization": f"Bearer {self._s.internal_api_token}",
            "X-Tenant-Id": str(ctx.tenant_id),
            "X-Project-Id": str(ctx.project_id),
        }
        if ctx.actor_id:
            h["X-Actor"] = str(ctx.actor_id)
        rid = ctx.request_id or structlog.contextvars.get_contextvars().get("request_id")
        if rid:
            h["X-Request-Id"] = str(rid)
        return h

    async def request(
        self,
        service: str,
        method: str,
        path: str,
        ctx: CallContext,
        *,
        params: dict[str, Any] | None = None,
        json: Any = None,
        accept_statuses: tuple[int, ...] = (),
    ) -> Any:
        url = self._bases[service] + path
        try:
            resp = await self._http.request(method, url, headers=self._headers(ctx), params=params, json=json)
        except httpx.HTTPError as exc:
            raise PlatformError(service, None, str(exc)) from exc
        if resp.status_code >= 400 and resp.status_code not in accept_statuses:
            raise PlatformError(service, resp.status_code, resp.text[:500])
        if not resp.content:
            return None
        return resp.json()

    # ---------------------------------------------------------------- core-api
    def _p(self, ctx: CallContext) -> str:
        return f"/api/v1/projects/{ctx.project_id}"

    async def get_project(self, ctx: CallContext) -> dict[str, Any]:
        return await self.request("core", "GET", self._p(ctx), ctx)  # type: ignore[no-any-return]

    async def analytics_overview(self, ctx: CallContext, since_days: int) -> Any:
        now = datetime.now(UTC)
        return await self.request(
            "core",
            "GET",
            self._p(ctx) + "/analytics/overview",
            ctx,
            params={"from": (now - timedelta(days=since_days)).isoformat(), "to": now.isoformat()},
        )

    async def feature_drift(self, ctx: CallContext) -> Any:
        return await self.request("core", "GET", self._p(ctx) + "/analytics/drift", ctx)

    async def typology_stats(self, ctx: CallContext) -> Any:
        return await self.request("core", "GET", self._p(ctx) + "/analytics/typologies", ctx)

    async def field_catalog(self, ctx: CallContext) -> Any:
        return await self.request("core", "GET", f"/v1/internal/projects/{ctx.project_id}/field-catalog", ctx)

    # ---------------------------------------------------------------- rule-service
    async def list_rules(self, ctx: CallContext, params: dict[str, Any] | None = None) -> Any:
        return await self.request("rule", "GET", self._p(ctx) + "/rules", ctx, params=params)

    async def get_rule(self, ctx: CallContext, rule_id: str) -> Any:
        return await self.request("rule", "GET", self._p(ctx) + f"/rules/{rule_id}", ctx)

    async def rules_performance(self, ctx: CallContext, since_days: int) -> Any:
        return await self.request(
            "rule", "GET", self._p(ctx) + "/rules/performance", ctx, params={"since_days": since_days}
        )

    async def validate_rule(self, ctx: CallContext, envelope: dict[str, Any]) -> dict[str, Any]:
        return await self.request(  # type: ignore[no-any-return]
            "rule", "POST", self._p(ctx) + "/rules/validate", ctx, json=envelope, accept_statuses=(422,)
        )

    async def backtest_rule(self, ctx: CallContext, envelope: dict[str, Any], since_days: int = 30) -> Any:
        return await self.request(
            "rule", "POST", self._p(ctx) + "/rules/backtest", ctx, json={"rule": envelope, "since_days": since_days}
        )

    async def create_proposal(self, ctx: CallContext, proposal: dict[str, Any]) -> dict[str, Any]:
        return await self.request(  # type: ignore[no-any-return]
            "rule", "POST", self._p(ctx) + "/proposals", ctx, json=proposal
        )

    # ---------------------------------------------------------------- graph / ml
    async def graph_components(self, ctx: CallContext, min_size: int = 3) -> Any:
        return await self.request(
            "graph",
            "GET",
            self._p(ctx) + "/graph/components",
            ctx,
            params={"min_size": min_size, "only_with_fraud": "false"},
        )

    async def anomaly_clusters(self, ctx: CallContext) -> Any:
        return await self.request("ml", "GET", self._p(ctx) + "/ml/unsupervised/clusters", ctx, accept_statuses=(404,))

    async def graph_communities(self, ctx: CallContext, min_size: int = 3) -> Any:
        return await self.request(
            "ml",
            "GET",
            self._p(ctx) + "/ml/graph-communities",
            ctx,
            params={"min_size": min_size},
            accept_statuses=(404,),
        )

    async def ping(self, service: str) -> bool:
        try:
            resp = await self._http.get(self._bases[service] + "/health/live", timeout=2.0)
            return resp.status_code == 200
        except httpx.HTTPError:
            return False
