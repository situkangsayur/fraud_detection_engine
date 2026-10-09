"""Internal (service-to-service) endpoints."""

from __future__ import annotations

from typing import Annotated, Any

from fastapi import APIRouter, Depends
from pydantic import BaseModel, Field

from llm_service.auth import Principal, require_internal
from llm_service.container import Container, get_container
from llm_service.mapping.suggest import suggest_mapping

router = APIRouter(tags=["internal"])


class MappingSuggestBody(BaseModel):
    fields: list[dict[str, Any]] = Field(min_length=1, max_length=500)
    canonical_fields: list[Any] = Field(min_length=1, max_length=200)


@router.post("/v1/mapping/suggest")
async def mapping_suggest(
    body: MappingSuggestBody,
    _: Annotated[Principal, Depends(require_internal)],
    c: Annotated[Container, Depends(get_container)],
) -> dict[str, Any]:
    return {"suggestions": await suggest_mapping(c.ollama, body.fields, body.canonical_fields)}
