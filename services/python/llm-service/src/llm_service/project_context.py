"""Project context for prompts (name, stage, business context, llm_config) with a short TTL cache."""

from __future__ import annotations

import time
import uuid
from dataclasses import dataclass
from typing import Any

from starlette.concurrency import run_in_threadpool

from llm_service import repository as repo
from llm_service.clients.platform import CallContext, PlatformClient
from llm_service.config import Settings
from llm_service.db import tenant_session
from llm_service.logging import get_logger

log = get_logger(__name__)


@dataclass(frozen=True)
class ProjectContext:
    project: dict[str, Any]
    model: str
    temperature: float
    language: str
    system_prompt_extra: str
    regulation_ids: list[uuid.UUID]

    def prompt_vars(self) -> dict[str, Any]:
        return {"project": self.project, "language": self.language, "system_prompt_extra": self.system_prompt_extra}


class ProjectContextLoader:
    def __init__(self, settings: Settings, platform: PlatformClient, ttl_s: float = 60.0) -> None:
        self._s = settings
        self._platform = platform
        self._ttl = ttl_s
        self._cache: dict[uuid.UUID, tuple[float, dict[str, Any]]] = {}

    async def _project(self, ctx: CallContext) -> dict[str, Any]:
        hit = self._cache.get(ctx.project_id)
        if hit and time.monotonic() - hit[0] < self._ttl:
            return hit[1]
        try:
            project = await self._platform.get_project(ctx)
        except Exception as exc:  # core-api down → degrade gracefully, prompts still work
            log.warning("project_context_unavailable", project_id=str(ctx.project_id), error=str(exc))
            project = {"id": str(ctx.project_id), "name": str(ctx.project_id), "stage": "custom"}
        self._cache[ctx.project_id] = (time.monotonic(), project)
        return project

    async def load(self, ctx: CallContext) -> ProjectContext:
        project = await self._project(ctx)
        llm_cfg = project.get("llm_config") or {}

        def _regs() -> list[uuid.UUID]:
            with tenant_session(ctx.tenant_id) as s:
                return repo.attached_regulation_ids(s, ctx.project_id)

        regulation_ids = await run_in_threadpool(_regs)
        return ProjectContext(
            project=project,
            model=llm_cfg.get("chat_model") or self._s.ollama_chat_model,
            temperature=float(llm_cfg.get("temperature", 0.1) or 0.1),
            language=str(llm_cfg.get("language") or "id"),
            system_prompt_extra=str(llm_cfg.get("system_prompt_extra") or ""),
            regulation_ids=regulation_ids,
        )
