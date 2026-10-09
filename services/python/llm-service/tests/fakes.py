"""Test doubles: a scripted Ollama and deterministic embeddings."""

from __future__ import annotations

import hashlib
import json
import math
from collections.abc import AsyncIterator
from typing import Any


def hash_embedding(text: str, dim: int = 32) -> list[float]:
    """Deterministic bag-of-words embedding: similar texts share buckets → cosine similarity works."""
    vec = [0.0] * dim
    for word in text.lower().split():
        h = int(hashlib.sha256(word.encode()).hexdigest(), 16)
        vec[h % dim] += 1.0
    norm = math.sqrt(sum(v * v for v in vec)) or 1.0
    return [v / norm for v in vec]


class FakeOllama:
    """Returns scripted assistant messages in order; records every request."""

    chat_model = "fake-model"

    def __init__(self, script: list[dict[str, Any]] | None = None) -> None:
        self.script = list(script or [])
        self.requests: list[dict[str, Any]] = []

    def push(self, *messages: dict[str, Any]) -> None:
        self.script.extend(messages)

    async def chat(
        self,
        messages: list[dict[str, Any]],
        *,
        tools: Any = None,
        format_schema: Any = None,
        model: str | None = None,
        temperature: float = 0.1,
    ) -> dict[str, Any]:
        self.requests.append({"messages": messages, "tools": tools, "format": format_schema})
        if not self.script:
            return {"role": "assistant", "content": "(no more script)"}
        msg = self.script.pop(0)
        if "json" in msg:
            return {"role": "assistant", "content": json.dumps(msg["json"])}
        return {"role": "assistant", **msg}

    async def chat_stream(
        self, messages: list[dict[str, Any]], *, tools: Any = None, model: str | None = None, temperature: float = 0.1
    ) -> AsyncIterator[dict[str, Any]]:
        msg = await self.chat(messages, tools=tools, model=model, temperature=temperature)
        for word in str(msg.get("content") or "").split(" "):
            if word:
                yield {"content": word + " "}
        if msg.get("tool_calls"):
            yield {"tool_calls": msg["tool_calls"]}

    async def embed(self, texts: list[str], *, model: str | None = None) -> list[list[float]]:
        return [hash_embedding(t) for t in texts]

    async def ping(self) -> bool:
        return True

    async def aclose(self) -> None:
        return None


def tool_call(name: str, **arguments: Any) -> dict[str, Any]:
    return {"role": "assistant", "content": "", "tool_calls": [{"function": {"name": name, "arguments": arguments}}]}
