"""Thin async client for the Ollama HTTP API (chat with native tool calling, structured output, embeddings).

Deliberately no LangChain: every request/response is explicit, which keeps prompts auditable and tests simple.
"""

from __future__ import annotations

import json
import re
from collections.abc import AsyncIterator
from typing import Any

import httpx

from llm_service.config import Settings

Message = dict[str, Any]


class OllamaError(RuntimeError):
    pass


_THINK_OPEN, _THINK_CLOSE = "<think>", "</think>"
_THINK_BLOCK = re.compile(r"<think>.*?(?:</think>|$)\s*", re.DOTALL)


def strip_thinking(text: str) -> str:
    """Remove ``<think>...</think>`` reasoning blocks that thinking models put in ``content`` on older Ollama."""
    return _THINK_BLOCK.sub("", text) if _THINK_OPEN in text else text


class _ThinkFilter:
    """Streaming counterpart of :func:`strip_thinking`; tags may be split across chunks."""

    def __init__(self) -> None:
        self._buf = ""
        self._inside = False
        self._after_close = False  # drop whitespace that follows </think>, even when it arrives in later chunks

    def feed(self, chunk: str) -> str:
        self._buf += chunk
        if self._after_close:
            self._buf = self._buf.lstrip()
            self._after_close = not self._buf
        out: list[str] = []
        while self._buf:
            tag = _THINK_CLOSE if self._inside else _THINK_OPEN
            idx = self._buf.find(tag)
            if idx >= 0:
                if not self._inside:
                    out.append(self._buf[:idx])
                self._buf = self._buf[idx + len(tag) :]
                if self._inside:
                    self._buf = self._buf.lstrip()
                    self._after_close = not self._buf
                self._inside = not self._inside
                continue
            # keep a possible partial tag at the end for the next chunk
            keep = next((k for k in range(len(tag) - 1, 0, -1) if self._buf.endswith(tag[:k])), 0)
            if not self._inside:
                out.append(self._buf[: len(self._buf) - keep])
            self._buf = self._buf[len(self._buf) - keep :] if keep else ""
            break
        return "".join(out)

    def flush(self) -> str:
        rest, self._buf = ("" if self._inside else self._buf), ""
        return rest


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

    def _body(self, messages: list[Message], model: str | None, temperature: float, *, stream: bool) -> dict[str, Any]:
        body: dict[str, Any] = {
            "model": model or self.chat_model,
            "messages": messages,
            "stream": stream,
            "options": self._options(temperature),
        }
        if self._settings.ollama_think is not None:
            body["think"] = self._settings.ollama_think
        return body

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
        body = self._body(messages, model, temperature, stream=False)
        if tools:
            body["tools"] = tools
        if format_schema is not None:
            body["format"] = format_schema
        try:
            resp = await self._http.post("/api/chat", json=body)
            resp.raise_for_status()
        except httpx.HTTPError as exc:
            raise OllamaError(f"ollama chat failed: {type(exc).__name__}: {exc}") from exc
        data = resp.json()
        message: Message = data.get("message") or {}
        if isinstance(message.get("content"), str):
            message["content"] = strip_thinking(message["content"])
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
        body = self._body(messages, model, temperature, stream=True)
        think = _ThinkFilter()
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
                    if message.get("content") and (text := think.feed(message["content"])):
                        yield {"content": text}
                    if message.get("tool_calls"):
                        yield {"tool_calls": message["tool_calls"]}
                    if chunk.get("done"):
                        break
                if rest := think.flush():
                    yield {"content": rest}
        except httpx.HTTPError as exc:
            raise OllamaError(f"ollama stream failed: {type(exc).__name__}: {exc}") from exc

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
                raise OllamaError(f"ollama embed failed: {type(exc).__name__}: {exc}") from exc
            out.extend(resp.json()["embeddings"])
        return out

    async def ping(self) -> bool:
        try:
            resp = await self._http.get("/api/tags", timeout=3.0)
            return resp.status_code == 200
        except httpx.HTTPError:
            return False
