from __future__ import annotations

import json

import httpx
import pytest
import respx

from llm_service.clients.ollama import OllamaClient, _ThinkFilter, strip_thinking
from llm_service.config import Settings

BASE = "http://ollama.test"


def _client(**overrides: object) -> OllamaClient:
    return OllamaClient(Settings(ollama_url=BASE, **overrides))  # type: ignore[arg-type]


def test_strip_thinking_removes_blocks_and_unterminated_tail() -> None:
    assert strip_thinking("<think>\nhmm\n</think>\n\nHalo") == "Halo"
    assert strip_thinking("no tags here") == "no tags here"
    assert strip_thinking("<think>cut off by num_predict") == ""


@pytest.mark.parametrize("size", [1, 2, 3, 7, 100])
def test_think_filter_handles_tags_split_across_chunks(size: int) -> None:
    text = "<think>\nreasoning </thi here\n</think>\n\nJawaban <b>akhir</b> <th"
    f = _ThinkFilter()
    out = "".join(f.feed(text[i : i + size]) for i in range(0, len(text), size)) + f.flush()
    assert out == "Jawaban <b>akhir</b> <th"


@respx.mock
async def test_chat_strips_thinking_and_omits_think_flag_by_default() -> None:
    route = respx.post(f"{BASE}/api/chat").mock(
        return_value=httpx.Response(200, json={"message": {"role": "assistant", "content": "<think>x</think>OK"}})
    )
    msg = await _client().chat([{"role": "user", "content": "hi"}])
    assert msg["content"] == "OK"
    assert "think" not in json.loads(route.calls.last.request.content)


@respx.mock
async def test_stream_sends_think_flag_when_configured() -> None:
    lines = [
        {"message": {"content": "<thi"}},
        {"message": {"content": "nk>plan</think>Ha"}},
        {"message": {"content": "lo"}, "done": True},
    ]
    route = respx.post(f"{BASE}/api/chat").mock(
        return_value=httpx.Response(200, text="\n".join(json.dumps(x) for x in lines))
    )
    chunks = [c async for c in _client(ollama_think=False).chat_stream([{"role": "user", "content": "hi"}])]
    assert "".join(c.get("content", "") for c in chunks) == "Halo"
    assert json.loads(route.calls.last.request.content)["think"] is False


@pytest.mark.parametrize(("raw", "expected"), [("", None), ("false", False), ("true", True)])
def test_ollama_think_env_parsing(monkeypatch: pytest.MonkeyPatch, raw: str, expected: bool | None) -> None:
    monkeypatch.setenv("OLLAMA_THINK", raw)
    assert Settings().ollama_think is expected
