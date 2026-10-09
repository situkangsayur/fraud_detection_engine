"""Database access: SQLAlchemy engine + tenant-scoped transactions (RLS)."""

from __future__ import annotations

from collections.abc import Iterator
from contextlib import contextmanager
from functools import lru_cache
from uuid import UUID

from sqlalchemy import Engine, create_engine, text
from sqlalchemy.engine import Connection

from ml_service.config import get_settings


@lru_cache(maxsize=1)
def get_engine() -> Engine:
    settings = get_settings()
    return create_engine(
        settings.database_url,
        pool_size=settings.database_pool_size,
        max_overflow=settings.database_pool_size,
        pool_pre_ping=True,
        pool_recycle=1800,
    )


@contextmanager
def tenant_session(tenant_id: UUID | str) -> Iterator[Connection]:
    """Transaction with `app.tenant_id` set (transaction-local → pool safe). RLS fails closed without it.

    Queries must still filter by `project_id` explicitly; RLS is the safety net, not the primary filter.
    """
    with get_engine().begin() as conn:
        conn.execute(text("SELECT set_config('app.tenant_id', :t, true)"), {"t": str(tenant_id)})
        yield conn


@contextmanager
def platform_session() -> Iterator[Connection]:
    """Transaction without tenant context — only for platform tables (ml.algorithms)."""
    with get_engine().begin() as conn:
        yield conn


def ping() -> bool:
    try:
        with get_engine().connect() as conn:
            conn.execute(text("SELECT 1"))
        return True
    except Exception:
        return False
