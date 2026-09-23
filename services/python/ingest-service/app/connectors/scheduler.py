"""Background loops: pull-connector polling and upload expiry cleanup."""

from __future__ import annotations

import asyncio
import contextlib
import shutil
import time
import uuid
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from app import repo
from app.config import Settings
from app.connectors import sql as sql_conn
from app.core_client import CoreClient
from app.db import get_engine, tenant_session
from app.logging import log
from app.metrics import POLLS


def poll_source_once(
    settings: Settings,
    client: CoreClient,
    tenant_id: uuid.UUID,
    project_id: uuid.UUID,
    source_id: uuid.UUID,
    now: float | None = None,
) -> int:
    """Poll one SQL source if its interval elapsed. Returns rows sent. At-least-once: the cursor advances
    only after core-api accepted the batch (core-api dedups on (source, external_id))."""
    now = now or time.time()
    with tenant_session(tenant_id) as c:
        src = repo.get_data_source(c, project_id, source_id)
    if not src or not src["is_active"]:
        return 0
    cfg: dict[str, Any] = src["connection"] or {}
    poll = cfg.get("poll") or {}
    state: dict[str, Any] = dict(src["cursor_state"] or {})
    if now - float(state.get("last_polled_epoch", 0)) < float(poll.get("interval_seconds", 60)):
        return 0
    cursor_field = cfg.get("cursor_field")
    if not cursor_field:
        state.update(last_error="cursor_field is required for polling", last_polled_epoch=now)
        with tenant_session(tenant_id) as c:
            repo.update_cursor_state(c, source_id, state)
        return 0
    sent = 0
    try:
        rows = sql_conn.fetch_increment(cfg, state.get("last_cursor"), int(poll.get("batch_size", 500)))
        if rows:
            client.send_batch(
                tenant_id=tenant_id,
                project_id=project_id,
                source_id=source_id,
                records=rows,
                mode=src["mode"],
                job_id=None,
                actor="ingest-service:poller",
            )
            sent = len(rows)
            state["last_cursor"] = rows[-1].get(cursor_field)
        state.pop("last_error", None)
        POLLS.labels("ok").inc()
    except Exception as e:
        state["last_error"] = f"{type(e).__name__}: {e}"[:500]
        POLLS.labels("error").inc()
        log.warning("poll_failed", source_id=str(source_id), error=state["last_error"])
    state["last_polled_epoch"] = now
    state["last_polled_at"] = datetime.fromtimestamp(now, UTC).isoformat()
    with tenant_session(tenant_id) as c:
        repo.update_cursor_state(c, source_id, state)
    return sent


async def connector_loop(settings: Settings, client: CoreClient, stop: asyncio.Event) -> None:
    warned = False
    while not stop.is_set():
        try:

            def _list() -> list[dict[str, Any]]:
                with get_engine().begin() as c:
                    return repo.list_pollable_sources(c)

            sources = await asyncio.to_thread(_list)
            for s in sources:
                await asyncio.to_thread(
                    poll_source_once, settings, client, s["tenant_id"], s["project_id"], s["data_source_id"]
                )
        except Exception as e:
            if not warned:
                log.warning("connector_loop_unavailable", error=str(e).splitlines()[0][:200])
                warned = True
        with contextlib.suppress(TimeoutError):
            await asyncio.wait_for(stop.wait(), timeout=settings.connector_tick_s)


def cleanup_uploads_once(settings: Settings) -> int:
    """Delete expired upload files. The upload dir layout is <UPLOAD_DIR>/<tenant_id>/<upload_id>/file,
    so each tenant's rows are removed under that tenant's RLS context."""
    removed = 0
    root = settings.upload_dir
    if not root.exists():
        return 0
    now = datetime.now(UTC)
    for tenant_dir in root.iterdir():
        try:
            tenant_id = uuid.UUID(tenant_dir.name)
        except ValueError:
            continue
        try:
            with tenant_session(tenant_id) as c:
                paths = repo.delete_expired_uploads(c, now)
        except Exception:
            paths = []
        for p in paths:
            shutil.rmtree(Path(p).parent, ignore_errors=True)
            removed += 1
        # orphans (no DB row) older than TTL
        cutoff = time.time() - settings.upload_ttl_hours * 3600
        for up in tenant_dir.iterdir():
            if up.is_dir() and up.stat().st_mtime < cutoff:
                shutil.rmtree(up, ignore_errors=True)
                removed += 1
    return removed


async def cleanup_loop(settings: Settings, stop: asyncio.Event) -> None:
    while not stop.is_set():
        try:
            n = await asyncio.to_thread(cleanup_uploads_once, settings)
            if n:
                log.info("uploads_cleaned", removed=n)
        except Exception as e:
            log.warning("cleanup_failed", error=str(e))
        with contextlib.suppress(TimeoutError):
            await asyncio.wait_for(stop.wait(), timeout=3600)
