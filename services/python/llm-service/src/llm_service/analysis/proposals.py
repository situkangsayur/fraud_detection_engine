"""Turn an LLM rule recommendation into a *pending* rule proposal.

Flow: validate with rule-service → if invalid, ask the LLM to repair (≤ ``rule_repair_attempts``) → create the
proposal via rule-service (which backtests it). The LLM never activates rules; approval is a human decision.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any

from llm_service import prompts
from llm_service.analysis.schemas import RULE_ENVELOPE_SCHEMA
from llm_service.analysis.structured import StructuredOutputError, structured_chat
from llm_service.clients.ollama import OllamaClient
from llm_service.clients.platform import CallContext, PlatformClient, PlatformError
from llm_service.logging import get_logger
from llm_service.metrics import PROPOSALS

log = get_logger(__name__)


@dataclass
class ProposalOutcome:
    status: str  # created | invalid | failed | skipped
    proposal_id: str | None = None
    rule_code: str | None = None
    proposal_type: str | None = None
    validation: dict[str, Any] | None = None
    repair_attempts: int = 0
    error: str | None = None
    backtest: Any = None
    extra: dict[str, Any] = field(default_factory=dict)

    def as_dict(self) -> dict[str, Any]:
        return {k: v for k, v in self.__dict__.items() if v not in (None, {}, [])}


def _errors(validation: dict[str, Any]) -> list[Any]:
    errs = validation.get("errors")
    if errs:
        return list(errs)
    if "detail" in validation:  # problem+json 422
        return [validation.get("detail"), *(validation.get("errors") or [])]
    return []


def resolve_citations(llm_citations: list[dict[str, Any]], retrieved: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Attach regulation/chunk ids to LLM citations when they match a retrieved chunk (code + section)."""
    out: list[dict[str, Any]] = []
    for c in llm_citations or []:
        match = next(
            (
                r
                for r in retrieved
                if r.get("code") == c.get("code")
                and (c.get("section", "") in r.get("section", "") or r.get("section", "") in c.get("section", ""))
            ),
            None,
        )
        item = dict(c)
        if match:
            item.update(
                {
                    "regulation_id": match["regulation_id"],
                    "chunk_id": match["chunk_id"],
                    "version": match.get("version"),
                }
            )
            item.setdefault("excerpt", match.get("excerpt", ""))
        out.append(item)
    return out


class ProposalPipeline:
    def __init__(self, platform: PlatformClient, ollama: OllamaClient, repair_attempts: int = 2) -> None:
        self._platform = platform
        self._ollama = ollama
        self._repairs = repair_attempts

    async def validate_and_repair(
        self, ctx: CallContext, envelope: dict[str, Any], field_paths: list[str], model: str | None
    ) -> tuple[dict[str, Any], dict[str, Any], int]:
        attempts = 0
        validation = await self._platform.validate_rule(ctx, envelope)
        while not validation.get("valid") and attempts < self._repairs:
            attempts += 1
            prompt = prompts.render(
                "rule_repair.md.j2", rule=envelope, errors=_errors(validation), field_paths=field_paths
            )
            try:
                envelope = await structured_chat(
                    self._ollama, [{"role": "user", "content": prompt}], RULE_ENVELOPE_SCHEMA, model=model
                )
            except StructuredOutputError as exc:
                log.warning("rule_repair_unparseable", error=str(exc))
                continue
            validation = await self._platform.validate_rule(ctx, envelope)
        return envelope, validation, attempts

    async def propose(
        self,
        ctx: CallContext,
        rec: dict[str, Any],
        *,
        field_paths: list[str],
        rule_ids_by_code: dict[str, str],
        retrieved: list[dict[str, Any]],
        report_id: str | None,
        model: str,
    ) -> ProposalOutcome:
        ptype = rec.get("proposal_type", "new_rule")
        target_code = rec.get("target_rule_code")
        target_id = rule_ids_by_code.get(target_code) if target_code else None
        if ptype in ("modify_rule", "retire_rule") and target_id is None:
            PROPOSALS.labels("skipped").inc()
            return ProposalOutcome(
                "skipped", proposal_type=ptype, rule_code=target_code, error=f"unknown target rule code {target_code!r}"
            )
        envelope = rec.get("rule")
        validation: dict[str, Any] = {}
        attempts = 0
        if ptype != "retire_rule":
            if not isinstance(envelope, dict):
                PROPOSALS.labels("skipped").inc()
                return ProposalOutcome("skipped", proposal_type=ptype, error="recommendation has no rule")
            try:
                envelope, validation, attempts = await self.validate_and_repair(ctx, envelope, field_paths, model)
            except PlatformError as exc:
                PROPOSALS.labels("failed").inc()
                return ProposalOutcome("failed", proposal_type=ptype, error=f"validation call failed: {exc}")
            if not validation.get("valid"):
                PROPOSALS.labels("invalid").inc()
                return ProposalOutcome(
                    "invalid",
                    proposal_type=ptype,
                    rule_code=envelope.get("code"),
                    validation=validation,
                    repair_attempts=attempts,
                )
        else:
            envelope = None
        payload = {
            "source": "llm",
            "proposal_type": ptype,
            "target_rule_id": target_id,
            "definition": envelope,
            "rationale": rec.get("rationale", ""),
            "citations": resolve_citations(rec.get("citations") or [], retrieved),
            "evidence": rec.get("evidence") or {},
            "report_id": report_id,
            "llm_model": model,
        }
        try:
            created = await self._platform.create_proposal(ctx, payload)
        except PlatformError as exc:
            PROPOSALS.labels("failed").inc()
            return ProposalOutcome("failed", proposal_type=ptype, error=str(exc), repair_attempts=attempts)
        PROPOSALS.labels("created").inc()
        return ProposalOutcome(
            "created",
            proposal_id=str(created.get("id")),
            proposal_type=ptype,
            rule_code=(envelope or {}).get("code") or target_code,
            validation=validation or None,
            repair_attempts=attempts,
            backtest=created.get("backtest"),
        )
