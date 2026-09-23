from __future__ import annotations

import json
import uuid
from typing import Any

import httpx
import respx

from llm_service.analysis.proposals import ProposalPipeline, resolve_citations
from llm_service.analysis.service import AnalysisService
from llm_service.clients.platform import CallContext, PlatformClient
from llm_service.jobs import JobRunner
from llm_service.project_context import ProjectContext, ProjectContextLoader
from tests.conftest import PROJECT, TENANT, USER
from tests.fakes import FakeOllama

CTX = CallContext(TENANT, PROJECT, USER)
BASE = f"http://rule.test/api/v1/projects/{PROJECT}"

VALID_RULE = {
    "code": "RL-LLM-001",
    "name": "Promo device farm",
    "description": "Many customers redeem on one device",
    "kind": "velocity",
    "typologies": ["promo_abuse"],
    "event_types": ["promo_redemption"],
    "risk_score": 40,
    "action": "score",
    "definition": {
        "kind": "velocity",
        "group_by": ["device_id"],
        "window": {"duration": "7d"},
        "aggregate": {"fn": "distinct_count", "field": "customer_id"},
        "compare": {"op": "gte", "right": {"type": "const", "value": 3}},
    },
}
BROKEN_RULE = {**VALID_RULE, "definition": {**VALID_RULE["definition"], "group_by": ["device"]}}


def rec(rule: dict[str, Any] | None, ptype: str = "new_rule", target: str | None = None) -> dict[str, Any]:
    return {
        "proposal_type": ptype,
        "target_rule_code": target,
        "rationale": "cluster 4 fraud rate 38%",
        "evidence": {"cluster_id": 4},
        "citations": [{"code": "POJK-12-2024", "section": "Pasal 5"}],
        "rule": rule,
    }


@respx.mock
async def test_invalid_rule_is_repaired_then_proposed(settings: Any) -> None:
    validate = respx.post(f"{BASE}/rules/validate").mock(
        side_effect=[
            httpx.Response(
                422,
                json={
                    "valid": False,
                    "errors": [{"path": "definition.group_by[0]", "message": "unknown velocity field 'device'"}],
                },
            ),
            httpx.Response(200, json={"valid": True, "errors": []}),
        ]
    )
    created = respx.post(f"{BASE}/proposals").mock(
        return_value=httpx.Response(201, json={"id": "p-1", "status": "pending", "backtest": {"hit_rate": 0.01}})
    )
    ollama = FakeOllama([{"json": VALID_RULE}])
    pipeline = ProposalPipeline(PlatformClient(settings), ollama, repair_attempts=2)
    retrieved = [
        {
            "code": "POJK-12-2024",
            "section": "BAB II > Pasal 5",
            "regulation_id": "r-1",
            "chunk_id": "r-1:9",
            "excerpt": "wajib memantau",
        }
    ]
    out = await pipeline.propose(
        CTX,
        rec(BROKEN_RULE),
        field_paths=["event.device_id"],
        rule_ids_by_code={},
        retrieved=retrieved,
        report_id="rep-1",
        model="fake-model",
    )
    assert out.status == "created" and out.proposal_id == "p-1" and out.repair_attempts == 1
    assert validate.call_count == 2
    repair_prompt = ollama.requests[0]["messages"][0]["content"]
    assert "unknown velocity field 'device'" in repair_prompt and "event.device_id" in repair_prompt
    body = json.loads(created.calls.last.request.content)
    assert body["source"] == "llm" and body["definition"]["definition"]["group_by"] == ["device_id"]
    assert body["report_id"] == "rep-1" and body["llm_model"] == "fake-model"
    assert body["citations"][0]["chunk_id"] == "r-1:9"  # resolved against retrieved chunk


@respx.mock
async def test_still_invalid_after_repairs_is_not_proposed(settings: Any) -> None:
    respx.post(f"{BASE}/rules/validate").mock(
        return_value=httpx.Response(422, json={"valid": False, "errors": [{"path": "x", "message": "bad"}]})
    )
    proposals = respx.post(f"{BASE}/proposals").mock(return_value=httpx.Response(201, json={"id": "p"}))
    ollama = FakeOllama([{"json": BROKEN_RULE}, {"content": "not json"}, {"content": "still not json"}])
    out = await ProposalPipeline(PlatformClient(settings), ollama, repair_attempts=2).propose(
        CTX, rec(BROKEN_RULE), field_paths=[], rule_ids_by_code={}, retrieved=[], report_id=None, model="m"
    )
    assert out.status == "invalid" and out.repair_attempts == 2
    assert not proposals.called


@respx.mock
async def test_retire_skips_validation_and_resolves_target(settings: Any) -> None:
    validate = respx.post(f"{BASE}/rules/validate")
    created = respx.post(f"{BASE}/proposals").mock(return_value=httpx.Response(201, json={"id": "p-2"}))
    pipeline = ProposalPipeline(PlatformClient(settings), FakeOllama())
    out = await pipeline.propose(
        CTX,
        rec(None, "retire_rule", "RL-OLD-1"),
        field_paths=[],
        rule_ids_by_code={"RL-OLD-1": "rule-uuid"},
        retrieved=[],
        report_id=None,
        model="m",
    )
    assert out.status == "created" and not validate.called
    body = json.loads(created.calls.last.request.content)
    assert body["target_rule_id"] == "rule-uuid" and body["definition"] is None
    unknown = await pipeline.propose(
        CTX,
        rec(None, "modify_rule", "RL-NOPE"),
        field_paths=[],
        rule_ids_by_code={},
        retrieved=[],
        report_id=None,
        model="m",
    )
    assert unknown.status == "skipped"


def test_resolve_citations_keeps_unmatched() -> None:
    out = resolve_citations([{"code": "X", "section": "Pasal 1"}], [])
    assert out == [{"code": "X", "section": "Pasal 1"}]


class FakeSearch:
    async def search(self, *a: Any, **k: Any) -> list[Any]:
        return []


@respx.mock
async def test_recommend_rules_degrades_and_creates_proposals(settings: Any) -> None:
    core = f"http://core.test/api/v1/projects/{PROJECT}"
    respx.get(f"{BASE}/rules").mock(
        return_value=httpx.Response(
            200,
            json={
                "items": [
                    {"id": "r1", "code": "RL-CARD-001", "name": "Card testing", "kind": "velocity", "status": "active"}
                ]
            },
        )
    )
    respx.get(f"{BASE}/rules/performance").mock(return_value=httpx.Response(200, json=[]))
    respx.get(f"{core}/analytics/overview").mock(return_value=httpx.Response(200, json={"totals": {"events": 10}}))
    respx.get(f"{core}/analytics/typologies").mock(return_value=httpx.Response(200, json=[]))
    respx.get(f"{core}/analytics/drift").mock(return_value=httpx.Response(500))  # degraded source
    respx.get(f"http://ml.test/api/v1/projects/{PROJECT}/ml/unsupervised/clusters").mock(
        side_effect=httpx.ConnectError("down")
    )  # degraded source
    respx.get(f"http://graph.test/api/v1/projects/{PROJECT}/graph/components").mock(
        return_value=httpx.Response(200, json=[])
    )
    respx.get(f"http://core.test/v1/internal/projects/{PROJECT}/field-catalog").mock(
        return_value=httpx.Response(200, json={"items": [{"path": "event.device_id"}]})
    )
    respx.post(f"{BASE}/rules/validate").mock(return_value=httpx.Response(200, json={"valid": True}))
    created = respx.post(f"{BASE}/proposals").mock(return_value=httpx.Response(201, json={"id": "p-9"}))

    ollama = FakeOllama(
        [{"json": {"summary_md": "## Ringkasan\nPromo farm terdeteksi.", "recommendations": [rec(VALID_RULE)]}}]
    )
    platform = PlatformClient(settings)
    svc = AnalysisService(
        settings,
        ollama,
        platform,
        FakeSearch(),  # type: ignore[arg-type]
        ProposalPipeline(platform, ollama),
        ProjectContextLoader(settings, platform),
        JobRunner(),
    )
    pctx = ProjectContext(
        project={"name": "Promo", "stage": "promo"},
        model="fake-model",
        temperature=0.1,
        language="id",
        system_prompt_extra="",
        regulation_ids=[],
    )
    md, structured = await svc._recommend_rules(CTX, pctx, uuid.uuid4(), {"max_rules": 3})
    assert set(structured["data_gaps"]) == {"feature_drift", "anomaly_clusters"}
    assert structured["proposals"][0]["status"] == "created"
    assert "Proposal yang dibuat" in md and "RL-LLM-001" in md
    prompt = ollama.requests[0]["messages"][0]["content"]
    assert "Project: Promo (stage: promo" in prompt and "<field_catalog>" in prompt
    assert ollama.requests[0]["format"]["required"] == ["summary_md", "recommendations"]
    assert created.called
