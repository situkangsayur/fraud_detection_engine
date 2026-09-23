"""LRU cache of active models per (project, kind) with hot swap and a short negative cache.

The hot path touches the database only when a model is not cached, when a cached entry is older than
`model_refresh_seconds` (to pick up activations done by another replica), or when the negative-cache entry
("no active model") expired.
"""

from __future__ import annotations

import threading
import time
from collections import OrderedDict
from pathlib import Path
from typing import Any
from uuid import UUID

from ml_service.config import get_settings
from ml_service.db import tenant_session
from ml_service.errors import ProblemError
from ml_service.logging import get_logger
from ml_service.repository import cluster_fraud_rates, get_active_model
from ml_service.serving.loaded import LoadedSupervised, LoadedUnsupervised

log = get_logger(__name__)
Loaded = LoadedSupervised | LoadedUnsupervised


def no_active_model(kind: str) -> ProblemError:
    return ProblemError(
        404, "No active model", f"project has no active {kind} model", type_="no_active_model"
    )


class ModelCache:
    def __init__(self, capacity: int, refresh_seconds: int, negative_ttl: int) -> None:
        self.capacity = capacity
        self.refresh_seconds = refresh_seconds
        self.negative_ttl = negative_ttl
        self._items: OrderedDict[tuple[str, str], Loaded] = OrderedDict()
        self._negative: dict[tuple[str, str], float] = {}
        self._lock = threading.RLock()
        self._load_locks: dict[tuple[str, str], threading.Lock] = {}

    def get(self, tenant_id: UUID, project_id: UUID, kind: str) -> Loaded:
        key = (str(project_id), kind)
        now = time.monotonic()
        with self._lock:
            item = self._items.get(key)
            if item is not None and now - item.loaded_at < self.refresh_seconds:
                self._items.move_to_end(key)
                return item
            neg = self._negative.get(key)
            if item is None and neg is not None and now - neg < self.negative_ttl:
                raise no_active_model(kind)
            lock = self._load_locks.setdefault(key, threading.Lock())
        with lock:  # one loader per key; others wait and reuse
            with self._lock:
                fresh = self._items.get(key)
                if fresh is not None and time.monotonic() - fresh.loaded_at < self.refresh_seconds:
                    return fresh
            return self._load(tenant_id, project_id, kind, current=item)

    def _load(self, tenant_id: UUID, project_id: UUID, kind: str, current: Loaded | None) -> Loaded:
        key = (str(project_id), kind)
        with tenant_session(tenant_id) as conn:
            row = get_active_model(conn, project_id, kind)
            rates = cluster_fraud_rates(conn, row["id"]) if row is not None and kind == "unsupervised" else {}
        if row is None or not row["artifact_path"]:
            with self._lock:
                self._items.pop(key, None)
                self._negative[key] = time.monotonic()
            raise no_active_model(kind)
        if current is not None and current.model_id == str(row["id"]):
            current.loaded_at = time.monotonic()  # still the active one: just extend
            if isinstance(current, LoadedUnsupervised):
                current.cluster_fraud_rates = rates
            return current
        loaded = self._instantiate(
            kind, str(row["id"]), int(row["version"]), Path(row["artifact_path"]), rates
        )
        self._put(key, loaded)
        log.info("model_loaded", project_id=str(project_id), kind=kind, model_id=str(row["id"]))
        return loaded

    @staticmethod
    def _instantiate(
        kind: str, model_id: str, version: int, directory: Path, rates: dict[int, float | None]
    ) -> Loaded:
        try:
            if kind == "supervised":
                return LoadedSupervised.load(model_id, version, directory)
            return LoadedUnsupervised.load(model_id, version, directory, rates)
        except (KeyError, ValueError, FileNotFoundError) as exc:
            raise ProblemError(
                503,
                "Model unavailable",
                f"active {kind} model cannot be loaded: {exc}",
                type_="model_unavailable",
            ) from exc

    def _put(self, key: tuple[str, str], loaded: Loaded) -> None:
        with self._lock:
            self._items[key] = loaded
            self._items.move_to_end(key)
            self._negative.pop(key, None)
            while len(self._items) > self.capacity:
                self._items.popitem(last=False)

    def activate(
        self,
        project_id: UUID,
        kind: str,
        model_id: str,
        version: int,
        artifact_path: str,
        rates: dict[int, float | None] | None = None,
    ) -> None:
        """Hot swap after approval: load the new model now so the next request is served by it."""
        loaded = self._instantiate(kind, model_id, version, Path(artifact_path), rates or {})
        self._put((str(project_id), kind), loaded)

    def invalidate(self, project_id: UUID | None = None) -> None:
        with self._lock:
            if project_id is None:
                self._items.clear()
                self._negative.clear()
                return
            for key in [k for k in self._items if k[0] == str(project_id)]:
                self._items.pop(key, None)
            for key in [k for k in self._negative if k[0] == str(project_id)]:
                self._negative.pop(key, None)

    def stats(self) -> dict[str, Any]:
        with self._lock:
            return {"loaded": len(self._items), "capacity": self.capacity}


_cache: ModelCache | None = None


def get_cache() -> ModelCache:
    global _cache
    if _cache is None:
        s = get_settings()
        _cache = ModelCache(s.model_cache_size, s.model_refresh_seconds, s.no_model_negative_ttl_seconds)
    return _cache
