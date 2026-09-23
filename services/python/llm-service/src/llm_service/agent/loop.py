"""Tool-calling agent loop (explicit, bounded, observable).

    messages ─▶ model ─▶ tool_calls? ─yes─▶ run allow-listed tools (timeout each) ─▶ append results ─┐
                   ▲                                                                                 │
                   └─────────────────────────────── until no tool_calls or max iterations ◀──────────┘

Emits events so the same loop serves the JSON endpoint and the SSE stream:
``{"type": "token"}``, ``{"type": "tool"}``, ``{"type": "done"}``.
"""

from __future__ import annotations

import asyncio
import json
from collections.abc import AsyncIterator
from typing import Any

from llm_service.agent.tools import TOOLS, ToolContext, tool_specs, wrap_result
from llm_service.clients.ollama import Message, OllamaClient
from llm_service.logging import get_logger
from llm_service.metrics import TOOL_CALLS

log = get_logger(__name__)

FINALISE_INSTRUCTION = (
    "Tool budget exhausted. Answer the analyst now with the information you already have; "
    "state clearly what could not be verified."
)


def _parse_args(raw: Any) -> dict[str, Any]:
    if isinstance(raw, dict):
        return raw
    if isinstance(raw, str) and raw.strip():
        try:
            parsed = json.loads(raw)
            return parsed if isinstance(parsed, dict) else {}
        except json.JSONDecodeError:
            return {}
    return {}


def _summary(result: Any) -> str:
    text = json.dumps(result, default=str, ensure_ascii=False)
    return text[:300] + ("…" if len(text) > 300 else "")


async def _execute(ctx: ToolContext, name: str, args: dict[str, Any], timeout_s: float) -> tuple[bool, Any]:
    tool = TOOLS.get(name)
    if tool is None:
        TOOL_CALLS.labels(name, "unknown").inc()
        return False, {"error": f"unknown tool {name!r}; allowed: {sorted(TOOLS)}"}
    try:
        result = await asyncio.wait_for(tool.handler(ctx, args), timeout=timeout_s)
        TOOL_CALLS.labels(name, "ok").inc()
        return True, result
    except TimeoutError:
        TOOL_CALLS.labels(name, "timeout").inc()
        return False, {"error": f"tool {name} timed out after {timeout_s}s"}
    except (KeyError, TypeError, ValueError) as exc:
        TOOL_CALLS.labels(name, "bad_args").inc()
        return False, {"error": f"invalid arguments for {name}: {exc}"}
    except Exception as exc:
        TOOL_CALLS.labels(name, "error").inc()
        log.warning("tool_failed", tool=name, error=str(exc))
        return False, {"error": f"{name} failed: {str(exc)[:300]}"}


async def run_agent(
    ollama: OllamaClient,
    messages: list[Message],
    ctx: ToolContext,
    *,
    max_iterations: int,
    tool_timeout: float,
    stream: bool = False,
    temperature: float = 0.1,
) -> AsyncIterator[dict[str, Any]]:
    convo = list(messages)
    trace: list[dict[str, Any]] = []
    specs = tool_specs()
    answer = ""
    for iteration in range(max_iterations + 1):
        final_round = iteration == max_iterations
        if final_round:
            convo.append({"role": "user", "content": FINALISE_INSTRUCTION})
        tools = None if final_round else specs
        content_parts: list[str] = []
        tool_calls: list[dict[str, Any]] = []
        if stream:
            async for item in ollama.chat_stream(convo, tools=tools, model=ctx.model, temperature=temperature):
                if "content" in item:
                    content_parts.append(item["content"])
                    yield {"type": "token", "content": item["content"]}
                if "tool_calls" in item:
                    tool_calls.extend(item["tool_calls"])
        else:
            msg = await ollama.chat(convo, tools=tools, model=ctx.model, temperature=temperature)
            content_parts.append(str(msg.get("content") or ""))
            tool_calls.extend(msg.get("tool_calls") or [])
        content = "".join(content_parts)
        if not tool_calls:
            answer = content.strip()
            break
        convo.append({"role": "assistant", "content": content, "tool_calls": tool_calls})
        for call in tool_calls:
            fn = call.get("function") or {}
            name = str(fn.get("name", ""))
            args = _parse_args(fn.get("arguments"))
            ok, result = await _execute(ctx, name, args, tool_timeout)
            entry = {"name": name, "args": args, "ok": ok, "result_summary": _summary(result)}
            trace.append(entry)
            yield {"type": "tool", **entry}
            convo.append({"role": "tool", "content": wrap_result(name, result), "tool_name": name})
    yield {
        "type": "done",
        "answer": answer,
        "tool_calls": trace,
        "citations": ctx.citations,
        "proposals": ctx.created_proposals,
    }


async def run_agent_to_completion(
    ollama: OllamaClient,
    messages: list[Message],
    ctx: ToolContext,
    *,
    max_iterations: int,
    tool_timeout: float,
    temperature: float = 0.1,
) -> dict[str, Any]:
    final: dict[str, Any] = {}
    async for ev in run_agent(
        ollama, messages, ctx, max_iterations=max_iterations, tool_timeout=tool_timeout, temperature=temperature
    ):
        if ev["type"] == "done":
            final = ev
    return final
