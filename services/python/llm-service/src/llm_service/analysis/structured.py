"""Structured-output helper: Ollama ``format`` = JSON schema, then parse + jsonschema validation + one retry."""

from __future__ import annotations

import json
from typing import Any

import jsonschema

from llm_service.clients.ollama import Message, OllamaClient
from llm_service.metrics import LLM_CALLS


class StructuredOutputError(RuntimeError):
    pass


def _parse(content: str) -> Any:
    text = content.strip()
    if text.startswith("```"):
        text = text.strip("`")
        text = text[text.find("\n") + 1 :] if "\n" in text else text
    return json.loads(text)


async def structured_chat(
    ollama: OllamaClient,
    messages: list[Message],
    schema: dict[str, Any],
    *,
    model: str | None = None,
    temperature: float = 0.1,
    retries: int = 1,
) -> dict[str, Any]:
    convo = list(messages)
    last_error = ""
    for _ in range(retries + 1):
        msg = await ollama.chat(convo, format_schema=schema, model=model, temperature=temperature)
        content = str(msg.get("content", ""))
        try:
            data = _parse(content)
            jsonschema.validate(data, schema)
            LLM_CALLS.labels("structured", "ok").inc()
            return dict(data)
        except (json.JSONDecodeError, jsonschema.ValidationError) as exc:
            last_error = str(exc).split("\n", 1)[0][:500]
            LLM_CALLS.labels("structured", "invalid").inc()
            convo = [
                *convo,
                {"role": "assistant", "content": content},
                {
                    "role": "user",
                    "content": f"The JSON was invalid: {last_error}. Return only valid JSON that matches the schema.",
                },
            ]
    raise StructuredOutputError(f"model did not return valid JSON: {last_error}")
