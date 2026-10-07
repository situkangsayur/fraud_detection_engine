"""Gemini chat client (OpenAI-compatible endpoint) with the same interface as :class:`OllamaClient`.

Only chat moves to Gemini: embeddings stay on Ollama (bge-m3), so existing regulation indexes keep working.
Messages, tool calls and results are converted between Ollama's shape (used by the agent loop) and OpenAI's.
"""

from __future__ import annotations

import json
from collections.abc import AsyncIterator
from typing import Any

import httpx

from llm_service.clients.ollama import Message, OllamaClient, OllamaError, strip_thinking
from llm_service.config import Settings


def to_openai_messages(messages: list[Message]) -> list[dict[str, Any]]:
    """Ollama → OpenAI: tool calls get ids and JSON-string arguments; tool results reference those ids in order."""
    out: list[dict[str, Any]] = []
    pending: list[str] = []
    for i, m in enumerate(messages):
        role = m.get("role")
        if role == "assistant" and m.get("tool_calls"):
            calls: list[dict[str, Any]] = []
            ids: list[str] = []
            for j, call in enumerate(m["tool_calls"]):
                fn = call.get("function") or {}
                args = fn.get("arguments")
                calls.append(
                    {
                        "id": call.get("id") or f"call_{i}_{j}",
                        "type": "function",
                        "function": {
                            "name": fn.get("name", ""),
                            "arguments": args if isinstance(args, str) else json.dumps(args or {}),
                        },
                    }
                )
                ids.append(calls[-1]["id"])
            pending = ids
            out.append({"role": "assistant", "content": m.get("content") or None, "tool_calls": calls})
        elif role == "tool":
            out.append(
                {
                    "role": "tool",
                    "tool_call_id": pending.pop(0) if pending else m.get("tool_name", "tool"),
                    "content": m.get("content", ""),
                }
            )
        else:
            out.append({"role": role, "content": m.get("content", "")})
    return out


def _from_openai_calls(calls: list[dict[str, Any]] | None) -> list[dict[str, Any]]:
    out = []
    for call in calls or []:
        fn = call.get("function") or {}
        raw = fn.get("arguments") or "{}"
        try:
            args = json.loads(raw) if isinstance(raw, str) else raw
        except json.JSONDecodeError:
            args = raw  # the agent loop reports unparsable arguments back to the model
        out.append({"id": call.get("id"), "function": {"name": fn.get("name", ""), "arguments": args}})
    return out


class GeminiClient(OllamaClient):
    def __init__(
        self,
        settings: Settings,
        http: httpx.AsyncClient | None = None,
        embed_http: httpx.AsyncClient | None = None,
    ) -> None:
        super().__init__(settings, embed_http)  # embeddings → Ollama
        if not settings.gemini_api_key:
            raise ValueError("LLM_PROVIDER=gemini needs GEMINI_API_KEY")
        self._chat_http = http or httpx.AsyncClient(
            base_url=settings.gemini_base_url.rstrip("/") + "/",
            headers={"authorization": f"Bearer {settings.gemini_api_key}"},
            timeout=httpx.Timeout(settings.ollama_timeout_s, connect=10.0),
        )

    @property
    def chat_model(self) -> str:
        return self._settings.gemini_model

    def _model(self, model: str | None) -> str:
        # Projects may still name an Ollama model (e.g. "qwen3:8b"); only Gemini names are honoured here.
        return model if model and model.startswith("gemini") else self.chat_model

    async def aclose(self) -> None:
        await self._chat_http.aclose()
        await super().aclose()

    def _payload(
        self,
        messages: list[Message],
        tools: list[dict[str, Any]] | None,
        model: str | None,
        temperature: float,
        *,
        stream: bool,
    ) -> dict[str, Any]:
        body: dict[str, Any] = {
            "model": self._model(model),
            "messages": to_openai_messages(messages),
            "temperature": temperature,
            "stream": stream,
        }
        if tools:
            body["tools"] = tools
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
        body = self._payload(messages, tools, model, temperature, stream=False)
        if format_schema is not None:
            body["response_format"] = {
                "type": "json_schema",
                "json_schema": {"name": "output", "schema": format_schema},
            }
        try:
            resp = await self._chat_http.post("chat/completions", json=body)
            resp.raise_for_status()
        except httpx.HTTPStatusError as exc:
            raise OllamaError(f"gemini chat failed: {exc.response.status_code} {exc.response.text[:300]}") from exc
        except httpx.HTTPError as exc:
            raise OllamaError(f"gemini chat failed: {exc}") from exc
        choice = (resp.json().get("choices") or [{}])[0]
        msg = choice.get("message") or {}
        out: Message = {"role": "assistant", "content": strip_thinking(msg.get("content") or "")}
        if calls := _from_openai_calls(msg.get("tool_calls")):
            out["tool_calls"] = calls
        return out

    async def chat_stream(
        self,
        messages: list[Message],
        *,
        tools: list[dict[str, Any]] | None = None,
        model: str | None = None,
        temperature: float = 0.1,
    ) -> AsyncIterator[dict[str, Any]]:
        body = self._payload(messages, tools, model, temperature, stream=True)
        calls: dict[int, dict[str, Any]] = {}  # tool-call deltas arrive in fragments, keyed by index
        try:
            async with self._chat_http.stream("POST", "chat/completions", json=body) as resp:
                if resp.status_code >= 400:
                    detail = (await resp.aread()).decode(errors="replace")[:300]
                    raise OllamaError(f"gemini stream failed: {resp.status_code} {detail}")
                async for line in resp.aiter_lines():
                    if not line.startswith("data:"):
                        continue
                    data = line[5:].strip()
                    if data == "[DONE]":
                        break
                    delta = ((json.loads(data).get("choices") or [{}])[0]).get("delta") or {}
                    if delta.get("content"):
                        yield {"content": delta["content"]}
                    for k, tc in enumerate(delta.get("tool_calls") or []):
                        slot = calls.setdefault(
                            tc.get("index", k), {"id": None, "function": {"name": "", "arguments": ""}}
                        )
                        slot["id"] = tc.get("id") or slot["id"]
                        fn = tc.get("function") or {}
                        slot["function"]["name"] += fn.get("name") or ""
                        args = fn.get("arguments")
                        slot["function"]["arguments"] += args if isinstance(args, str) else json.dumps(args or {})
        except httpx.HTTPError as exc:
            raise OllamaError(f"gemini stream failed: {exc}") from exc
        if calls:
            yield {"tool_calls": _from_openai_calls([calls[i] for i in sorted(calls)])}

    async def ping(self) -> bool:
        try:
            resp = await self._chat_http.get("models", timeout=5.0)
            return resp.status_code == 200 and await super().ping()
        except httpx.HTTPError:
            return False
