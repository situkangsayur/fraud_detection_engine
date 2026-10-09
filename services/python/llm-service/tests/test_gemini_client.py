from __future__ import annotations

import json

import httpx
import pytest
import respx

from llm_service.clients.gemini import GeminiClient, to_openai_messages
from llm_service.clients.ollama import OllamaError
from llm_service.config import Settings

BASE = "https://gemini.test/v1beta/openai"
OLLAMA = "http://ollama.test"


def _client() -> GeminiClient:
    return GeminiClient(
        Settings(  # type: ignore[call-arg]
            llm_provider="gemini", gemini_api_key="k", gemini_base_url=BASE, ollama_url=OLLAMA
        )
    )


def test_requires_api_key() -> None:
    with pytest.raises(ValueError, match="GEMINI_API_KEY"):
        GeminiClient(Settings(llm_provider="gemini"))  # type: ignore[call-arg]


def test_settings_chat_model_follows_provider() -> None:
    assert Settings(llm_provider="gemini").chat_model == "gemini-flash-latest"  # type: ignore[call-arg]
    assert Settings(ollama_chat_model="qwen3:8b").chat_model == "qwen3:8b"  # type: ignore[call-arg]


def test_tool_results_reference_call_ids_in_order() -> None:
    msgs = to_openai_messages(
        [
            {"role": "user", "content": "q"},
            {
                "role": "assistant",
                "content": "",
                "tool_calls": [
                    {"function": {"name": "a", "arguments": {"x": 1}}},
                    {"id": "given", "function": {"name": "b", "arguments": '{"y": 2}'}},
                ],
            },
            {"role": "tool", "content": "ra", "tool_name": "a"},
            {"role": "tool", "content": "rb", "tool_name": "b"},
        ]
    )
    calls = msgs[1]["tool_calls"]
    assert calls[0]["function"]["arguments"] == '{"x": 1}'
    assert msgs[1]["content"] is None
    assert [m["tool_call_id"] for m in msgs[2:]] == [calls[0]["id"], "given"]


@respx.mock
async def test_chat_maps_tool_calls_schema_and_model() -> None:
    route = respx.post(f"{BASE}/chat/completions").mock(
        return_value=httpx.Response(
            200,
            json={
                "choices": [
                    {
                        "message": {
                            "role": "assistant",
                            "content": None,
                            "tool_calls": [
                                {"id": "c1", "type": "function", "function": {"name": "t", "arguments": '{"k": 5}'}}
                            ],
                        }
                    }
                ]
            },
        )
    )
    msg = await _client().chat([{"role": "user", "content": "hi"}], format_schema={"type": "object"}, model="qwen3:8b")
    assert msg["tool_calls"] == [{"id": "c1", "function": {"name": "t", "arguments": {"k": 5}}}]
    sent = json.loads(route.calls.last.request.content)
    assert sent["model"] == "gemini-flash-latest", "Ollama model names fall back to the Gemini model"
    assert sent["response_format"]["json_schema"]["schema"] == {"type": "object"}
    assert route.calls.last.request.headers["authorization"] == "Bearer k"


@respx.mock
async def test_stream_yields_tokens_then_assembled_tool_calls() -> None:
    events = [
        {"choices": [{"delta": {"content": "Ha"}}]},
        {"choices": [{"delta": {"content": "lo"}}]},
        {
            "choices": [
                {
                    "delta": {
                        "tool_calls": [{"index": 0, "id": "c1", "function": {"name": "search", "arguments": '{"q":'}}]
                    }
                }
            ]
        },
        {"choices": [{"delta": {"tool_calls": [{"index": 0, "function": {"arguments": ' "x"}'}}]}}]},
    ]
    body = "".join(f"data: {json.dumps(e)}\n\n" for e in events) + "data: [DONE]\n\n"
    respx.post(f"{BASE}/chat/completions").mock(return_value=httpx.Response(200, text=body))
    items = [i async for i in _client().chat_stream([{"role": "user", "content": "hi"}])]
    assert "".join(i.get("content", "") for i in items) == "Halo"
    assert items[-1] == {"tool_calls": [{"id": "c1", "function": {"name": "search", "arguments": {"q": "x"}}}]}


@respx.mock
async def test_http_errors_surface_status_and_detail() -> None:
    respx.post(f"{BASE}/chat/completions").mock(return_value=httpx.Response(403, text="denied access"))
    with pytest.raises(OllamaError, match="403 denied access"):
        await _client().chat([{"role": "user", "content": "hi"}])


@respx.mock
async def test_embeddings_stay_on_ollama() -> None:
    respx.post(f"{OLLAMA}/api/embed").mock(return_value=httpx.Response(200, json={"embeddings": [[0.1, 0.2]]}))
    assert await _client().embed(["x"]) == [[0.1, 0.2]]
