"""External SQL sources (Postgres/MySQL) — read-only sampling, full loads and incremental polling.

Connection config (core.data_sources.connection), never containing the password itself:
    {"driver": "postgres"|"mysql", "host": "...", "port": 5432, "database": "...", "user": "...",
     "password_env": "SRC_ERP_DB_PASSWORD", "table": "schema.table" | null, "query": "SELECT ..." | null,
     "cursor_field": "updated_at", "poll": {"enabled": true, "interval_seconds": 60, "batch_size": 500}}

Use a database user with SELECT-only grants. Sessions are additionally forced read-only.
"""

from __future__ import annotations

import os
import re
from collections.abc import Iterator
from typing import Any

from sqlalchemy import create_engine, event, text
from sqlalchemy.engine import URL, Engine

from app.errors import ProblemError
from app.readers import jsonable, unflatten

IDENT_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_$]*(\.[A-Za-z_][A-Za-z0-9_$]*)?$")
FORBIDDEN_SQL = re.compile(
    r";|\b(insert|update|delete|drop|alter|create|grant|truncate|copy|call|do)\b", re.I
)


def build_url(conn_cfg: dict[str, Any]) -> URL:
    driver = conn_cfg.get("driver") or conn_cfg.get("kind")
    pw_env = conn_cfg.get("password_env")
    password = os.environ.get(pw_env, "") if pw_env else None
    if pw_env and password == "":
        raise ProblemError(422, "Connection misconfigured", f"environment variable {pw_env} is not set")
    if driver in ("postgres", "postgresql"):
        return URL.create(
            "postgresql+psycopg",
            username=conn_cfg.get("user"),
            password=password,
            host=conn_cfg.get("host"),
            port=int(conn_cfg.get("port") or 5432),
            database=conn_cfg.get("database"),
        )
    if driver == "mysql":
        return URL.create(
            "mysql+pymysql",
            username=conn_cfg.get("user"),
            password=password,
            host=conn_cfg.get("host"),
            port=int(conn_cfg.get("port") or 3306),
            database=conn_cfg.get("database"),
        )
    raise ProblemError(422, "Connection misconfigured", f"unsupported driver '{driver}'")


def make_engine(conn_cfg: dict[str, Any]) -> Engine:
    url = build_url(conn_cfg)
    if url.drivername.startswith("postgresql"):
        eng = create_engine(
            url,
            pool_pre_ping=True,
            pool_size=2,
            connect_args={"options": "-c default_transaction_read_only=on", "connect_timeout": 10},
        )
    else:
        eng = create_engine(url, pool_pre_ping=True, pool_size=2, connect_args={"connect_timeout": 10})

        @event.listens_for(eng, "connect")
        def _ro(dbapi_conn: Any, _: Any) -> None:  # pragma: no cover - needs mysql
            with dbapi_conn.cursor() as cur:
                cur.execute("SET SESSION TRANSACTION READ ONLY")

    return eng


def source_relation(conn_cfg: dict[str, Any], table: str | None = None, query: str | None = None) -> str:
    table = table or conn_cfg.get("table")
    query = query or conn_cfg.get("query")
    if table:
        if not IDENT_RE.match(table):
            raise ProblemError(422, "Invalid table", "table must be an identifier like schema.table")
        return table
    if query:
        q = query.strip().rstrip(";")
        if not re.match(r"^\s*(select|with)\b", q, re.I) or FORBIDDEN_SQL.search(q):
            raise ProblemError(422, "Invalid query", "only a single read-only SELECT/WITH query is allowed")
        return f"({q}) AS src"
    raise ProblemError(422, "Invalid source", "either table or query is required")


def _check_ident(name: str) -> str:
    if not IDENT_RE.match(name):
        raise ProblemError(422, "Invalid cursor field", f"'{name}' is not a valid column identifier")
    return name


def _rows(result: Any) -> list[dict[str, Any]]:
    return [unflatten({str(k): jsonable(v) for k, v in r._mapping.items()}) for r in result]


def sample(
    conn_cfg: dict[str, Any], table: str | None, query: str | None, limit: int
) -> list[dict[str, Any]]:
    rel = source_relation(conn_cfg, table, query)
    eng = make_engine(conn_cfg)
    try:
        with eng.connect() as c:
            return _rows(c.execute(text(f"SELECT * FROM {rel} LIMIT :n"), {"n": int(limit)}))
    except ProblemError:
        raise
    except Exception as e:
        raise ProblemError(502, "Source database error", str(e).splitlines()[0][:300]) from e
    finally:
        eng.dispose()


def iter_full(conn_cfg: dict[str, Any], chunk: int) -> Iterator[list[dict[str, Any]]]:
    rel = source_relation(conn_cfg)
    cursor = conn_cfg.get("cursor_field")
    order = f" ORDER BY {_check_ident(cursor)}" if cursor else ""
    eng = make_engine(conn_cfg)
    try:
        with eng.connect() as c:
            res = c.execution_options(stream_results=True, yield_per=chunk).execute(
                text(f"SELECT * FROM {rel}{order}")
            )
            while True:
                part = res.fetchmany(chunk)
                if not part:
                    break
                yield _rows(part)
    finally:
        eng.dispose()


def fetch_increment(conn_cfg: dict[str, Any], last_cursor: Any, batch: int) -> list[dict[str, Any]]:
    """Rows with cursor_field > last_cursor, ascending. The cursor must be monotonic and unique enough
    that a batch boundary never splits rows sharing one cursor value (e.g. an id or (updated_at, id))."""
    rel = source_relation(conn_cfg)
    cursor = _check_ident(conn_cfg.get("cursor_field") or "")
    where = f" WHERE {cursor} > :c" if last_cursor is not None else ""
    eng = make_engine(conn_cfg)
    try:
        with eng.connect() as c:
            params: dict[str, Any] = {"n": int(batch)}
            if last_cursor is not None:
                params["c"] = last_cursor
            return _rows(c.execute(text(f"SELECT * FROM {rel}{where} ORDER BY {cursor} LIMIT :n"), params))
    finally:
        eng.dispose()
