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
from datetime import UTC, datetime, timedelta
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


# Small models ask for k=1 and then fill the gaps from memory; always retrieve enough context to ground the answer.
_MIN_REGULATION_HITS = 4


def _int_arg(a: dict[str, Any], name: str, default: int, lo: int, hi: int) -> int:
    """Small models often send 0, null or strings for optional integers; fall back to the default and clamp."""
    try:
        v = int(a.get(name) or default)
    except (TypeError, ValueError):
        v = default
    return min(max(v if v > 0 else default, lo), hi)


# ---------------------------------------------------------------------------- handlers
async def _search_regulations(ctx: ToolContext, a: dict[str, Any]) -> Any:
    hits = await ctx.search.search(
        ctx.call.tenant_id, str(a.get("query", "")), ctx.regulation_ids, k=_int_arg(a, "k", 5, _MIN_REGULATION_HITS, 10)
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
    return await ctx.platform.rules_performance(ctx.call, _int_arg(a, "since_days", 30, 1, 365))


async def _validate(ctx: ToolContext, a: dict[str, Any]) -> Any:
    return await ctx.platform.validate_rule(ctx.call, dict(a["rule"]))


async def _backtest(ctx: ToolContext, a: dict[str, Any]) -> Any:
    return await ctx.platform.backtest_rule(ctx.call, dict(a["rule"]), _int_arg(a, "since_days", 30, 1, 365))


async def _overview(ctx: ToolContext, a: dict[str, Any]) -> Any:
    return await ctx.platform.analytics_overview(ctx.call, _int_arg(a, "since_days", 7, 1, 365))


async def _drift(ctx: ToolContext, _: dict[str, Any]) -> Any:
    return await ctx.platform.feature_drift(ctx.call)


async def _typologies(ctx: ToolContext, _: dict[str, Any]) -> Any:
    return await ctx.platform.typology_stats(ctx.call)


async def _clusters(ctx: ToolContext, _: dict[str, Any]) -> Any:
    return await ctx.platform.anomaly_clusters(ctx.call)


async def _components(ctx: ToolContext, a: dict[str, Any]) -> Any:
    return await ctx.platform.graph_components(ctx.call, _int_arg(a, "min_size", 3, 2, 1000))


async def _communities(ctx: ToolContext, a: dict[str, Any]) -> Any:
    return await ctx.platform.graph_communities(ctx.call, _int_arg(a, "min_size", 3, 2, 1000))


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


# ---------------------------------------------------------------------------- data lookup (customers, events, cases)
# Same formats as platform::pii in Rust, so masked values look identical to what the UI shows.
def mask_email(v: str) -> str:
    local, sep, domain = v.partition("@")
    return f"{local[:1]}***@{domain}" if sep else "***"


def mask_phone(v: str) -> str:
    return "****" if len(v) <= 7 else f"{v[:6]}****{v[-3:]}"


_MASKERS: dict[str, Callable[[str], str]] = {
    "email": mask_email,
    "email_normalized": mask_email,
    "phone": mask_phone,
    "phone_normalized": mask_phone,
}
# Large or raw blobs the model does not need for analysis (raw payload is still reachable in the UI).
_DROP_KEYS = {"payload", "raw", "raw_payload"}


def minimise(value: Any) -> Any:
    """Data minimisation (UU PDP) before anything reaches the model: mask contact fields, drop raw payloads."""
    if isinstance(value, dict):
        out: dict[str, Any] = {}
        for k, v in value.items():
            if k in _DROP_KEYS:
                continue
            masker = _MASKERS.get(k)
            out[k] = masker(v) if masker and isinstance(v, str) and v else minimise(v)
        return out
    if isinstance(value, list):
        return [minimise(v) for v in value]
    return value


def _items(page: Any) -> list[Any]:
    return list(page.get("items", [])) if isinstance(page, dict) else list(page or [])


def _filters(a: dict[str, Any], keys: tuple[str, ...]) -> dict[str, Any]:
    return {k: a[k] for k in keys if a.get(k) not in (None, "")}


async def _search_customers(ctx: ToolContext, a: dict[str, Any]) -> Any:
    q = str(a.get("q") or "").strip()
    params = _filters(a, ("risk_label",)) | {"page_size": _int_arg(a, "limit", 10, 1, 20)}
    if q:
        params["q"] = q
    page = await ctx.platform.list_customers(ctx.call, params)
    result: dict[str, Any] = {
        "customers": minimise(_items(page)),
        "total": page.get("total") if isinstance(page, dict) else None,
    }
    # Phone, IP, device and reference ids are matched on graph entities (exact, normalised).
    if len(q) >= 2:
        try:
            hits = await ctx.platform.graph_search(ctx.call, q)
            result["linked_by_entity"] = [
                {k: e.get(k) for k in ("kind", "display_value", "customer_count", "customer_ids")}
                for e in (hits or {}).get("entities", [])
            ]
        except Exception as exc:  # graph-service down: customer search still answers
            result["linked_by_entity_error"] = str(exc)[:200]
    return result


async def _get_customer(ctx: ToolContext, a: dict[str, Any]) -> Any:
    cid = str(a["customer_id"])
    customer = await ctx.platform.get_customer(ctx.call, cid)
    events = await ctx.platform.list_events(
        ctx.call, {"customer_id": cid, "page_size": _int_arg(a, "events_limit", 15, 1, 30)}
    )
    result: dict[str, Any] = {"customer": minimise(customer), "recent_events": minimise(_items(events))}
    try:
        result["fraud_proximity"] = await ctx.platform.fraud_proximity(ctx.call, cid)
    except Exception as exc:
        result["fraud_proximity_error"] = str(exc)[:200]
    return result


async def _search_events(ctx: ToolContext, a: dict[str, Any]) -> Any:
    now = datetime.now(UTC)
    params = _filters(a, ("q", "customer_id", "decision", "event_type", "min_score")) | {
        "from": (now - timedelta(days=_int_arg(a, "since_days", 30, 1, 365))).isoformat(),
        "page_size": _int_arg(a, "limit", 20, 1, 50),
    }
    page = await ctx.platform.list_events(ctx.call, params)
    return {"events": minimise(_items(page)), "total": page.get("total") if isinstance(page, dict) else None}


async def _get_event(ctx: ToolContext, a: dict[str, Any]) -> Any:
    return minimise(await ctx.platform.get_event(ctx.call, str(a["event_id"])))


async def _search_cases(ctx: ToolContext, a: dict[str, Any]) -> Any:
    params = _filters(a, ("status", "typology", "priority", "customer_id")) | {
        "page_size": _int_arg(a, "limit", 20, 1, 50)
    }
    page = await ctx.platform.list_cases(ctx.call, params)
    return {"cases": minimise(_items(page)), "total": page.get("total") if isinstance(page, dict) else None}


async def _get_case(ctx: ToolContext, a: dict[str, Any]) -> Any:
    return minimise(await ctx.platform.get_case(ctx.call, str(a["case_id"])))


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
            _obj(
                {"query": {"type": "string"}, "k": {"type": "integer", "minimum": _MIN_REGULATION_HITS, "maximum": 10}},
                ["query"],
            ),
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
            _obj({"since_days": {"type": "integer", "minimum": 1, "maximum": 365}}),
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
            _obj({"rule": _RULE_PARAM, "since_days": {"type": "integer", "minimum": 1, "maximum": 365}}, ["rule"]),
            _backtest,
        ),
        Tool(
            "get_analytics_overview",
            "Event volumes, decision mix, daily trend, score histogram, open cases.",
            _obj({"since_days": {"type": "integer", "minimum": 1, "maximum": 365}}),
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
            _obj({"min_size": {"type": "integer", "minimum": 2}}),
            _components,
        ),
        Tool(
            "get_graph_communities",
            "Louvain communities of the customer graph with fraud rate.",
            _obj({"min_size": {"type": "integer", "minimum": 2}}),
            _communities,
        ),
        Tool(
            "get_project_context",
            "Project details, ML/graph config, attached regulations and available field paths for rules.",
            _obj(),
            _project_context,
        ),
        Tool(
            "search_customers",
            "Find customers by name (partial), customer ID, email, phone number, IP, device ID or transaction "
            "reference, and/or risk_label (fraud|legit|unknown). Contact fields come back masked. "
            "'linked_by_entity' lists customers sharing the searched phone/email/IP/device.",
            _obj(
                {
                    "q": {"type": "string", "description": "name, ID, email, phone, IP, device id or ref id"},
                    "risk_label": {"type": "string", "enum": ["fraud", "legit", "unknown"]},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 20},
                }
            ),
            _search_customers,
        ),
        Tool(
            "get_customer",
            "One customer's profile, 30-day stats, recent transactions with decisions and scores, and graph "
            "distance to the nearest fraud customer. Use the id returned by search_customers.",
            _obj(
                {
                    "customer_id": {"type": "string"},
                    "events_limit": {"type": "integer", "minimum": 1, "maximum": 30},
                },
                ["customer_id"],
            ),
            _get_customer,
        ),
        Tool(
            "search_events",
            "List transactions/events (newest first) with decision and score. Filters: q (event or customer "
            "external id), customer_id, decision (approve|review|decline), event_type, min_score, since_days.",
            _obj(
                {
                    "q": {"type": "string"},
                    "customer_id": {"type": "string"},
                    "decision": {"type": "string", "enum": ["approve", "review", "decline"]},
                    "event_type": {"type": "string"},
                    "min_score": {"type": "number"},
                    "since_days": {"type": "integer", "minimum": 1, "maximum": 365},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 50},
                }
            ),
            _search_events,
        ),
        Tool(
            "get_event",
            "One event with features, decision (per-engine scores and reasons/rule hits), labels and case.",
            _obj({"event_id": {"type": "string"}}, ["event_id"]),
            _get_event,
        ),
        Tool(
            "search_cases",
            "List investigation cases. Filters: status, typology, priority, customer_id.",
            _obj(
                {
                    "status": {"type": "string"},
                    "typology": {"type": "string"},
                    "priority": {"type": "string"},
                    "customer_id": {"type": "string"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 50},
                }
            ),
            _search_cases,
        ),
        Tool(
            "get_case",
            "One case with its event, decision, customer and graph summary.",
            _obj({"case_id": {"type": "string"}}, ["case_id"]),
            _get_case,
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
