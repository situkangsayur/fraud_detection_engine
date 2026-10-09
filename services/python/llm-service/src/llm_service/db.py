"""Database access: SQLAlchemy engine + tenant-scoped sessions (RLS via app.tenant_id)."""

from __future__ import annotations

import uuid
from collections.abc import Iterator
from contextlib import contextmanager
from functools import lru_cache

from sqlalchemy import Engine, create_engine, text
from sqlalchemy.orm import Session, sessionmaker

from llm_service.config import get_settings


@lru_cache
def get_engine() -> Engine:
    return create_engine(get_settings().database_url, pool_pre_ping=True, pool_size=10, max_overflow=5)


@lru_cache
def _session_factory() -> sessionmaker[Session]:
    return sessionmaker(bind=get_engine(), expire_on_commit=False)


@contextmanager
def tenant_session(tenant_id: uuid.UUID) -> Iterator[Session]:
    """Open a transaction bound to ``tenant_id``.

    ``set_config(..., true)`` is transaction-local, so pooled connections never leak tenant context.
    Every statement inside is subject to Row-Level Security; queries still filter by project explicitly.
    """
    session = _session_factory()()
    try:
        session.execute(text("SELECT set_config('app.tenant_id', :tid, true)"), {"tid": str(tenant_id)})
        yield session
        session.commit()
    except Exception:
        session.rollback()
        raise
    finally:
        session.close()


def ping() -> bool:
    try:
        with get_engine().connect() as conn:
            conn.execute(text("SELECT 1"))
        return True
    except Exception:
        return False
