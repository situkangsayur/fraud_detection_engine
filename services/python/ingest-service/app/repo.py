"""Persistence (SQL only here). Every function takes a tenant-scoped connection (see app.db.tenant_session)."""

from __future__ import annotations

import json
import uuid
from datetime import datetime
from typing import Any

from sqlalchemy import text
from sqlalchemy.engine import Connection

JOB_COLUMNS = (
    "id, tenant_id, project_id, data_source_id, mode, upload_id, status, total_rows, processed_rows, "
    "accepted_rows, rejected_rows, error, created_by, created_at, started_at, finished_at"
)


def _row(r: Any) -> dict[str, Any]:
    return dict(r._mapping) if r is not None else {}


def get_data_source(conn: Connection, project_id: uuid.UUID, source_id: uuid.UUID) -> dict[str, Any] | None:
    r = conn.execute(
        text(
            "SELECT id, tenant_id, project_id, slug, name, kind, default_event_type, mode, connection, cursor_state, "
            "is_active FROM core.data_sources WHERE project_id = :p AND id = :s"
        ),
        {"p": project_id, "s": source_id},
    ).first()
    return _row(r) if r else None


def get_active_mapping(conn: Connection, source_id: uuid.UUID) -> dict[str, Any] | None:
    r = conn.execute(
        text(
            "SELECT version, mapping FROM core.data_source_mappings WHERE data_source_id = :s AND status = 'active'"
        ),
        {"s": source_id},
    ).first()
    return _row(r) if r else None


def update_inferred_schema(conn: Connection, source_id: uuid.UUID, schema: dict[str, Any]) -> None:
    conn.execute(
        text("UPDATE core.data_sources SET inferred_schema = CAST(:j AS jsonb) WHERE id = :s"),
        {"j": json.dumps(schema, default=str), "s": source_id},
    )


def update_cursor_state(conn: Connection, source_id: uuid.UUID, state: dict[str, Any]) -> None:
    conn.execute(
        text("UPDATE core.data_sources SET cursor_state = CAST(:j AS jsonb) WHERE id = :s"),
        {"j": json.dumps(state, default=str), "s": source_id},
    )


# ------------------------------------------------------------------ uploads
def insert_upload(conn: Connection, **u: Any) -> None:
    conn.execute(
        text(
            "INSERT INTO ingest.uploads (id, tenant_id, project_id, data_source_id, file_name, file_path, file_format, "
            "size_bytes, row_estimate, expires_at) VALUES (:id, :tenant_id, :project_id, :data_source_id, :file_name, "
            ":file_path, :file_format, :size_bytes, :row_estimate, :expires_at)"
        ),
        u,
    )


def get_upload(conn: Connection, project_id: uuid.UUID, upload_id: str) -> dict[str, Any] | None:
    r = conn.execute(
        text(
            "SELECT id, data_source_id, file_name, file_path, file_format, size_bytes, row_estimate, expires_at "
            "FROM ingest.uploads WHERE project_id = :p AND id = :u"
        ),
        {"p": project_id, "u": upload_id},
    ).first()
    return _row(r) if r else None


def delete_expired_uploads(conn: Connection, now: datetime) -> list[str]:
    rows = conn.execute(
        text("DELETE FROM ingest.uploads WHERE expires_at < :n RETURNING file_path"), {"n": now}
    ).fetchall()
    return [r[0] for r in rows]


# ------------------------------------------------------------------ jobs
def insert_job(
    conn: Connection,
    *,
    tenant_id: uuid.UUID,
    project_id: uuid.UUID,
    source_id: uuid.UUID,
    mode: str,
    upload_id: str | None,
    total_rows: int | None,
    created_by: uuid.UUID | None,
) -> dict[str, Any]:
    r = conn.execute(
        text(
            f"INSERT INTO ingest.jobs (tenant_id, project_id, data_source_id, mode, upload_id, total_rows, created_by) "
            f"VALUES (:t, :p, :s, :m, :u, :n, :c) RETURNING {JOB_COLUMNS}"
        ),
        {
            "t": tenant_id,
            "p": project_id,
            "s": source_id,
            "m": mode,
            "u": upload_id,
            "n": total_rows,
            "c": created_by,
        },
    ).first()
    return _row(r)


def get_job(conn: Connection, project_id: uuid.UUID, job_id: uuid.UUID) -> dict[str, Any] | None:
    r = conn.execute(
        text(f"SELECT {JOB_COLUMNS} FROM ingest.jobs WHERE project_id = :p AND id = :j"),
        {"p": project_id, "j": job_id},
    ).first()
    return _row(r) if r else None


def list_jobs(
    conn: Connection, project_id: uuid.UUID, source_id: uuid.UUID, page: int, page_size: int
) -> tuple[list[dict[str, Any]], int]:
    total = conn.execute(
        text("SELECT count(*) FROM ingest.jobs WHERE project_id = :p AND data_source_id = :s"),
        {"p": project_id, "s": source_id},
    ).scalar_one()
    rows = conn.execute(
        text(
            f"SELECT {JOB_COLUMNS} FROM ingest.jobs WHERE project_id = :p AND data_source_id = :s "
            "ORDER BY created_at DESC LIMIT :l OFFSET :o"
        ),
        {"p": project_id, "s": source_id, "l": page_size, "o": (page - 1) * page_size},
    ).fetchall()
    return [_row(r) for r in rows], int(total)


_JOB_UPDATABLE = {
    "status",
    "total_rows",
    "processed_rows",
    "accepted_rows",
    "rejected_rows",
    "error",
    "started_at",
    "finished_at",
}


def update_job(conn: Connection, job_id: uuid.UUID, **fields: Any) -> None:
    bad = set(fields) - _JOB_UPDATABLE
    if bad:
        raise ValueError(f"not updatable: {bad}")
    sets = ", ".join(f"{k} = :{k}" for k in fields)
    conn.execute(text(f"UPDATE ingest.jobs SET {sets} WHERE id = :id"), {**fields, "id": job_id})


def add_job_counters(
    conn: Connection, job_id: uuid.UUID, processed: int, accepted: int, rejected: int
) -> str:
    """Atomically bump counters; returns current status (used for cooperative cancellation)."""
    r = conn.execute(
        text(
            "UPDATE ingest.jobs SET processed_rows = processed_rows + :p, accepted_rows = accepted_rows + :a, "
            "rejected_rows = rejected_rows + :r WHERE id = :id RETURNING status"
        ),
        {"p": processed, "a": accepted, "r": rejected, "id": job_id},
    ).first()
    return str(r[0]) if r else "cancelled"


def list_pollable_sources(conn: Connection) -> list[dict[str, Any]]:
    """Cross-tenant listing of SQL sources with polling enabled.

    Needs the SECURITY DEFINER function core.list_pollable_sources() (RLS hides other tenants' rows from
    this role otherwise). Returns [] when the function is not installed.
    """
    rows = conn.execute(
        text("SELECT tenant_id, project_id, data_source_id FROM core.list_pollable_sources()")
    )
    return [_row(r) for r in rows]
