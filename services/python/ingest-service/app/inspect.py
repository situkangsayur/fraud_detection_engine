"""Inspection = schema inference + mapping suggestion + preview (+ optional LLM assist)."""

from __future__ import annotations

import uuid
from typing import Any

import httpx

from app.config import Settings
from app.inference.schema import infer_schema
from app.logging import log
from app.mapping.suggest import apply_llm_suggestions, canonical_targets, suggest_mapping
from app.readers import jsonable


def llm_suggest(
    settings: Settings, tenant_id: uuid.UUID, project_id: uuid.UUID, fields: list[dict[str, Any]]
) -> list[dict[str, Any]]:
    """Ask llm-service for suggestions; returns [] on any failure (LLM assist is best-effort)."""
    try:
        resp = httpx.post(
            f"{settings.llm_service_url}/v1/mapping/suggest",
            json={
                "fields": [
                    {
                        "path": f["path"],
                        "inferred_type": f["inferred_type"],
                        "sample_values": f.get("sample_values", []),
                    }
                    for f in fields
                ],
                "canonical_fields": canonical_targets(),
            },
            headers={
                "Authorization": f"Bearer {settings.internal_api_token}",
                "X-Tenant-Id": str(tenant_id),
                "X-Project-Id": str(project_id),
            },
            timeout=60.0,
        )
        resp.raise_for_status()
        return list(resp.json().get("suggestions", []))
    except (httpx.HTTPError, ValueError) as e:
        log.warning("llm_mapping_suggest_unavailable", error=str(e)[:200])
        return []


def inspect_records(
    settings: Settings,
    records: list[dict[str, Any]],
    *,
    tenant_id: uuid.UUID,
    project_id: uuid.UUID,
    default_event_type: str | None,
    use_llm: bool,
) -> dict[str, Any]:
    fields = infer_schema(records)
    result = suggest_mapping(fields, records, default_event_type)
    llm_used = False
    if use_llm:
        low = [f for f in fields if f["path"] in set(result["unmapped_fields"])]
        if low:
            sugg = llm_suggest(settings, tenant_id, project_id, low)
            if sugg:
                result = apply_llm_suggestions(result, fields, sugg)
                llm_used = True
    return {
        "schema": {"fields": fields, "sampled_rows": len(records)},
        "suggested_mapping": result["suggested_mapping"],
        "confidence": result["confidence"],
        "unmapped_fields": result["unmapped_fields"],
        "notes": result["notes"],
        "llm_used": llm_used,
        "preview": [jsonable(r) for r in records[: settings.preview_rows]],
    }
