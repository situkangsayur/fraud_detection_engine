"""Project-scoped chat with conversation memory, tools and regulation citations."""

from __future__ import annotations

import json
import uuid
from collections.abc import AsyncIterator
from typing import Any

from starlette.concurrency import run_in_threadpool

from llm_service import prompts
from llm_service import repository as repo
from llm_service.agent.loop import run_agent
from llm_service.agent.tools import ToolContext
from llm_service.analysis.proposals import ProposalPipeline
from llm_service.clients.ollama import Message, OllamaClient
from llm_service.clients.platform import CallContext, PlatformClient
from llm_service.config import Settings
from llm_service.db import tenant_session
from llm_service.errors import not_found
from llm_service.project_context import ProjectContextLoader
from llm_service.retrieval.search import RegulationSearch

HISTORY_MESSAGES = 20


class ChatService:
    def __init__(
        self,
        settings: Settings,
        ollama: OllamaClient,
        platform: PlatformClient,
        search: RegulationSearch,
        proposals: ProposalPipeline,
        contexts: ProjectContextLoader,
    ) -> None:
        self._s = settings
        self._ollama = ollama
        self._platform = platform
        self._search = search
        self._proposals = proposals
        self._contexts = contexts

    def _open_conversation(
        self, ctx: CallContext, conversation_id: uuid.UUID | None, first_message: str
    ) -> tuple[uuid.UUID, list[Message]]:
        with tenant_session(ctx.tenant_id) as s:
            if conversation_id is None:
                created = repo.create_conversation(
                    s,
                    tenant_id=ctx.tenant_id,
                    project_id=ctx.project_id,
                    user_id=ctx.actor_id,
                    title=first_message[:80],
                )
                return created["id"], []
            conv = repo.get_conversation(s, ctx.project_id, conversation_id)
            if conv is None or (conv["user_id"] is not None and conv["user_id"] != ctx.actor_id):
                raise not_found("conversation")
            history = [
                {"role": m["role"], "content": m["content"]}
                for m in repo.list_messages(s, conversation_id)
                if m["role"] in ("user", "assistant")
            ]
            return conversation_id, history[-HISTORY_MESSAGES:]

    def _persist(self, ctx: CallContext, conversation_id: uuid.UUID, user_message: str, final: dict[str, Any]) -> None:
        with tenant_session(ctx.tenant_id) as s:
            repo.add_message(
                s, tenant_id=ctx.tenant_id, conversation_id=conversation_id, role="user", content=user_message
            )
            if final.get("tool_calls"):
                repo.add_message(
                    s,
                    tenant_id=ctx.tenant_id,
                    conversation_id=conversation_id,
                    role="tool",
                    content=json.dumps([t["name"] for t in final["tool_calls"]]),
                    tool_calls=final["tool_calls"],
                )
            repo.add_message(
                s,
                tenant_id=ctx.tenant_id,
                conversation_id=conversation_id,
                role="assistant",
                content=final.get("answer") or "",
                citations=final.get("citations") or [],
            )

    async def _prepare(
        self, ctx: CallContext, conversation_id: uuid.UUID | None, message: str
    ) -> tuple[uuid.UUID, list[Message], ToolContext, float]:
        pctx = await self._contexts.load(ctx)
        conv_id, history = await run_in_threadpool(self._open_conversation, ctx, conversation_id, message)
        system = prompts.render("chat.md.j2", **pctx.prompt_vars())
        messages: list[Message] = [
            {"role": "system", "content": system},
            *history,
            {"role": "user", "content": message},
        ]
        tool_ctx = ToolContext(
            call=ctx,
            platform=self._platform,
            search=self._search,
            proposals=self._proposals,
            regulation_ids=pctx.regulation_ids,
            model=pctx.model,
            project=pctx.project,
        )
        return conv_id, messages, tool_ctx, pctx.temperature

    async def chat(self, ctx: CallContext, conversation_id: uuid.UUID | None, message: str) -> dict[str, Any]:
        conv_id, messages, tool_ctx, temperature = await self._prepare(ctx, conversation_id, message)
        final: dict[str, Any] = {}
        async for ev in run_agent(
            self._ollama,
            messages,
            tool_ctx,
            max_iterations=self._s.chat_max_tool_iterations,
            tool_timeout=self._s.tool_timeout_s,
            temperature=temperature,
        ):
            if ev["type"] == "done":
                final = ev
        await run_in_threadpool(self._persist, ctx, conv_id, message, final)
        return {
            "conversation_id": str(conv_id),
            "answer": final.get("answer", ""),
            "tool_calls": final.get("tool_calls", []),
            "citations": final.get("citations", []),
            "proposals": final.get("proposals", []),
        }

    async def chat_stream(
        self, ctx: CallContext, conversation_id: uuid.UUID | None, message: str
    ) -> AsyncIterator[dict[str, Any]]:
        conv_id, messages, tool_ctx, temperature = await self._prepare(ctx, conversation_id, message)
        yield {"type": "conversation", "conversation_id": str(conv_id)}
        seen_citations: set[str] = set()
        async for ev in run_agent(
            self._ollama,
            messages,
            tool_ctx,
            max_iterations=self._s.chat_max_tool_iterations,
            tool_timeout=self._s.tool_timeout_s,
            stream=True,
            temperature=temperature,
        ):
            if ev["type"] == "tool":
                yield ev
                for c in tool_ctx.citations:
                    if c["chunk_id"] not in seen_citations:
                        seen_citations.add(c["chunk_id"])
                        yield {"type": "citation", **c}
            elif ev["type"] == "done":
                await run_in_threadpool(self._persist, ctx, conv_id, message, ev)
                yield {
                    "type": "done",
                    "conversation_id": str(conv_id),
                    "answer": ev["answer"],
                    "tool_calls": ev["tool_calls"],
                    "citations": ev["citations"],
                    "proposals": ev["proposals"],
                }
            else:
                yield ev

    def list_conversations(self, ctx: CallContext, limit: int, offset: int) -> tuple[list[dict[str, Any]], int]:
        with tenant_session(ctx.tenant_id) as s:
            return repo.list_conversations(s, ctx.project_id, ctx.actor_id, limit=limit, offset=offset)

    def get_conversation(self, ctx: CallContext, conversation_id: uuid.UUID) -> dict[str, Any]:
        with tenant_session(ctx.tenant_id) as s:
            conv = repo.get_conversation(s, ctx.project_id, conversation_id)
            if conv is None or (conv["user_id"] is not None and conv["user_id"] != ctx.actor_id):
                raise not_found("conversation")
            return {**conv, "messages": repo.list_messages(s, conversation_id)}
