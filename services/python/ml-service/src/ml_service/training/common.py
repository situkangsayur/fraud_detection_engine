"""Shared training helpers: artifact layout, progress reporting, failure handling."""

from __future__ import annotations

import json
import shutil
import time
from collections.abc import Callable
from pathlib import Path
from typing import Any
from uuid import UUID

from ml_service.config import get_settings
from ml_service.db import tenant_session
from ml_service.logging import get_logger
from ml_service.repository import update_model

log = get_logger(__name__)


class TrainingError(Exception):
    """Expected, user-facing training failure (e.g. not enough labelled data)."""


def artifact_dir(tenant_id: UUID, project_id: UUID, model_id: UUID) -> Path:
    return get_settings().model_dir / str(tenant_id) / str(project_id) / str(model_id)


def write_meta(directory: Path, meta: dict[str, Any]) -> None:
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "meta.json").write_text(json.dumps(meta, default=str))


def read_meta(directory: Path) -> dict[str, Any]:
    return json.loads((directory / "meta.json").read_text())


def progress_reporter(tenant_id: UUID, model_id: UUID, lo: float, hi: float) -> Callable[[float], None]:
    """Maps plugin progress [0,1] onto [lo,hi] of the overall job; throttled DB writes (≤ 1/2s)."""
    last = [0.0]

    def report(fraction: float) -> None:
        now = time.monotonic()
        if now - last[0] < 2.0 and fraction < 1.0:
            return
        last[0] = now
        value = round(lo + (hi - lo) * max(0.0, min(1.0, fraction)), 4)
        try:
            with tenant_session(tenant_id) as conn:
                update_model(conn, model_id, progress=value)
        except Exception as exc:  # progress is best-effort
            log.warning("progress_update_failed", model_id=str(model_id), error=str(exc))

    return report


def fail_model(tenant_id: UUID, model_id: UUID, error: str, directory: Path | None = None) -> None:
    if directory is not None and directory.exists():
        shutil.rmtree(directory, ignore_errors=True)
    with tenant_session(tenant_id) as conn:
        update_model(conn, model_id, status="failed", error=error[:2000], training_finished_at="now")
