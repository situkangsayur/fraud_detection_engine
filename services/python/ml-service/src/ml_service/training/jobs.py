"""Bounded background executor for training jobs (TRAINING_WORKERS)."""

from __future__ import annotations

import threading
from collections.abc import Callable
from concurrent.futures import Future, ThreadPoolExecutor
from datetime import UTC, datetime
from typing import Any

from prometheus_client import Counter, Gauge

from ml_service.config import get_settings

PROCESS_STARTED_AT = datetime.now(UTC)
JOBS_TOTAL = Counter("ml_training_jobs_total", "Training jobs submitted", ["kind"])
JOBS_RUNNING = Gauge("ml_training_jobs_running", "Training jobs currently running")


class JobRunner:
    def __init__(self, workers: int) -> None:
        self._executor = ThreadPoolExecutor(max_workers=max(1, workers), thread_name_prefix="train")
        self._running: set[str] = set()
        self._lock = threading.Lock()

    def submit(self, model_id: str, kind: str, fn: Callable[..., Any], *args: Any) -> Future[Any]:
        JOBS_TOTAL.labels(kind=kind).inc()
        with self._lock:
            self._running.add(model_id)

        def wrapped() -> Any:
            JOBS_RUNNING.inc()
            try:
                return fn(*args)
            finally:
                JOBS_RUNNING.dec()
                with self._lock:
                    self._running.discard(model_id)

        return self._executor.submit(wrapped)

    def running(self) -> set[str]:
        with self._lock:
            return set(self._running)

    def shutdown(self) -> None:
        self._executor.shutdown(wait=False, cancel_futures=True)


_runner: JobRunner | None = None


def get_runner() -> JobRunner:
    global _runner
    if _runner is None:
        _runner = JobRunner(get_settings().training_workers)
    return _runner
