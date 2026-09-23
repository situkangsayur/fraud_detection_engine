"""Thin async client for the Ollama HTTP API (chat with native tool calling, structured output, embeddings).

Deliberately no LangChain: every request/response is explicit, which keeps prompts auditable and tests simple.
"""

from __future__ import annotations

import json
from collections.abc import AsyncIterator
from typing import Any

import httpx

from llm_service.config import Settings

Message = dict[str, Any]


class OllamaError(RuntimeError):
    pass


class OllamaClient:
    def __init__(self, settings: Settings, http: httpx.AsyncClient | None = None) -> None:
        self._settings = settings
        self._http = http or httpx.AsyncClient(
            base_url=settings.ollama_url, timeout=httpx.Timeout(settings.ollama_timeout_s, connect=5.0)
        )

    @property
    def chat_model(self) -> str:
        return self._settings.ollama_chat_model

    async def aclose(self) -> None:
        await self._http.aclose()

    def _options(self, temperature: float) -> dict[str, Any]:
        return {"temperature": temperature, "num_ctx": self._settings.ollama_num_ctx}

    async def chat(
        self,
        messages: list[Message],
        *,
        tools: list[dict[str, Any]] | None = None,
        format_schema: dict[str, Any] | None = None,
        model: str | None = None,
        temperature: float = 0.1,
    ) -> Message:
        """Single non-streaming chat turn. Returns the assistant message (may contain ``tool_calls``)."""
        body: dict[str, Any] = {
            "model": model or self.chat_model,
            "messages": messages,
            "stream": False,
            "options": self._options(temperature),
        }
        if tools:
            body["tools"] = tools
        if format_schema is not None:
            body["format"] = format_schema
        try:
            resp = await self._http.post("/api/chat", json=body)
            resp.raise_for_status()
        except httpx.HTTPError as exc:
            raise OllamaError(f"ollama chat failed: {exc}") from exc
        data = resp.json()
        message: Message = data.get("message") or {}
        return message

    async def chat_stream(
        self,
        messages: list[Message],
        *,
        tools: list[dict[str, Any]] | None = None,
        model: str | None = None,
        temperature: float = 0.1,
    ) -> AsyncIterator[dict[str, Any]]:
        """Stream a chat turn. Yields ``{"content": token}`` and/or ``{"tool_calls": [...]}`` items."""
        body: dict[str, Any] = {
            "model": model or self.chat_model,
            "messages": messages,
            "stream": True,
            "options": self._options(temperature),
        }
        if tools:
            body["tools"] = tools
        try:
            async with self._http.stream("POST", "/api/chat", json=body) as resp:
                resp.raise_for_status()
                async for line in resp.aiter_lines():
                    if not line.strip():
                        continue
                    chunk = json.loads(line)
                    message = chunk.get("message") or {}
                    if message.get("content"):
                        yield {"content": message["content"]}
                    if message.get("tool_calls"):
                        yield {"tool_calls": message["tool_calls"]}
                    if chunk.get("done"):
                        break
        except httpx.HTTPError as exc:
            raise OllamaError(f"ollama stream failed: {exc}") from exc

    async def embed(self, texts: list[str], *, model: str | None = None) -> list[list[float]]:
        if not texts:
            return []
        out: list[list[float]] = []
        batch = max(1, self._settings.embed_batch_size)
        for i in range(0, len(texts), batch):
            try:
                resp = await self._http.post(
                    "/api/embed",
                    json={"model": model or self._settings.ollama_embed_model, "input": texts[i : i + batch]},
                )
                resp.raise_for_status()
            except httpx.HTTPError as exc:
                raise OllamaError(f"ollama embed failed: {exc}") from exc
            out.extend(resp.json()["embeddings"])
        return out

    async def ping(self) -> bool:
        try:
            resp = await self._http.get("/api/tags", timeout=3.0)
            return resp.status_code == 200
        except httpx.HTTPError:
            return False
