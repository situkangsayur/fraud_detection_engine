"""Tool registry for the chat agent (Ollama native function calling).

Allow-list: every tool here is read-only except ``create_rule_proposal`` which creates a *pending* proposal.
Tool results are wrapped in ``<tool_result>`` blocks and truncated, so they can never masquerade as instructions
or blow up the context window.
"""

from __future__ import annotations

import json
import uuid
from collections.abc import Awaitable, Callable
from dataclasses import dataclass, field
from typing import Any

from llm_service.analysis.proposals import ProposalPipeline
from llm_service.clients.platform import CallContext, PlatformClient
from llm_service.retrieval.search import RegulationSearch

MAX_RESULT_CHARS = 6000


@dataclass
class ToolContext:
    call: CallContext
    platform: PlatformClient
    search: RegulationSearch
    proposals: ProposalPipeline
    regulation_ids: list[uuid.UUID]
    model: str
    project: dict[str, Any] = field(default_factory=dict)
    citations: list[dict[str, Any]] = field(default_factory=list)
    created_proposals: list[dict[str, Any]] = field(default_factory=list)
    field_paths: list[str] | None = None


Handler = Callable[[ToolContext, dict[str, Any]], Awaitable[Any]]


@dataclass(frozen=True)
class Tool:
    name: str
    description: str
    parameters: dict[str, Any]
    handler: Handler
    writes: bool = False

    def spec(self) -> dict[str, Any]:
        return {
            "type": "function",
            "function": {"name": self.name, "description": self.description, "parameters": self.parameters},
        }


def _obj(props: dict[str, Any] | None = None, required: list[str] | None = None) -> dict[str, Any]:
    return {"type": "object", "properties": props or {}, "required": required or []}


async def _field_paths(ctx: ToolContext) -> list[str]:
    if ctx.field_paths is None:
        try:
            cat = await ctx.platform.field_catalog(ctx.call)
            items = cat.get("items", cat) if isinstance(cat, dict) else cat
            ctx.field_paths = [str(i["path"]) for i in items if isinstance(i, dict) and "path" in i]
        except Exception:
            ctx.field_paths = []
    return ctx.field_paths


# ---------------------------------------------------------------------------- handlers
async def _search_regulations(ctx: ToolContext, a: dict[str, Any]) -> Any:
    hits = await ctx.search.search(
        ctx.call.tenant_id, str(a.get("query", "")), ctx.regulation_ids, k=int(a.get("k", 5))
    )
    cites = [h.citation() for h in hits]
    for c in cites:
        if c["chunk_id"] not in {x["chunk_id"] for x in ctx.citations}:
            ctx.citations.append(c)
    if not ctx.regulation_ids:
        return {"results": [], "note": "no regulations are attached to this project"}
    return {"results": [{**c, "text": h.hit.text} for c, h in zip(cites, hits, strict=True)]}


async def _list_rules(ctx: ToolContext, a: dict[str, Any]) -> Any:
    params = {k: a[k] for k in ("status", "kind", "typology", "q") if a.get(k)}
    params["page_size"] = 200
    return await ctx.platform.list_rules(ctx.call, params)


async def _get_rule(ctx: ToolContext, a: dict[str, Any]) -> Any:
    return await ctx.platform.get_rule(ctx.call, str(a["rule_id"]))


async def _rules_performance(ctx: ToolContext, a: dict[str, Any]) -> Any:
    return await ctx.platform.rules_performance(ctx.call, int(a.get("since_days", 30)))


async def _validate(ctx: ToolContext, a: dict[str, Any]) -> Any:
    return await ctx.platform.validate_rule(ctx.call, dict(a["rule"]))


async def _backtest(ctx: ToolContext, a: dict[str, Any]) -> Any:
    return await ctx.platform.backtest_rule(ctx.call, dict(a["rule"]), int(a.get("since_days", 30)))


async def _overview(ctx: ToolContext, a: dict[str, Any]) -> Any:
    return await ctx.platform.analytics_overview(ctx.call, int(a.get("since_days", 7)))


async def _drift(ctx: ToolContext, _: dict[str, Any]) -> Any:
    return await ctx.platform.feature_drift(ctx.call)


async def _typologies(ctx: ToolContext, _: dict[str, Any]) -> Any:
    return await ctx.platform.typology_stats(ctx.call)


async def _clusters(ctx: ToolContext, _: dict[str, Any]) -> Any:
    return await ctx.platform.anomaly_clusters(ctx.call)


async def _components(ctx: ToolContext, a: dict[str, Any]) -> Any:
    return await ctx.platform.graph_components(ctx.call, int(a.get("min_size", 3)))


async def _communities(ctx: ToolContext, a: dict[str, Any]) -> Any:
    return await ctx.platform.graph_communities(ctx.call, int(a.get("min_size", 3)))


async def _project_context(ctx: ToolContext, _: dict[str, Any]) -> Any:
    keep = (
        "id",
        "name",
        "slug",
        "stage",
        "description",
        "business_context",
        "timezone",
        "currency",
        "ml_config",
        "graph_config",
        "summary",
    )
    return {k: ctx.project.get(k) for k in keep if k in ctx.project} | {
        "attached_regulations": [str(r) for r in ctx.regulation_ids],
        "field_catalog_paths": (await _field_paths(ctx))[:300],
    }


async def _create_proposal(ctx: ToolContext, a: dict[str, Any]) -> Any:
    rules = await ctx.platform.list_rules(ctx.call, {"page_size": 200})
    items = rules.get("items", []) if isinstance(rules, dict) else rules
    by_code = {str(r.get("code")): str(r.get("id")) for r in items if isinstance(r, dict)}
    rec = {
        "proposal_type": a.get("proposal_type", "new_rule"),
        "target_rule_code": a.get("target_rule_code"),
        "rationale": a.get("rationale", ""),
        "citations": a.get("citations") or [],
        "evidence": a.get("evidence") or {},
        "rule": a.get("rule"),
    }
    outcome = await ctx.proposals.propose(
        ctx.call,
        rec,
        field_paths=await _field_paths(ctx),
        rule_ids_by_code=by_code,
        retrieved=ctx.citations,
        report_id=None,
        model=ctx.model,
    )
    result = outcome.as_dict()
    if outcome.status == "created":
        ctx.created_proposals.append(result)
        result["note"] = "Proposal is PENDING human approval; if approved it starts in shadow mode."
    return result


_RULE_PARAM = {"type": "object", "description": "Full rule envelope following the Rule DSL"}

TOOLS: dict[str, Tool] = {
    t.name: t
    for t in [
        Tool(
            "search_regulations",
            "Hybrid search over the regulations/policies attached to this project. "
            "Returns chunks with code + section for citation.",
            _obj({"query": {"type": "string"}, "k": {"type": "integer", "minimum": 1, "maximum": 10}}, ["query"]),
            _search_regulations,
        ),
        Tool(
            "list_rules",
            "List rules of the project (filters: status, kind, typology, q).",
            _obj(
                {
                    "status": {"type": "string"},
                    "kind": {"type": "string"},
                    "typology": {"type": "string"},
                    "q": {"type": "string"},
                }
            ),
            _list_rules,
        ),
        Tool(
            "get_rule", "Get one rule with its versions.", _obj({"rule_id": {"type": "string"}}, ["rule_id"]), _get_rule
        ),
        Tool(
            "get_rules_performance",
            "Per-rule evaluated/matched/trapped counts, hit rate and precision.",
            _obj({"since_days": {"type": "integer"}}),
            _rules_performance,
        ),
        Tool(
            "validate_rule_definition",
            "Validate a rule envelope with the rule-service validator.",
            _obj({"rule": _RULE_PARAM}, ["rule"]),
            _validate,
        ),
        Tool(
            "backtest_rule_definition",
            "Backtest an (unsaved) rule envelope on recent project data.",
            _obj({"rule": _RULE_PARAM, "since_days": {"type": "integer"}}, ["rule"]),
            _backtest,
        ),
        Tool(
            "get_analytics_overview",
            "Event volumes, decision mix, daily trend, score histogram, open cases.",
            _obj({"since_days": {"type": "integer"}}),
            _overview,
        ),
        Tool(
            "get_feature_drift",
            "Population stability index (PSI) per feature, recent 7d vs previous 30d.",
            _obj(),
            _drift,
        ),
        Tool(
            "get_typology_stats", "Labelled fraud counts and amounts per fraud typology and week.", _obj(), _typologies
        ),
        Tool(
            "get_anomaly_clusters",
            "Clusters of the active unsupervised model with size, fraud rate, profile.",
            _obj(),
            _clusters,
        ),
        Tool(
            "get_graph_components",
            "Connected customer components (shared device/phone/card/address...) with fraud counts.",
            _obj({"min_size": {"type": "integer"}}),
            _components,
        ),
        Tool(
            "get_graph_communities",
            "Louvain communities of the customer graph with fraud rate.",
            _obj({"min_size": {"type": "integer"}}),
            _communities,
        ),
        Tool(
            "get_project_context",
            "Project details, ML/graph config, attached regulations and available field paths for rules.",
            _obj(),
            _project_context,
        ),
        Tool(
            "create_rule_proposal",
            "Create a PENDING rule proposal for human review (never activates anything). "
            "Only call when the analyst explicitly asks to propose a rule.",
            _obj(
                {
                    "proposal_type": {"type": "string", "enum": ["new_rule", "modify_rule", "retire_rule"]},
                    "target_rule_code": {"type": "string"},
                    "rationale": {"type": "string"},
                    "rule": _RULE_PARAM,
                    "evidence": {"type": "object"},
                    "citations": {"type": "array", "items": {"type": "object"}},
                },
                ["proposal_type", "rationale"],
            ),
            _create_proposal,
            writes=True,
        ),
    ]
}


def tool_specs() -> list[dict[str, Any]]:
    return [t.spec() for t in TOOLS.values()]


def wrap_result(name: str, result: Any) -> str:
    body = json.dumps(result, default=str, ensure_ascii=False)
    if len(body) > MAX_RESULT_CHARS:
        body = body[:MAX_RESULT_CHARS] + "… [truncated]"
    return f'<tool_result name="{name}">\n{body}\n</tool_result>'
