"""Tenant regulation/policy library and project attachment."""

from __future__ import annotations

import uuid
from datetime import date
from typing import Annotated, Any, Literal

from fastapi import APIRouter, Depends, File, Form, Path, Query, UploadFile, status
from pydantic import BaseModel, Field
from starlette.concurrency import run_in_threadpool

from llm_service import repository as repo
from llm_service.api.common import PageParams, call_ctx, page_params, paged
from llm_service.auth import Principal, ProjectScope, get_principal, project_scope
from llm_service.container import Container, get_container
from llm_service.db import tenant_session
from llm_service.errors import ProblemError, not_found
from llm_service.regulations.service import UploadMeta

router = APIRouter(tags=["regulations"])


def _require_uploader(principal: Principal, tid: uuid.UUID) -> None:
    principal.require_tenant(tid)
    if principal.kind == "service" or principal.platform_admin or principal.tenant_role == "tenant_admin":
        return
    if not any(r in ("analyst", "approver", "project_admin") for r in principal.projects.values()):
        raise ProblemError(403, "Forbidden", "requires tenant_admin or analyst role in a project")


@router.get("/api/v1/tenants/{tid}/regulations")
async def list_regulations(
    tid: Annotated[uuid.UUID, Path()],
    principal: Annotated[Principal, Depends(get_principal)],
    p: Annotated[PageParams, Depends(page_params)],
    status_: Annotated[str | None, Query(alias="status")] = None,
    q: str | None = None,
) -> dict[str, Any]:
    principal.require_tenant(tid)

    def _list() -> tuple[list[dict[str, Any]], int]:
        with tenant_session(tid) as s:
            return repo.list_regulations(s, tid, status=status_, q=q, limit=p.page_size, offset=p.offset)

    items, total = await run_in_threadpool(_list)
    return paged(items, total, p)


@router.post("/api/v1/tenants/{tid}/regulations", status_code=status.HTTP_202_ACCEPTED)
async def upload_regulation(
    tid: Annotated[uuid.UUID, Path()],
    principal: Annotated[Principal, Depends(get_principal)],
    c: Annotated[Container, Depends(get_container)],
    file: Annotated[UploadFile, File()],
    code: Annotated[str, Form(min_length=2, max_length=80)],
    title: Annotated[str, Form(min_length=2, max_length=300)],
    issuer: Annotated[str, Form(min_length=1, max_length=80)],
    doc_type: Annotated[Literal["regulation", "internal_policy", "sop", "other"], Form()] = "regulation",
    effective_date: Annotated[date | None, Form()] = None,
    supersedes_id: Annotated[uuid.UUID | None, Form()] = None,
) -> dict[str, Any]:
    _require_uploader(principal, tid)
    data = await file.read()
    meta = UploadMeta(
        code=code.strip(),
        title=title.strip(),
        doc_type=doc_type,
        issuer=issuer.strip(),
        effective_date=effective_date,
        supersedes_id=supersedes_id,
    )
    return await c.regulations.upload(tid, data, file.filename or "document", meta, principal.user_id)


@router.get("/api/v1/tenants/{tid}/regulations/{reg_id}")
async def get_regulation(
    tid: Annotated[uuid.UUID, Path()],
    reg_id: Annotated[uuid.UUID, Path()],
    principal: Annotated[Principal, Depends(get_principal)],
) -> dict[str, Any]:
    principal.require_tenant(tid)

    def _get() -> dict[str, Any]:
        with tenant_session(tid) as s:
            reg = repo.get_regulation(s, reg_id)
            if reg is None:
                raise not_found("regulation")
            return {**reg, "changes": repo.list_changes(s, reg_id)}

    return await run_in_threadpool(_get)


@router.delete("/api/v1/tenants/{tid}/regulations/{reg_id}", status_code=status.HTTP_204_NO_CONTENT)
async def delete_regulation(
    tid: Annotated[uuid.UUID, Path()],
    reg_id: Annotated[uuid.UUID, Path()],
    principal: Annotated[Principal, Depends(get_principal)],
    c: Annotated[Container, Depends(get_container)],
) -> None:
    principal.require_tenant(tid, admin=True)
    await c.regulations.delete(tid, reg_id)


# ---------------------------------------------------------------------------- project attachment & search
class AttachBody(BaseModel):
    regulation_ids: list[uuid.UUID] = Field(default_factory=list, max_length=500)


class SearchBody(BaseModel):
    query: str = Field(min_length=1, max_length=2000)
    k: int = Field(6, ge=1, le=20)


@router.get("/api/v1/projects/{pid}/llm/regulations")
async def list_attached(scope: Annotated[ProjectScope, Depends(project_scope("viewer"))]) -> dict[str, Any]:
    def _list() -> list[dict[str, Any]]:
        with tenant_session(scope.tenant_id) as s:
            return repo.list_attached(s, scope.project_id)

    items = await run_in_threadpool(_list)
    return {"items": items, "regulation_ids": [str(i["id"]) for i in items]}


@router.put("/api/v1/projects/{pid}/llm/regulations")
async def set_attached(
    body: AttachBody, scope: Annotated[ProjectScope, Depends(project_scope("project_admin"))]
) -> dict[str, Any]:
    ids = list(dict.fromkeys(body.regulation_ids))

    def _set() -> list[dict[str, Any]]:
        with tenant_session(scope.tenant_id) as s:
            found = {r["id"] for r in repo.regulations_by_ids(s, ids)}  # RLS: only this tenant's documents
            missing = [str(i) for i in ids if i not in found]
            if missing:
                raise ProblemError(
                    422,
                    "Validation failed",
                    "unknown regulation ids",
                    errors=[{"field": "regulation_ids", "message": m} for m in missing],
                )
            repo.set_attached(
                s,
                tenant_id=scope.tenant_id,
                project_id=scope.project_id,
                regulation_ids=ids,
                actor=scope.principal.user_id,
            )
            return repo.list_attached(s, scope.project_id)

    items = await run_in_threadpool(_set)
    return {"items": items, "regulation_ids": [str(i["id"]) for i in items]}


@router.post("/api/v1/projects/{pid}/llm/regulations/search")
async def search(
    body: SearchBody,
    scope: Annotated[ProjectScope, Depends(project_scope("viewer"))],
    c: Annotated[Container, Depends(get_container)],
) -> dict[str, Any]:
    pctx = await c.contexts.load(call_ctx(scope))
    hits = await c.search.search(scope.tenant_id, body.query, pctx.regulation_ids, k=body.k)
    return {"items": [{**h.citation(excerpt_chars=100000)} for h in hits]}
