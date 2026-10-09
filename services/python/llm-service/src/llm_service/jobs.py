"""In-process background job runner with bounded concurrency and graceful shutdown.

Analyses and document indexing are long-running (minutes on CPU inference); the API answers immediately with an
id that the client polls. A durable queue is on the backlog; on restart, jobs that were ``running`` stay visible as
such until re-triggered (documented limitation).
"""

from __future__ import annotations

import asyncio
from collections.abc import Awaitable, Callable

from llm_service.logging import get_logger

log = get_logger(__name__)


class JobRunner:
    def __init__(self, max_concurrency: int = 2) -> None:
        self._sem = asyncio.Semaphore(max_concurrency)
        self._tasks: set[asyncio.Task[None]] = set()

    def submit(self, name: str, factory: Callable[[], Awaitable[None]]) -> None:
        async def _run() -> None:
            async with self._sem:
                try:
                    await factory()
                except asyncio.CancelledError:
                    raise
                except Exception:
                    log.exception("job_failed", job=name)

        task = asyncio.create_task(_run(), name=name)
        self._tasks.add(task)
        task.add_done_callback(self._tasks.discard)

    @property
    def running(self) -> int:
        return len(self._tasks)

    async def shutdown(self, grace_s: float = 10.0) -> None:
        if not self._tasks:
            return
        _, pending = await asyncio.wait(self._tasks, timeout=grace_s)
        for t in pending:
            t.cancel()
