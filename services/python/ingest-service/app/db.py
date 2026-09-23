"""Database access: SQLAlchemy engine + tenant-scoped sessions (Postgres RLS).

Every unit of work runs in a transaction that starts with
`set_config('app.tenant_id', <tenant>, true)` (transaction-local → safe with connection pools).
Queries still filter by project_id explicitly; RLS is the safety net.
"""

from __future__ import annotations

import uuid
from collections.abc import Iterator
from contextlib import contextmanager
from functools import lru_cache

from sqlalchemy import Engine, create_engine, text
from sqlalchemy.engine import Connection

from app.config import get_settings


@lru_cache
def get_engine() -> Engine:
    s = get_settings()
    return create_engine(s.database_url, pool_pre_ping=True, pool_size=5, max_overflow=10, future=True)


@contextmanager
def tenant_session(tenant_id: uuid.UUID, engine: Engine | None = None) -> Iterator[Connection]:
    eng = engine or get_engine()
    with eng.begin() as conn:
        conn.execute(text("SELECT set_config('app.tenant_id', :t, true)"), {"t": str(tenant_id)})
        yield conn


def ping(engine: Engine | None = None) -> bool:
    try:
        with (engine or get_engine()).connect() as conn:
            conn.execute(text("SELECT 1"))
        return True
    except Exception:
        return False
