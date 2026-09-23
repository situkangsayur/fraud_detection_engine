"""Ingest job execution.

A job reads a stored upload (or a full SQL source), optionally sorts records by the mapped `occurred_at`
(velocity features and graph links assume chronological order), and streams batches of raw records to
core-api. Mapping itself is applied only by core-api (single implementation for webhook/batch/preview).

Memory: files up to `sort_max_rows` rows are loaded fully to sort; larger files are streamed in file order
(pre-sort such files, or load them with mode=load_only first). Cancellation is cooperative: the job
checks its DB status after every batch.
"""

from __future__ import annotations

import threading
import uuid
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from app import repo
from app.config import Settings
from app.connectors import sql as sql_conn
from app.core_client import CoreApiError, CoreClient
from app.db import tenant_session
from app.inference.flatten import get_path
from app.inference.types import to_utc_iso
from app.logging import log
from app.metrics import JOBS, ROWS
from app.readers import iter_records


def occurred_at_key(mapping: dict[str, Any] | None) -> tuple[str | None, str | None]:
    """(source path, datetime format) of the mapped occurred_at, if it is a simple `from` mapping."""
    if not mapping:
        return None, None
    spec = (mapping.get("event") or {}).get("occurred_at") or {}
    src = spec.get("from")
    if not isinstance(src, str):
        return None, None
    fmt = None
    for t in spec.get("transform") or []:
        if t.get("fn") == "parse_datetime":
            fmt = t.get("format")
    return src, fmt


def sort_records(records: list[dict[str, Any]], mapping: dict[str, Any] | None) -> bool:
    path, fmt = occurred_at_key(mapping)
    if not path:
        return False
    records.sort(key=lambda r: to_utc_iso(get_path(r, path), fmt) or "9999")
    return True


class JobRunner:
    def __init__(self, settings: Settings, workers: int = 2, client: CoreClient | None = None) -> None:
        self.s = settings
        self.pool = ThreadPoolExecutor(max_workers=workers, thread_name_prefix="ingest-job")
        self.client = client or CoreClient(settings)
        self._cancel: set[uuid.UUID] = set()
        self._lock = threading.Lock()

    def submit(
        self,
        job: dict[str, Any],
        source: dict[str, Any],
        mapping: dict[str, Any] | None,
        upload: dict[str, Any] | None,
        actor: str | None,
    ) -> None:
        self.pool.submit(self.run, job, source, mapping, upload, actor)

    def cancel(self, job_id: uuid.UUID) -> None:
        with self._lock:
            self._cancel.add(job_id)

    def _cancelled(self, job_id: uuid.UUID) -> bool:
        with self._lock:
            return job_id in self._cancel

    def _chunks(
        self,
        source: dict[str, Any],
        mapping: dict[str, Any] | None,
        upload: dict[str, Any] | None,
        job_id: uuid.UUID,
    ) -> Any:
        if upload:
            path, fmt = Path(upload["file_path"]), upload["file_format"]
            est = upload.get("row_estimate")
            if est is not None and est <= self.s.sort_max_rows and occurred_at_key(mapping)[0]:
                everything: list[dict[str, Any]] = []
                for chunk in iter_records(path, fmt):
                    everything.extend(chunk)
                sort_records(everything, mapping)
                for i in range(0, len(everything), self.s.batch_size):
                    yield everything[i : i + self.s.batch_size]
                return
            log.info("job_streaming_unsorted", job_id=str(job_id), row_estimate=est)
            for chunk in iter_records(path, fmt, chunk_size=self.s.batch_size):
                yield chunk
        else:
            yield from sql_conn.iter_full(source["connection"] or {}, self.s.batch_size)

    def run(
        self,
        job: dict[str, Any],
        source: dict[str, Any],
        mapping: dict[str, Any] | None,
        upload: dict[str, Any] | None,
        actor: str | None,
    ) -> str:
        job_id: uuid.UUID = job["id"]
        tenant_id: uuid.UUID = job["tenant_id"]
        project_id: uuid.UUID = job["project_id"]
        status = "done"
        error: str | None = None
        with tenant_session(tenant_id) as c:
            repo.update_job(c, job_id, status="running", started_at=datetime.now(UTC))
        try:
            for batch in self._chunks(source, mapping, upload, job_id):
                if not batch:
                    continue
                res = self.client.send_batch(
                    tenant_id=tenant_id,
                    project_id=project_id,
                    source_id=source["id"],
                    records=batch,
                    mode=job["mode"],
                    job_id=job_id,
                    actor=actor,
                )
                ROWS.labels("accepted").inc(res.accepted)
                ROWS.labels("rejected").inc(res.rejected)
                with tenant_session(tenant_id) as c:
                    current = repo.add_job_counters(c, job_id, len(batch), res.accepted, res.rejected)
                if current == "cancelled" or self._cancelled(job_id):
                    status = "cancelled"
                    break
        except CoreApiError as e:
            status, error = "failed", str(e)
        except Exception as e:  # file/DB errors → job failure, never crash the worker
            status, error = "failed", f"{type(e).__name__}: {e}"
            log.exception("job_failed", job_id=str(job_id))
        with tenant_session(tenant_id) as c:
            fields: dict[str, Any] = {"status": status, "finished_at": datetime.now(UTC)}
            if error:
                fields["error"] = error[:2000]
            repo.update_job(c, job_id, **fields)
        with self._lock:
            self._cancel.discard(job_id)
        JOBS.labels(status).inc()
        log.info("job_finished", job_id=str(job_id), status=status)
        return status

    def shutdown(self) -> None:
        self.pool.shutdown(wait=False, cancel_futures=True)
        self.client.close()
