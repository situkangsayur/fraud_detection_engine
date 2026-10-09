"""LLM-assisted field mapping suggestions for ingest-service (structured output, post-validated)."""

from __future__ import annotations

from typing import Any

from llm_service import prompts
from llm_service.analysis.schemas import MAPPING_OUTPUT_SCHEMA
from llm_service.analysis.structured import structured_chat
from llm_service.clients.ollama import OllamaClient

MULTI_USE_TARGETS = {"instrument_fingerprint", "card_bin", "card_last4"}


def sanitize(suggestions: list[dict[str, Any]], source_paths: set[str], canonical: set[str]) -> list[dict[str, Any]]:
    """Drop hallucinated paths/targets and enforce one source per canonical target (highest confidence wins)."""
    valid = [s for s in suggestions if s.get("source_path") in source_paths]
    for s in valid:
        if s.get("target") not in canonical:
            s["target"] = None
        s["confidence"] = max(0.0, min(1.0, float(s.get("confidence", 0))))
    best: dict[str, dict[str, Any]] = {}
    for s in sorted(valid, key=lambda x: -x["confidence"]):
        t = s["target"]
        if t is None or t in MULTI_USE_TARGETS:
            continue
        if t in best:
            s["target"] = None
            s["reason"] = f"{s.get('reason', '')} (target already taken by {best[t]['source_path']})".strip()
        else:
            best[t] = s
    return valid


async def suggest_mapping(
    ollama: OllamaClient, fields: list[dict[str, Any]], canonical_fields: list[Any]
) -> list[dict[str, Any]]:
    canonical = {c if isinstance(c, str) else str(c.get("name") or c.get("path")) for c in canonical_fields}
    slim = [
        {
            "path": f.get("path"),
            "inferred_type": f.get("inferred_type"),
            "sample_values": (f.get("sample_values") or [])[:5],
        }
        for f in fields
    ]
    prompt = prompts.render("mapping_suggest.md.j2", fields=slim, canonical_fields=sorted(canonical))
    out = await structured_chat(ollama, [{"role": "user", "content": prompt}], MAPPING_OUTPUT_SCHEMA, temperature=0.0)
    return sanitize(out["suggestions"], {str(f.get("path")) for f in fields}, canonical)
