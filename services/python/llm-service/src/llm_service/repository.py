"""Data access for the ``llm`` schema. Every function takes an open tenant-scoped session (RLS applies).

Plain SQL (SQLAlchemy Core ``text``) keeps queries explicit and reviewable; the schema is owned by
``db/migrations`` (this service never runs DDL). f-strings here only interpolate module constants
(column lists) and whitelisted identifiers — every value is a bound parameter (ruff S608 is ignored for this file).
"""

from __future__ import annotations

import json
import uuid
from datetime import date
from typing import Any

from sqlalchemy import text
from sqlalchemy.orm import Session

Row = dict[str, Any]


def _rows(result: Any) -> list[Row]:
    return [dict(r._mapping) for r in result]


def _one(result: Any) -> Row | None:
    r = result.first()
    return dict(r._mapping) if r is not None else None


# ---------------------------------------------------------------------------- regulations
_REG_COLS = (
    "id, tenant_id, code, title, doc_type, issuer, version, effective_date, supersedes_id, file_name, "
    "file_sha256, file_path, status, chunk_count, summary, error, uploaded_by, created_at"
)


def find_regulation_by_sha(s: Session, tenant_id: uuid.UUID, sha: str) -> Row | None:
    return _one(
        s.execute(
            text(f"SELECT {_REG_COLS} FROM llm.regulations WHERE tenant_id=:t AND file_sha256=:h"),
            {"t": tenant_id, "h": sha},
        )
    )


def next_regulation_version(s: Session, tenant_id: uuid.UUID, code: str) -> int:
    v = s.execute(
        text("SELECT COALESCE(MAX(version), 0) + 1 FROM llm.regulations WHERE tenant_id=:t AND code=:c"),
        {"t": tenant_id, "c": code},
    ).scalar_one()
    return int(v)


def insert_regulation(
    s: Session,
    *,
    reg_id: uuid.UUID,
    tenant_id: uuid.UUID,
    code: str,
    title: str,
    doc_type: str,
    issuer: str,
    version: int,
    effective_date: date | None,
    supersedes_id: uuid.UUID | None,
    file_name: str,
    sha: str,
    file_path: str,
    uploaded_by: uuid.UUID | None,
) -> Row:
    row = _one(
        s.execute(
            text(f"""
        INSERT INTO llm.regulations (id, tenant_id, code, title, doc_type, issuer, version, effective_date,
            supersedes_id, file_name, file_sha256, file_path, status, uploaded_by)
        VALUES (:id, :t, :code, :title, :doc_type, :issuer, :version, :eff, :sup, :fn, :sha, :fp, 'processing', :by)
        RETURNING {_REG_COLS}"""),
            {
                "id": reg_id,
                "t": tenant_id,
                "code": code,
                "title": title,
                "doc_type": doc_type,
                "issuer": issuer,
                "version": version,
                "eff": effective_date,
                "sup": supersedes_id,
                "fn": file_name,
                "sha": sha,
                "fp": file_path,
                "by": uploaded_by,
            },
        )
    )
    assert row is not None
    return row


def get_regulation(s: Session, reg_id: uuid.UUID) -> Row | None:
    return _one(s.execute(text(f"SELECT {_REG_COLS} FROM llm.regulations WHERE id=:id"), {"id": reg_id}))


def list_regulations(
    s: Session, tenant_id: uuid.UUID, *, status: str | None, q: str | None, limit: int, offset: int
) -> tuple[list[Row], int]:
    where = ["tenant_id = :t"]
    params: dict[str, Any] = {"t": tenant_id, "limit": limit, "offset": offset}
    if status:
        where.append("status = :status")
        params["status"] = status
    if q:
        where.append("(code ILIKE :q OR title ILIKE :q)")
        params["q"] = f"%{q}%"
    w = " AND ".join(where)
    total = s.execute(text(f"SELECT count(*) FROM llm.regulations WHERE {w}"), params).scalar_one()
    rows = _rows(
        s.execute(
            text(
                f"SELECT {_REG_COLS} FROM llm.regulations WHERE {w} "
                "ORDER BY created_at DESC LIMIT :limit OFFSET :offset"
            ),
            params,
        )
    )
    return rows, int(total)


def update_regulation(s: Session, reg_id: uuid.UUID, **fields: Any) -> None:
    allowed = {"status", "chunk_count", "summary", "error"}
    sets = {k: v for k, v in fields.items() if k in allowed}
    if not sets:
        return
    assignments = ", ".join(f"{k} = :{k}" for k in sets)
    s.execute(text(f"UPDATE llm.regulations SET {assignments} WHERE id = :id"), {**sets, "id": reg_id})


def delete_regulation(s: Session, reg_id: uuid.UUID) -> None:
    s.execute(text("UPDATE llm.regulations SET supersedes_id = NULL WHERE supersedes_id = :id"), {"id": reg_id})
    s.execute(text("DELETE FROM llm.regulations WHERE id = :id"), {"id": reg_id})


def insert_change(
    s: Session,
    *,
    tenant_id: uuid.UUID,
    regulation_id: uuid.UUID,
    previous_id: uuid.UUID,
    changed_sections: list[dict[str, Any]],
    diff_summary: str | None,
) -> Row:
    row = _one(
        s.execute(
            text("""
        INSERT INTO llm.regulation_changes
            (tenant_id, regulation_id, previous_regulation_id, changed_sections, diff_summary)
        VALUES (:t, :r, :p, CAST(:cs AS jsonb), :ds)
        RETURNING id, regulation_id, previous_regulation_id, changed_sections, diff_summary, created_at"""),
            {
                "t": tenant_id,
                "r": regulation_id,
                "p": previous_id,
                "cs": json.dumps(changed_sections),
                "ds": diff_summary,
            },
        )
    )
    assert row is not None
    return row


def list_changes(s: Session, regulation_id: uuid.UUID) -> list[Row]:
    return _rows(
        s.execute(
            text("""
        SELECT id, regulation_id, previous_regulation_id, changed_sections, diff_summary, created_at
        FROM llm.regulation_changes WHERE regulation_id = :r ORDER BY created_at DESC"""),
            {"r": regulation_id},
        )
    )


# ---------------------------------------------------------------------------- project attachment
def attached_regulation_ids(s: Session, project_id: uuid.UUID, *, indexed_only: bool = True) -> list[uuid.UUID]:
    sql = """SELECT pr.regulation_id FROM llm.project_regulations pr
             JOIN llm.regulations r ON r.id = pr.regulation_id
             WHERE pr.project_id = :p"""
    if indexed_only:
        sql += " AND r.status = 'indexed'"
    return [r[0] for r in s.execute(text(sql), {"p": project_id})]


def list_attached(s: Session, project_id: uuid.UUID) -> list[Row]:
    return _rows(
        s.execute(
            text(f"""
        SELECT {", ".join("r." + c.strip() for c in _REG_COLS.split(","))}, pr.attached_at, pr.attached_by
        FROM llm.project_regulations pr JOIN llm.regulations r ON r.id = pr.regulation_id
        WHERE pr.project_id = :p ORDER BY r.code, r.version"""),
            {"p": project_id},
        )
    )


def set_attached(
    s: Session, *, tenant_id: uuid.UUID, project_id: uuid.UUID, regulation_ids: list[uuid.UUID], actor: uuid.UUID | None
) -> None:
    s.execute(
        text("DELETE FROM llm.project_regulations WHERE project_id = :p AND NOT (regulation_id = ANY(:ids))"),
        {"p": project_id, "ids": regulation_ids},
    )
    for rid in regulation_ids:
        s.execute(
            text("""INSERT INTO llm.project_regulations (tenant_id, project_id, regulation_id, attached_by)
                          VALUES (:t, :p, :r, :a) ON CONFLICT (project_id, regulation_id) DO NOTHING"""),
            {"t": tenant_id, "p": project_id, "r": rid, "a": actor},
        )


def move_attachments(s: Session, old_id: uuid.UUID, new_id: uuid.UUID) -> None:
    """When a regulation is superseded, projects that followed the old version follow the new one."""
    s.execute(
        text("""INSERT INTO llm.project_regulations (tenant_id, project_id, regulation_id, attached_by)
                      SELECT tenant_id, project_id, :new, attached_by
                      FROM llm.project_regulations WHERE regulation_id = :old
                      ON CONFLICT (project_id, regulation_id) DO NOTHING"""),
        {"old": old_id, "new": new_id},
    )
    s.execute(text("DELETE FROM llm.project_regulations WHERE regulation_id = :old"), {"old": old_id})


def regulations_by_ids(s: Session, ids: list[uuid.UUID]) -> list[Row]:
    if not ids:
        return []
    return _rows(s.execute(text(f"SELECT {_REG_COLS} FROM llm.regulations WHERE id = ANY(:ids)"), {"ids": ids}))


# ---------------------------------------------------------------------------- reports
_REPORT_COLS = (
    "id, tenant_id, project_id, report_type, title, status, params, content_md, structured, model, "
    "error, created_by, created_at, finished_at"
)


def create_report(
    s: Session,
    *,
    tenant_id: uuid.UUID,
    project_id: uuid.UUID,
    report_type: str,
    title: str,
    params: dict[str, Any],
    model: str,
    created_by: uuid.UUID | None,
) -> Row:
    row = _one(
        s.execute(
            text(f"""
        INSERT INTO llm.reports (tenant_id, project_id, report_type, title, status, params, model, created_by)
        VALUES (:t, :p, :rt, :title, 'running', CAST(:params AS jsonb), :model, :by) RETURNING {_REPORT_COLS}"""),
            {
                "t": tenant_id,
                "p": project_id,
                "rt": report_type,
                "title": title,
                "params": json.dumps(params),
                "model": model,
                "by": created_by,
            },
        )
    )
    assert row is not None
    return row


def finish_report(s: Session, report_id: uuid.UUID, *, content_md: str, structured: dict[str, Any]) -> None:
    s.execute(
        text("""UPDATE llm.reports SET status='done', content_md=:md, structured=CAST(:st AS jsonb),
                      finished_at=now() WHERE id=:id"""),
        {"md": content_md, "st": json.dumps(structured, default=str), "id": report_id},
    )


def fail_report(s: Session, report_id: uuid.UUID, error: str) -> None:
    s.execute(
        text("UPDATE llm.reports SET status='failed', error=:e, finished_at=now() WHERE id=:id"),
        {"e": error[:4000], "id": report_id},
    )


def get_report(s: Session, project_id: uuid.UUID, report_id: uuid.UUID) -> Row | None:
    return _one(
        s.execute(
            text(f"SELECT {_REPORT_COLS} FROM llm.reports WHERE id=:id AND project_id=:p"),
            {"id": report_id, "p": project_id},
        )
    )


def list_reports(
    s: Session, project_id: uuid.UUID, *, report_type: str | None, limit: int, offset: int
) -> tuple[list[Row], int]:
    params: dict[str, Any] = {"p": project_id, "limit": limit, "offset": offset}
    where = "project_id = :p"
    if report_type:
        where += " AND report_type = :rt"
        params["rt"] = report_type
    total = s.execute(text(f"SELECT count(*) FROM llm.reports WHERE {where}"), params).scalar_one()
    rows = _rows(
        s.execute(
            text(
                f"SELECT id, tenant_id, project_id, report_type, title, status, params, model, error, created_by, "
                f"created_at, finished_at FROM llm.reports WHERE {where} ORDER BY created_at DESC "
                f"LIMIT :limit OFFSET :offset"
            ),
            params,
        )
    )
    return rows, int(total)


# ---------------------------------------------------------------------------- conversations
def create_conversation(
    s: Session, *, tenant_id: uuid.UUID, project_id: uuid.UUID, user_id: uuid.UUID | None, title: str
) -> Row:
    row = _one(
        s.execute(
            text("""
        INSERT INTO llm.conversations (tenant_id, project_id, user_id, title) VALUES (:t, :p, :u, :title)
        RETURNING id, tenant_id, project_id, user_id, title, created_at"""),
            {"t": tenant_id, "p": project_id, "u": user_id, "title": title[:200]},
        )
    )
    assert row is not None
    return row


def get_conversation(s: Session, project_id: uuid.UUID, conversation_id: uuid.UUID) -> Row | None:
    return _one(
        s.execute(
            text("""SELECT id, tenant_id, project_id, user_id, title, created_at FROM llm.conversations
                                  WHERE id=:id AND project_id=:p"""),
            {"id": conversation_id, "p": project_id},
        )
    )


def list_conversations(
    s: Session, project_id: uuid.UUID, user_id: uuid.UUID | None, *, limit: int, offset: int
) -> tuple[list[Row], int]:
    params: dict[str, Any] = {"p": project_id, "u": user_id, "limit": limit, "offset": offset}
    where = "project_id = :p AND user_id IS NOT DISTINCT FROM :u"
    total = s.execute(text(f"SELECT count(*) FROM llm.conversations WHERE {where}"), params).scalar_one()
    rows = _rows(
        s.execute(
            text(f"""SELECT id, tenant_id, project_id, user_id, title, created_at FROM llm.conversations
        WHERE {where} ORDER BY created_at DESC LIMIT :limit OFFSET :offset"""),
            params,
        )
    )
    return rows, int(total)


def add_message(
    s: Session,
    *,
    tenant_id: uuid.UUID,
    conversation_id: uuid.UUID,
    role: str,
    content: str,
    tool_calls: Any = None,
    citations: Any = None,
) -> None:
    s.execute(
        text("""INSERT INTO llm.messages (tenant_id, conversation_id, role, content, tool_calls, citations)
                      VALUES (:t, :c, :r, :content, CAST(:tc AS jsonb), CAST(:ci AS jsonb))"""),
        {
            "t": tenant_id,
            "c": conversation_id,
            "r": role,
            "content": content,
            "tc": json.dumps(tool_calls, default=str) if tool_calls is not None else None,
            "ci": json.dumps(citations, default=str) if citations is not None else None,
        },
    )


def list_messages(s: Session, conversation_id: uuid.UUID, limit: int = 200) -> list[Row]:
    rows = _rows(
        s.execute(
            text("""SELECT id, role, content, tool_calls, citations, created_at FROM llm.messages
                                   WHERE conversation_id = :c ORDER BY id DESC LIMIT :l"""),
            {"c": conversation_id, "l": limit},
        )
    )
    rows.reverse()
    return rows
