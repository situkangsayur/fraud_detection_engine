"""Public API (via gateway) — api-contract.md §F."""

from __future__ import annotations

import json
import re
import secrets
import uuid
from datetime import UTC, datetime, timedelta
from typing import Annotated, Any, Literal

from fastapi import APIRouter, Depends, Query, Request
from pydantic import BaseModel, Field

from app import repo
from app.config import Settings, get_settings
from app.connectors import sql as sql_conn
from app.db import tenant_session
from app.errors import ProblemError
from app.inspect import inspect_records
from app.metrics import INSPECTIONS
from app.readers import detect_format, estimate_rows, read_sample
from app.security import Principal, require_analyst, require_viewer

router = APIRouter(prefix="/api/v1/projects/{pid}")
SAFE_NAME = re.compile(r"[^A-Za-z0-9._-]+")


class SqlInspect(BaseModel):
    table: str | None = None
    query: str | None = None
    limit: int = Field(default=1000, ge=1, le=10000)


class JsonInspect(BaseModel):
    sql: SqlInspect | None = None
    records: list[dict[str, Any]] | None = Field(default=None, max_length=10000)


class JobCreate(BaseModel):
    upload_id: str | None = None
    mode: Literal["score", "load_only"] | None = None


def _source_or_404(pid: uuid.UUID, sid: uuid.UUID, tenant_id: uuid.UUID) -> dict[str, Any]:
    with tenant_session(tenant_id) as c:
        src = repo.get_data_source(c, pid, sid)
    if not src:
        raise ProblemError(404, "Not found", "data source not found")
    return src


async def _save_upload(
    request: Request, settings: Settings, tenant_id: uuid.UUID, pid: uuid.UUID, sid: uuid.UUID
) -> dict[str, Any]:
    form = await request.form(max_files=1)
    upload = form.get("file")
    if upload is None or isinstance(upload, str):
        raise ProblemError(
            422, "Validation failed", errors=[{"field": "file", "message": "file is required"}]
        )
    upload_id = secrets.token_urlsafe(18)
    name = SAFE_NAME.sub("_", upload.filename or "upload")[:120] or "upload"
    target_dir = settings.upload_dir / str(tenant_id) / upload_id
    target_dir.mkdir(parents=True, exist_ok=True)
    target = target_dir / name
    limit = settings.max_upload_mb * 1024 * 1024
    size = 0
    head = b""
    with target.open("wb") as out:
        while chunk := await upload.read(1024 * 1024):
            size += len(chunk)
            if size > limit:
                out.close()
                target.unlink(missing_ok=True)
                raise ProblemError(
                    413, "Payload too large", f"max upload size is {settings.max_upload_mb} MB"
                )
            if len(head) < 4096:
                head += chunk[: 4096 - len(head)]
            out.write(chunk)
    fmt = detect_format(name, head)
    row_est = estimate_rows(target, fmt)
    with tenant_session(tenant_id) as c:
        repo.insert_upload(
            c,
            id=upload_id,
            tenant_id=tenant_id,
            project_id=pid,
            data_source_id=sid,
            file_name=name,
            file_path=str(target),
            file_format=fmt,
            size_bytes=size,
            row_estimate=row_est,
            expires_at=datetime.now(UTC) + timedelta(hours=settings.upload_ttl_hours),
        )
    return {
        "upload_id": upload_id,
        "file_path": target,
        "file_format": fmt,
        "row_estimate": row_est,
        "size_bytes": size,
        "file_name": name,
    }


@router.post("/data-sources/{sid}/inspect")
async def inspect(
    pid: uuid.UUID,
    sid: uuid.UUID,
    request: Request,
    principal: Annotated[Principal, Depends(require_analyst)],
    settings: Annotated[Settings, Depends(get_settings)],
    use_llm: Annotated[bool, Query()] = False,
) -> dict[str, Any]:
    assert principal.tenant_id is not None
    src = _source_or_404(pid, sid, principal.tenant_id)
    ctype = request.headers.get("content-type", "")
    extra: dict[str, Any] = {}
    if ctype.startswith("multipart/form-data"):
        up = await _save_upload(request, settings, principal.tenant_id, pid, sid)
        records = read_sample(up["file_path"], up["file_format"], settings.inspect_sample_rows)
        extra = {k: up[k] for k in ("upload_id", "file_format", "row_estimate", "size_bytes", "file_name")}
        INSPECTIONS.labels(up["file_format"]).inc()
    else:
        try:
            body = JsonInspect.model_validate(json.loads(await request.body() or b"{}"))
        except (ValueError, json.JSONDecodeError) as e:
            raise ProblemError(422, "Validation failed", str(e)[:500]) from e
        if body.sql:
            if src["kind"] not in ("postgres", "mysql"):
                raise ProblemError(409, "Conflict", "SQL inspection requires a postgres/mysql data source")
            records = sql_conn.sample(src["connection"] or {}, body.sql.table, body.sql.query, body.sql.limit)
            INSPECTIONS.labels("sql").inc()
        elif body.records:
            records = body.records
            INSPECTIONS.labels("json").inc()
        else:
            raise ProblemError(422, "Validation failed", "provide a multipart file, {sql} or {records}")
    if not records:
        raise ProblemError(422, "Empty source", "no records found to inspect")
    result = inspect_records(
        settings,
        records,
        tenant_id=principal.tenant_id,
        project_id=pid,
        default_event_type=src.get("default_event_type"),
        use_llm=use_llm,
    )
    with tenant_session(principal.tenant_id) as c:
        repo.update_inferred_schema(
            c, sid, {**result["schema"], "inspected_at": datetime.now(UTC).isoformat()}
        )
    return {**extra, **result}


@router.post("/data-sources/{sid}/jobs", status_code=202)
def create_job(
    pid: uuid.UUID,
    sid: uuid.UUID,
    body: JobCreate,
    request: Request,
    principal: Annotated[Principal, Depends(require_analyst)],
) -> dict[str, Any]:
    assert principal.tenant_id is not None
    tid = principal.tenant_id
    with tenant_session(tid) as c:
        src = repo.get_data_source(c, pid, sid)
        if not src:
            raise ProblemError(404, "Not found", "data source not found")
        if not src["is_active"]:
            raise ProblemError(409, "Conflict", "data source is inactive")
        mapping = repo.get_active_mapping(c, sid)
        if not mapping:
            raise ProblemError(409, "Conflict", "activate a mapping before loading data")
        upload = None
        if body.upload_id:
            upload = repo.get_upload(c, pid, body.upload_id)
            if not upload or upload["data_source_id"] != sid:
                raise ProblemError(404, "Not found", "upload not found or expired")
        elif src["kind"] not in ("postgres", "mysql"):
            raise ProblemError(422, "Validation failed", "upload_id is required for file sources")
        created_by = uuid.UUID(principal.user_id) if principal.user_id and not principal.is_service else None
        job = repo.insert_job(
            c,
            tenant_id=tid,
            project_id=pid,
            source_id=sid,
            mode=body.mode or src["mode"],
            upload_id=body.upload_id,
            total_rows=upload["row_estimate"] if upload else None,
            created_by=created_by,
        )
    request.app.state.runner.submit(job, src, mapping["mapping"], upload, principal.user_id)
    return {"job_id": str(job["id"]), "status": job["status"]}


@router.get("/data-sources/{sid}/jobs")
def list_jobs(
    pid: uuid.UUID,
    sid: uuid.UUID,
    principal: Annotated[Principal, Depends(require_viewer)],
    page: Annotated[int, Query(ge=1)] = 1,
    page_size: Annotated[int, Query(ge=1, le=200)] = 50,
) -> dict[str, Any]:
    assert principal.tenant_id is not None
    with tenant_session(principal.tenant_id) as c:
        items, total = repo.list_jobs(c, pid, sid, page, page_size)
    return {"items": items, "total": total, "page": page, "page_size": page_size}


@router.get("/ingest-jobs/{job_id}")
def get_job(
    pid: uuid.UUID, job_id: uuid.UUID, principal: Annotated[Principal, Depends(require_viewer)]
) -> dict[str, Any]:
    assert principal.tenant_id is not None
    with tenant_session(principal.tenant_id) as c:
        job = repo.get_job(c, pid, job_id)
    if not job:
        raise ProblemError(404, "Not found", "job not found")
    return job


@router.post("/ingest-jobs/{job_id}/cancel")
def cancel_job(
    pid: uuid.UUID,
    job_id: uuid.UUID,
    request: Request,
    principal: Annotated[Principal, Depends(require_analyst)],
) -> dict[str, Any]:
    assert principal.tenant_id is not None
    with tenant_session(principal.tenant_id) as c:
        job = repo.get_job(c, pid, job_id)
        if not job:
            raise ProblemError(404, "Not found", "job not found")
        if job["status"] in ("done", "failed", "cancelled"):
            raise ProblemError(409, "Conflict", f"job already {job['status']}")
        repo.update_job(c, job_id, status="cancelled", finished_at=datetime.now(UTC))
    request.app.state.runner.cancel(job_id)
    return {"job_id": str(job_id), "status": "cancelled"}
