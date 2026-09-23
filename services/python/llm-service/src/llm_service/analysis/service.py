"""Asynchronous LLM analyses written to ``llm.reports``.

* rule_relevance   — keep / tune / retire per rule (performance + drift + regulation fit)
* fraud_situation  — narrative of the current fraud & anomaly situation with key risks
* regulation_impact — obligations of a (changed) regulation vs current rules → gaps → proposals
* recommend_rules  — new data patterns + regulations → validated rule proposals

Every data source is fetched defensively: an unavailable service is listed in ``structured.data_gaps`` instead of
failing the whole report.
"""

from __future__ import annotations

import uuid
from collections.abc import Awaitable
from datetime import UTC, datetime
from typing import Any, Literal

from starlette.concurrency import run_in_threadpool

from llm_service import prompts
from llm_service import repository as repo
from llm_service.analysis.proposals import ProposalPipeline
from llm_service.analysis.schemas import (
    IMPACT_OUTPUT_SCHEMA,
    RECOMMEND_OUTPUT_SCHEMA,
    RELEVANCE_OUTPUT_SCHEMA,
    SITUATION_OUTPUT_SCHEMA,
)
from llm_service.analysis.structured import structured_chat
from llm_service.clients.ollama import OllamaClient
from llm_service.clients.platform import CallContext, PlatformClient
from llm_service.config import Settings
from llm_service.db import tenant_session
from llm_service.errors import ProblemError, not_found
from llm_service.jobs import JobRunner
from llm_service.logging import get_logger
from llm_service.metrics import REPORTS
from llm_service.project_context import ProjectContext, ProjectContextLoader
from llm_service.retrieval.search import RegulationSearch, RetrievedChunk

log = get_logger(__name__)

ReportType = Literal["rule_relevance", "fraud_situation", "regulation_impact", "recommend_rules"]
TITLES: dict[str, str] = {
    "rule_relevance": "Analisis relevansi rule",
    "fraud_situation": "Kondisi fraud & anomali",
    "regulation_impact": "Dampak regulasi",
    "recommend_rules": "Rekomendasi rule",
}


def _items(data: Any) -> list[Any]:
    if isinstance(data, dict):
        return list(data.get("items", []))
    return list(data or [])


def _slim_rule(r: dict[str, Any]) -> dict[str, Any]:
    keep = (
        "id",
        "code",
        "name",
        "description",
        "kind",
        "status",
        "typologies",
        "event_types",
        "risk_score",
        "action",
        "definition",
        "current_version",
        "stats_7d",
    )
    return {k: r[k] for k in keep if k in r}


class AnalysisService:
    def __init__(
        self,
        settings: Settings,
        ollama: OllamaClient,
        platform: PlatformClient,
        search: RegulationSearch,
        proposals: ProposalPipeline,
        contexts: ProjectContextLoader,
        jobs: JobRunner,
    ) -> None:
        self._s = settings
        self._ollama = ollama
        self._platform = platform
        self._search = search
        self._proposals = proposals
        self._contexts = contexts
        self._jobs = jobs

    # ------------------------------------------------------------------ entry
    async def start(self, ctx: CallContext, report_type: ReportType, params: dict[str, Any]) -> dict[str, Any]:
        if report_type == "regulation_impact":
            if not params.get("regulation_id"):
                raise ProblemError(422, "Validation failed", "regulation_id is required")
            await run_in_threadpool(self._require_regulation, ctx, uuid.UUID(str(params["regulation_id"])))
        pctx = await self._contexts.load(ctx)
        title = f"{TITLES[report_type]} — {datetime.now(UTC):%Y-%m-%d %H:%M} UTC"

        def _create() -> dict[str, Any]:
            with tenant_session(ctx.tenant_id) as s:
                return repo.create_report(
                    s,
                    tenant_id=ctx.tenant_id,
                    project_id=ctx.project_id,
                    report_type=report_type,
                    title=title,
                    params=params,
                    model=pctx.model,
                    created_by=ctx.actor_id,
                )

        report = await run_in_threadpool(_create)
        report_id = report["id"]
        self._jobs.submit(f"report-{report_id}", lambda: self._run(ctx, report_type, report_id, params))
        return {"report_id": str(report_id), "status": "running"}

    @staticmethod
    def _require_regulation(ctx: CallContext, reg_id: uuid.UUID) -> None:
        with tenant_session(ctx.tenant_id) as s:
            if repo.get_regulation(s, reg_id) is None:
                raise not_found("regulation")

    async def _run(
        self, ctx: CallContext, report_type: ReportType, report_id: uuid.UUID, params: dict[str, Any]
    ) -> None:
        try:
            pctx = await self._contexts.load(ctx)
            handler = {
                "rule_relevance": self._rule_relevance,
                "fraud_situation": self._fraud_situation,
                "regulation_impact": self._regulation_impact,
                "recommend_rules": self._recommend_rules,
            }
            content_md, structured = await handler[report_type](ctx, pctx, report_id, params)
            structured["generated_at"] = datetime.now(UTC).isoformat()

            def _finish() -> None:
                with tenant_session(ctx.tenant_id) as s:
                    repo.finish_report(s, report_id, content_md=content_md, structured=structured)

            await run_in_threadpool(_finish)
            REPORTS.labels(report_type, "done").inc()
        except Exception as exc:
            log.exception("report_failed", report_id=str(report_id), report_type=report_type)
            REPORTS.labels(report_type, "failed").inc()
            error = f"{type(exc).__name__}: {exc}"

            def _fail() -> None:
                with tenant_session(ctx.tenant_id) as s:
                    repo.fail_report(s, report_id, error)

            await run_in_threadpool(_fail)

    # ------------------------------------------------------------------ data gathering
    async def _safe(self, name: str, coro: Awaitable[Any], gaps: list[str], default: Any = None) -> Any:
        try:
            return await coro
        except Exception as exc:
            log.warning("analysis_source_unavailable", source=name, error=str(exc))
            gaps.append(name)
            return default

    async def _field_paths(self, ctx: CallContext, gaps: list[str]) -> list[str]:
        cat = await self._safe("field_catalog", self._platform.field_catalog(ctx), gaps, [])
        return [str(i["path"]) for i in _items(cat) if isinstance(i, dict) and "path" in i]

    async def _docs(
        self, ctx: CallContext, pctx: ProjectContext, query: str, k: int, regulation_ids: list[uuid.UUID] | None = None
    ) -> list[RetrievedChunk]:
        ids = regulation_ids if regulation_ids is not None else pctx.regulation_ids
        try:
            return await self._search.search(ctx.tenant_id, query, ids, k=k)
        except Exception as exc:
            log.warning("analysis_retrieval_failed", error=str(exc))
            return []

    async def _llm(self, pctx: ProjectContext, prompt: str, schema: dict[str, Any]) -> dict[str, Any]:
        return await structured_chat(
            self._ollama, [{"role": "user", "content": prompt}], schema, model=pctx.model, temperature=pctx.temperature
        )

    async def _make_proposals(
        self,
        ctx: CallContext,
        pctx: ProjectContext,
        report_id: uuid.UUID,
        recommendations: list[dict[str, Any]],
        rules: list[dict[str, Any]],
        field_paths: list[str],
        retrieved: list[RetrievedChunk],
        limit: int,
    ) -> list[dict[str, Any]]:
        by_code = {str(r.get("code")): str(r.get("id")) for r in rules}
        cites = [c.citation() for c in retrieved]
        outcomes = []
        for rec in recommendations[:limit]:
            outcome = await self._proposals.propose(
                ctx,
                rec,
                field_paths=field_paths,
                rule_ids_by_code=by_code,
                retrieved=cites,
                report_id=str(report_id),
                model=pctx.model,
            )
            outcomes.append({**outcome.as_dict(), "rationale": rec.get("rationale", "")})
        return outcomes

    @staticmethod
    def _proposals_md(outcomes: list[dict[str, Any]], language: str) -> str:
        if not outcomes:
            return ""
        head = (
            "## Proposal yang dibuat (menunggu persetujuan)"
            if language == "id"
            else "## Proposals created (pending approval)"
        )
        lines = [head, "", "| Rule | Tipe | Status | Catatan |", "|---|---|---|---|"]
        for o in outcomes:
            note = o.get("error") or (
                "perbaikan otomatis: " + str(o.get("repair_attempts")) if o.get("repair_attempts") else ""
            )
            lines.append(f"| {o.get('rule_code') or '-'} | {o.get('proposal_type')} | {o['status']} | {note} |")
        return "\n".join(lines)

    # ------------------------------------------------------------------ report types
    async def _rule_relevance(
        self, ctx: CallContext, pctx: ProjectContext, report_id: uuid.UUID, params: dict[str, Any]
    ) -> tuple[str, dict[str, Any]]:
        gaps: list[str] = []
        since = int(params.get("since_days", 30))
        rules = [
            r
            for r in _items(await self._safe("rules", self._platform.list_rules(ctx, {"page_size": 200}), gaps, []))
            if isinstance(r, dict)
        ]
        wanted = {str(x) for x in params.get("rule_ids") or []}
        if wanted:
            rules = [r for r in rules if str(r.get("id")) in wanted]
        perf = await self._safe("rules_performance", self._platform.rules_performance(ctx, since), gaps, [])
        drift = await self._safe("feature_drift", self._platform.feature_drift(ctx), gaps, [])
        query = "kewajiban pemantauan transaksi deteksi fraud " + " ".join(str(r.get("name", "")) for r in rules[:15])
        docs = await self._docs(ctx, pctx, query, k=8)
        prompt = prompts.render(
            "rule_relevance.md.j2",
            **pctx.prompt_vars(),
            since_days=since,
            rules=[_slim_rule(r) for r in rules],
            performance=perf,
            drift=drift,
            documents=[d.as_document() for d in docs],
        )
        out = await self._llm(pctx, prompt, RELEVANCE_OUTPUT_SCHEMA)
        outcomes: list[dict[str, Any]] = []
        if params.get("create_proposals", True):
            retire = [
                {
                    "proposal_type": "retire_rule",
                    "target_rule_code": v["rule_code"],
                    "rationale": v["rationale"],
                    "citations": v.get("citations") or [],
                    "rule": None,
                    "evidence": {"verdict": "retire"},
                }
                for v in out.get("verdicts", [])
                if v.get("verdict") == "retire"
            ]
            outcomes = await self._make_proposals(ctx, pctx, report_id, retire, rules, [], docs, limit=10)
        structured = {
            **out,
            "proposals": outcomes,
            "data_gaps": gaps,
            "citations": [d.citation() for d in docs],
            "prompt": prompts.version_of("rule_relevance.md.j2"),
            "since_days": since,
        }
        md = "\n\n".join(x for x in [out["summary_md"], self._proposals_md(outcomes, pctx.language)] if x)
        return md, structured

    async def _fraud_situation(
        self, ctx: CallContext, pctx: ProjectContext, report_id: uuid.UUID, params: dict[str, Any]
    ) -> tuple[str, dict[str, Any]]:
        gaps: list[str] = []
        since = int(params.get("since_days", 7))
        overview = await self._safe("analytics_overview", self._platform.analytics_overview(ctx, since), gaps, {})
        typologies = await self._safe("typologies", self._platform.typology_stats(ctx), gaps, [])
        clusters = await self._safe("anomaly_clusters", self._platform.anomaly_clusters(ctx), gaps, [])
        components = await self._safe("graph_components", self._platform.graph_components(ctx), gaps, [])
        communities = await self._safe("graph_communities", self._platform.graph_communities(ctx), gaps, [])
        drift = await self._safe("feature_drift", self._platform.feature_drift(ctx), gaps, [])
        prompt = prompts.render(
            "fraud_situation.md.j2",
            **pctx.prompt_vars(),
            since_days=since,
            overview=overview,
            typologies=typologies,
            clusters=clusters,
            components=_items(components)[:30],
            communities=_items(communities)[:30],
            drift=drift,
        )
        out = await self._llm(pctx, prompt, SITUATION_OUTPUT_SCHEMA)
        structured = {
            **out,
            "data_gaps": gaps,
            "prompt": prompts.version_of("fraud_situation.md.j2"),
            "since_days": since,
        }
        return out["summary_md"], structured

    async def _regulation_impact(
        self, ctx: CallContext, pctx: ProjectContext, report_id: uuid.UUID, params: dict[str, Any]
    ) -> tuple[str, dict[str, Any]]:
        gaps: list[str] = []
        reg_id = uuid.UUID(str(params["regulation_id"]))

        def _load() -> tuple[dict[str, Any], list[dict[str, Any]]]:
            with tenant_session(ctx.tenant_id) as s:
                reg = repo.get_regulation(s, reg_id)
                if reg is None:
                    raise not_found("regulation")
                return reg, repo.list_changes(s, reg_id)

        reg, changes = await run_in_threadpool(_load)
        changed = (changes[0]["changed_sections"] if changes else [])[:30]
        query = (
            " ".join((c.get("after") or c.get("before") or "")[:300] for c in changed[:8])
            or f"{reg['title']} kewajiban fraud"
        )
        own_docs = await self._docs(ctx, pctx, query, k=8, regulation_ids=[reg_id])
        other_docs = await self._docs(ctx, pctx, query, k=4)
        docs = own_docs + [d for d in other_docs if d.hit.chunk_id not in {o.hit.chunk_id for o in own_docs}]
        rules = [
            r
            for r in _items(await self._safe("rules", self._platform.list_rules(ctx, {"page_size": 200}), gaps, []))
            if isinstance(r, dict)
        ]
        field_paths = await self._field_paths(ctx, gaps)
        max_rules = int(params.get("max_rules", 5))
        prompt = prompts.render(
            "regulation_impact.md.j2",
            **pctx.prompt_vars(),
            regulation={"code": reg["code"], "version": reg["version"], "title": reg["title"]},
            changes=changed,
            rules=[_slim_rule(r) for r in rules],
            field_paths=field_paths,
            documents=[d.as_document() for d in docs],
            max_rules=max_rules,
        )
        out = await self._llm(pctx, prompt, IMPACT_OUTPUT_SCHEMA)
        outcomes = await self._make_proposals(
            ctx, pctx, report_id, out.get("recommendations", []), rules, field_paths, docs, limit=max_rules
        )
        structured = {
            **out,
            "proposals": outcomes,
            "data_gaps": gaps,
            "regulation_id": str(reg_id),
            "citations": [d.citation() for d in docs],
            "prompt": prompts.version_of("regulation_impact.md.j2"),
        }
        md = "\n\n".join(x for x in [out["summary_md"], self._proposals_md(outcomes, pctx.language)] if x)
        return md, structured

    async def _recommend_rules(
        self, ctx: CallContext, pctx: ProjectContext, report_id: uuid.UUID, params: dict[str, Any]
    ) -> tuple[str, dict[str, Any]]:
        gaps: list[str] = []
        since = int(params.get("since_days", 30))
        max_rules = int(params.get("max_rules", 5))
        focus = params.get("focus")
        rules = [
            r
            for r in _items(await self._safe("rules", self._platform.list_rules(ctx, {"page_size": 200}), gaps, []))
            if isinstance(r, dict)
        ]
        perf = await self._safe("rules_performance", self._platform.rules_performance(ctx, since), gaps, [])
        overview = await self._safe("analytics_overview", self._platform.analytics_overview(ctx, since), gaps, {})
        typologies = await self._safe("typologies", self._platform.typology_stats(ctx), gaps, [])
        clusters = await self._safe("anomaly_clusters", self._platform.anomaly_clusters(ctx), gaps, [])
        drift = await self._safe("feature_drift", self._platform.feature_drift(ctx), gaps, [])
        components = await self._safe("graph_components", self._platform.graph_components(ctx), gaps, [])
        field_paths = await self._field_paths(ctx, gaps)
        docs = await self._docs(
            ctx, pctx, f"{focus or 'fraud'} kewajiban pencegahan deteksi transaksi mencurigakan", k=6
        )
        prompt = prompts.render(
            "recommend_rules.md.j2",
            **pctx.prompt_vars(),
            since_days=since,
            focus=focus,
            max_rules=max_rules,
            rules=[_slim_rule(r) for r in rules],
            performance=perf,
            overview=overview,
            typologies=typologies,
            clusters=clusters,
            drift=drift,
            components=_items(components)[:20],
            field_paths=field_paths,
            documents=[d.as_document() for d in docs],
        )
        out = await self._llm(pctx, prompt, RECOMMEND_OUTPUT_SCHEMA)
        outcomes = await self._make_proposals(
            ctx, pctx, report_id, out.get("recommendations", []), rules, field_paths, docs, limit=max_rules
        )
        structured = {
            **out,
            "proposals": outcomes,
            "data_gaps": gaps,
            "citations": [d.citation() for d in docs],
            "prompt": prompts.version_of("recommend_rules.md.j2"),
            "since_days": since,
            "focus": focus,
        }
        md = "\n\n".join(x for x in [out["summary_md"], self._proposals_md(outcomes, pctx.language)] if x)
        return md, structured

    # ------------------------------------------------------------------ reads
    def get(self, ctx: CallContext, report_id: uuid.UUID) -> dict[str, Any]:
        with tenant_session(ctx.tenant_id) as s:
            row = repo.get_report(s, ctx.project_id, report_id)
        if row is None:
            raise not_found("report")
        return row

    def list(
        self, ctx: CallContext, report_type: str | None, limit: int, offset: int
    ) -> tuple[list[dict[str, Any]], int]:
        with tenant_session(ctx.tenant_id) as s:
            return repo.list_reports(s, ctx.project_id, report_type=report_type, limit=limit, offset=offset)
