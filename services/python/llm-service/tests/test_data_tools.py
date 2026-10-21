"""Data lookup tools (customers, events, cases): routing, filters and data minimisation."""

from __future__ import annotations

import json
from typing import Any

import httpx
import respx

from llm_service.agent.loop import run_agent_to_completion
from llm_service.agent.tools import TOOLS, mask_email, mask_phone, minimise
from tests.conftest import PROJECT
from tests.fakes import FakeOllama, tool_call
from tests.test_agent_loop import make_ctx

CORE = f"http://core.test/api/v1/projects/{PROJECT}"
GRAPH = f"http://graph.test/api/v1/projects/{PROJECT}"
CUSTOMER = {
    "id": "c-1",
    "external_id": "CUST-001",
    "full_name": "Budi Santoso",
    "email": "budi.santoso@gmail.com",
    "phone": "+6281234567890",
    "risk_label": "fraud",
}


def test_masks_match_platform_pii_format() -> None:
    assert mask_email("budi.santoso@gmail.com") == "b***@gmail.com"
    assert mask_email("no-at-sign") == "***"
    assert mask_phone("+6281234567890") == "+62812****890"
    assert mask_phone("12345") == "****"


def test_minimise_masks_nested_contacts_and_drops_payload() -> None:
    out = minimise({"customer": CUSTOMER, "payload": {"card": "4111"}, "events": [{"email": "a@b.co", "amount": 5}]})
    assert out["customer"]["email"] == "b***@gmail.com" and out["customer"]["phone"] == "+62812****890"
    assert out["customer"]["full_name"] == "Budi Santoso"
    assert "payload" not in out
    assert out["events"] == [{"email": "a***@b.co", "amount": 5}]


def test_data_tools_are_read_only() -> None:
    names = ["search_customers", "get_customer", "search_events", "get_event", "search_cases", "get_case"]
    assert all(n in TOOLS and not TOOLS[n].writes for n in names)


async def _run(settings: Any, *calls: dict[str, Any]) -> tuple[dict[str, Any], FakeOllama]:
    ollama = FakeOllama([*calls, {"content": "ok"}])
    final = await run_agent_to_completion(
        ollama,
        [{"role": "user", "content": "cek"}],  # type: ignore[arg-type]
        make_ctx(settings, ollama),
        max_iterations=6,
        tool_timeout=2,
    )
    return final, ollama


@respx.mock
async def test_search_customers_combines_core_and_graph_entities(settings: Any) -> None:
    core = respx.get(f"{CORE}/customers").mock(
        return_value=httpx.Response(200, json={"items": [CUSTOMER], "total": 1, "page": 1, "page_size": 10})
    )
    respx.get(f"{GRAPH}/graph/search").mock(
        return_value=httpx.Response(
            200,
            json={
                "customers": [],
                "entities": [
                    {
                        "id": 7,
                        "kind": "phone",
                        "display_value": "+62812****890",
                        "customer_count": 3,
                        "customer_ids": ["c-1", "c-2", "c-3"],
                    }
                ],
            },
        )
    )
    final, ollama = await _run(settings, tool_call("search_customers", q="081234567890", risk_label="fraud"))
    assert final["tool_calls"][0]["ok"]
    assert core.calls.last.request.url.params["q"] == "081234567890"
    assert core.calls.last.request.url.params["risk_label"] == "fraud"
    body = ollama.requests[1]["messages"][-1]["content"]
    payload = json.loads(body.split("\n", 1)[1].rsplit("\n", 1)[0])
    assert payload["customers"][0]["email"] == "b***@gmail.com"
    assert "budi.santoso@gmail.com" not in body and "+6281234567890" not in body
    assert payload["linked_by_entity"][0]["customer_count"] == 3


@respx.mock
async def test_get_customer_adds_events_and_survives_graph_failure(settings: Any) -> None:
    respx.get(f"{CORE}/customers/c-1").mock(return_value=httpx.Response(200, json=CUSTOMER))
    events = respx.get(f"{CORE}/events").mock(
        return_value=httpx.Response(200, json={"items": [{"id": "e-1", "decision": "decline", "final_score": 91.0}]})
    )
    respx.get(f"{GRAPH}/graph/customers/c-1/fraud-proximity").mock(return_value=httpx.Response(503))
    final, ollama = await _run(settings, tool_call("get_customer", customer_id="c-1"))
    assert final["tool_calls"][0]["ok"]
    assert events.calls.last.request.url.params["customer_id"] == "c-1"
    body = ollama.requests[1]["messages"][-1]["content"]
    assert '"decline"' in body and "fraud_proximity_error" in body


@respx.mock
async def test_search_events_and_cases_pass_filters(settings: Any) -> None:
    ev = respx.get(f"{CORE}/events").mock(return_value=httpx.Response(200, json={"items": [], "total": 0}))
    cs = respx.get(f"{CORE}/cases").mock(return_value=httpx.Response(200, json={"items": [], "total": 0}))
    final, _ = await _run(
        settings,
        tool_call("search_events", decision="decline", since_days=7, limit=0),
        tool_call("search_cases", status="open", typology="money_mule"),
    )
    assert [t["ok"] for t in final["tool_calls"]] == [True, True]
    p = ev.calls.last.request.url.params
    assert p["decision"] == "decline" and p["page_size"] == "20" and "from" in p
    assert cs.calls.last.request.url.params["typology"] == "money_mule"
