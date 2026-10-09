"""Chat, conversations, analyses and reports (project scoped)."""

from __future__ import annotations

import json
import uuid
from collections.abc import AsyncIterator
from typing import Annotated, Any, Literal

from fastapi import APIRouter, Depends, Path, Query, Request, status
from fastapi.responses import StreamingResponse
from pydantic import BaseModel, Field
from starlette.concurrency import run_in_threadpool

from llm_service.api.common import PageParams, call_ctx, page_params, paged
from llm_service.auth import ProjectScope, project_scope
from llm_service.container import Container, get_container
from llm_service.logging import get_logger

router = APIRouter(tags=["assistant"])
log = get_logger(__name__)


class ChatBody(BaseModel):
    conversation_id: uuid.UUID | None = None
    message: str = Field(min_length=1, max_length=8000)


def _rid(request: Request) -> str | None:
    return request.headers.get("x-request-id")


@router.post("/api/v1/projects/{pid}/llm/chat")
async def chat(
    body: ChatBody,
    request: Request,
    scope: Annotated[ProjectScope, Depends(project_scope("viewer"))],
    c: Annotated[Container, Depends(get_container)],
) -> dict[str, Any]:
    return await c.chat.chat(call_ctx(scope, _rid(request)), body.conversation_id, body.message)


def _sse(event: str, data: dict[str, Any]) -> str:
    return f"event: {event}\ndata: {json.dumps(data, default=str, ensure_ascii=False)}\n\n"


@router.post("/api/v1/projects/{pid}/llm/chat/stream")
async def chat_stream(
    body: ChatBody,
    request: Request,
    scope: Annotated[ProjectScope, Depends(project_scope("viewer"))],
    c: Annotated[Container, Depends(get_container)],
) -> StreamingResponse:
    ctx = call_ctx(scope, _rid(request))

    async def _events() -> AsyncIterator[str]:
        try:
            async for ev in c.chat.chat_stream(ctx, body.conversation_id, body.message):
                kind = ev.pop("type")
                yield _sse(kind, ev)
        except Exception as exc:
            log.exception("chat_stream_failed")
            yield _sse("error", {"detail": str(exc)[:300]})

    return StreamingResponse(
        _events(), media_type="text/event-stream", headers={"Cache-Control": "no-cache", "X-Accel-Buffering": "no"}
    )


@router.get("/api/v1/projects/{pid}/llm/conversations")
async def conversations(
    scope: Annotated[ProjectScope, Depends(project_scope("viewer"))],
    c: Annotated[Container, Depends(get_container)],
    p: Annotated[PageParams, Depends(page_params)],
) -> dict[str, Any]:
    items, total = await run_in_threadpool(c.chat.list_conversations, call_ctx(scope), p.page_size, p.offset)
    return paged(items, total, p)


@router.get("/api/v1/projects/{pid}/llm/conversations/{conversation_id}")
async def conversation(
    conversation_id: Annotated[uuid.UUID, Path()],
    scope: Annotated[ProjectScope, Depends(project_scope("viewer"))],
    c: Annotated[Container, Depends(get_container)],
) -> dict[str, Any]:
    return await run_in_threadpool(c.chat.get_conversation, call_ctx(scope), conversation_id)


# ---------------------------------------------------------------------------- analyses
class RuleRelevanceBody(BaseModel):
    rule_ids: list[uuid.UUID] | None = None
    since_days: int = Field(30, ge=1, le=365)
    create_proposals: bool = True


class FraudSituationBody(BaseModel):
    since_days: int = Field(7, ge=1, le=365)


class RegulationImpactBody(BaseModel):
    regulation_id: uuid.UUID
    max_rules: int = Field(5, ge=0, le=20)


class RecommendRulesBody(BaseModel):
    focus: str | None = Field(None, max_length=60)
    since_days: int = Field(30, ge=1, le=365)
    max_rules: int = Field(5, ge=1, le=20)


_ANALYST = Depends(project_scope("analyst"))


@router.post("/api/v1/projects/{pid}/llm/analysis/rule-relevance", status_code=status.HTTP_202_ACCEPTED)
async def rule_relevance(
    body: RuleRelevanceBody,
    request: Request,
    scope: Annotated[ProjectScope, _ANALYST],
    c: Annotated[Container, Depends(get_container)],
) -> dict[str, Any]:
    return await c.analysis.start(call_ctx(scope, _rid(request)), "rule_relevance", body.model_dump(mode="json"))


@router.post("/api/v1/projects/{pid}/llm/analysis/fraud-situation", status_code=status.HTTP_202_ACCEPTED)
async def fraud_situation(
    body: FraudSituationBody,
    request: Request,
    scope: Annotated[ProjectScope, _ANALYST],
    c: Annotated[Container, Depends(get_container)],
) -> dict[str, Any]:
    return await c.analysis.start(call_ctx(scope, _rid(request)), "fraud_situation", body.model_dump(mode="json"))


@router.post("/api/v1/projects/{pid}/llm/analysis/regulation-impact", status_code=status.HTTP_202_ACCEPTED)
async def regulation_impact(
    body: RegulationImpactBody,
    request: Request,
    scope: Annotated[ProjectScope, _ANALYST],
    c: Annotated[Container, Depends(get_container)],
) -> dict[str, Any]:
    return await c.analysis.start(call_ctx(scope, _rid(request)), "regulation_impact", body.model_dump(mode="json"))


@router.post("/api/v1/projects/{pid}/llm/analysis/recommend-rules", status_code=status.HTTP_202_ACCEPTED)
async def recommend_rules(
    body: RecommendRulesBody,
    request: Request,
    scope: Annotated[ProjectScope, _ANALYST],
    c: Annotated[Container, Depends(get_container)],
) -> dict[str, Any]:
    return await c.analysis.start(call_ctx(scope, _rid(request)), "recommend_rules", body.model_dump(mode="json"))


@router.get("/api/v1/projects/{pid}/llm/reports")
async def reports(
    scope: Annotated[ProjectScope, Depends(project_scope("viewer"))],
    c: Annotated[Container, Depends(get_container)],
    p: Annotated[PageParams, Depends(page_params)],
    report_type: Annotated[
        Literal["rule_relevance", "fraud_situation", "regulation_impact", "recommend_rules"] | None, Query()
    ] = None,
) -> dict[str, Any]:
    items, total = await run_in_threadpool(c.analysis.list, call_ctx(scope), report_type, p.page_size, p.offset)
    return paged(items, total, p)


@router.get("/api/v1/projects/{pid}/llm/reports/{report_id}")
async def report(
    report_id: Annotated[uuid.UUID, Path()],
    scope: Annotated[ProjectScope, Depends(project_scope("viewer"))],
    c: Annotated[Container, Depends(get_container)],
) -> dict[str, Any]:
    return await run_in_threadpool(c.analysis.get, call_ctx(scope), report_id)
