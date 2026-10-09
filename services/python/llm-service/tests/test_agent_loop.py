from __future__ import annotations

import json
from typing import Any

import httpx
import pytest
import respx

from llm_service.agent.loop import FINALISE_INSTRUCTION, run_agent, run_agent_to_completion
from llm_service.agent.tools import TOOLS, ToolContext, _int_arg
from llm_service.analysis.proposals import ProposalPipeline
from llm_service.clients.platform import CallContext, PlatformClient
from tests.conftest import PROJECT, TENANT, USER
from tests.fakes import FakeOllama, tool_call


class FakeSearch:
    async def search(self, tenant_id: Any, query: str, ids: Any, k: int = 6) -> list[Any]:
        return []


def make_ctx(settings: Any, ollama: FakeOllama) -> ToolContext:
    platform = PlatformClient(settings)
    return ToolContext(
        call=CallContext(TENANT, PROJECT, USER, "req-1"),
        platform=platform,
        search=FakeSearch(),
        proposals=ProposalPipeline(platform, ollama),  # type: ignore[arg-type]
        regulation_ids=[],
        model="fake-model",
        project={"name": "Checkout", "stage": "pre_payment"},
    )


def test_only_one_write_tool_in_allow_list() -> None:
    assert [t.name for t in TOOLS.values() if t.writes] == ["create_rule_proposal"]


@respx.mock
async def test_tool_call_then_answer(settings: Any) -> None:
    perf = respx.get(f"http://rule.test/api/v1/projects/{PROJECT}/rules/performance").mock(
        return_value=httpx.Response(200, json=[{"code": "RL-1", "hit_rate": 0.02}])
    )
    ollama = FakeOllama([tool_call("get_rules_performance", since_days=7), {"content": "RL-1 memiliki hit rate 2%."}])
    final = await run_agent_to_completion(
        ollama,
        [{"role": "user", "content": "performa rule?"}],  # type: ignore[arg-type]
        make_ctx(settings, ollama),
        max_iterations=4,
        tool_timeout=2,
    )
    assert final["answer"] == "RL-1 memiliki hit rate 2%."
    assert final["tool_calls"][0]["name"] == "get_rules_performance" and final["tool_calls"][0]["ok"]
    req = perf.calls.last.request
    assert req.url.params["since_days"] == "7"
    assert req.headers["authorization"] == f"Bearer {settings.internal_api_token}"
    assert req.headers["x-tenant-id"] == str(TENANT) and req.headers["x-project-id"] == str(PROJECT)
    assert req.headers["x-actor"] == str(USER) and req.headers["x-request-id"] == "req-1"
    tool_msg = ollama.requests[1]["messages"][-1]
    assert tool_msg["role"] == "tool" and tool_msg["content"].startswith('<tool_result name="get_rules_performance">')


async def test_unknown_tool_and_bad_args_are_reported_not_raised(settings: Any) -> None:
    ollama = FakeOllama([tool_call("drop_database"), tool_call("get_rule"), {"content": "done"}])
    final = await run_agent_to_completion(
        ollama,
        [{"role": "user", "content": "x"}],  # type: ignore[arg-type]
        make_ctx(settings, ollama),
        max_iterations=4,
        tool_timeout=2,
    )
    assert final["answer"] == "done"
    assert [t["ok"] for t in final["tool_calls"]] == [False, False]
    assert "unknown tool" in final["tool_calls"][0]["result_summary"]
    assert "invalid arguments" in final["tool_calls"][1]["result_summary"]


async def test_iteration_budget_forces_final_answer_without_tools(settings: Any) -> None:
    ollama = FakeOllama([tool_call("get_project_context")] * 3 + [{"content": "final"}])
    final = await run_agent_to_completion(
        ollama,
        [{"role": "user", "content": "x"}],  # type: ignore[arg-type]
        make_ctx(settings, ollama),
        max_iterations=3,
        tool_timeout=2,
    )
    assert final["answer"] == "final"
    last = ollama.requests[-1]
    assert last["tools"] is None and last["messages"][-1]["content"] == FINALISE_INSTRUCTION


async def test_streaming_emits_tokens_and_done(settings: Any) -> None:
    ollama = FakeOllama([{"content": "halo analis"}])
    events = [
        ev
        async for ev in run_agent(
            ollama,
            [{"role": "user", "content": "hai"}],  # type: ignore[arg-type]
            make_ctx(settings, ollama),
            max_iterations=2,
            tool_timeout=2,
            stream=True,
        )
    ]
    assert [e["type"] for e in events] == ["token", "token", "done"]
    assert events[-1]["answer"] == "halo analis"


@pytest.mark.parametrize("raw", ['{"since_days": 3}', {"since_days": 3}])
async def test_string_or_dict_arguments(settings: Any, raw: Any) -> None:
    with respx.mock:
        respx.get(f"http://rule.test/api/v1/projects/{PROJECT}/rules/performance").mock(
            return_value=httpx.Response(200, json=[])
        )
        call = {
            "role": "assistant",
            "content": "",
            "tool_calls": [{"function": {"name": "get_rules_performance", "arguments": raw}}],
        }
        ollama = FakeOllama([call, {"content": "ok"}])
        final = await run_agent_to_completion(
            ollama,
            [{"role": "user", "content": "x"}],  # type: ignore[arg-type]
            make_ctx(settings, ollama),
            max_iterations=2,
            tool_timeout=2,
        )
        assert json.loads(json.dumps(final["tool_calls"][0]["args"])) == {"since_days": 3}


@pytest.mark.parametrize(
    ("raw", "expected"),
    [({}, 5), ({"k": 0}, 5), ({"k": None}, 5), ({"k": "3"}, 3), ({"k": "x"}, 5), ({"k": -2}, 5), ({"k": 50}, 10)],
)
def test_int_args_from_the_model_fall_back_and_clamp(raw: dict[str, Any], expected: int) -> None:
    assert _int_arg(raw, "k", 5, 1, 10) == expected


def test_regulation_search_has_a_floor() -> None:
    assert _int_arg({"k": 1}, "k", 5, 4, 10) == 4
