"""Plugin interfaces.

Plugins only see numpy arrays: the platform owns data loading, feature pipelines, splits, metrics, artifact
storage, the registry and serving. A plugin must never touch the database or the network.
"""

from __future__ import annotations

import json
from abc import ABC, abstractmethod
from collections.abc import Callable
from pathlib import Path
from typing import Any, ClassVar, Literal

import numpy as np
from jsonschema import Draft202012Validator

PluginKind = Literal["supervised", "anomaly", "clustering"]
ProgressCallback = Callable[[float], None]


def _noop_progress(_: float) -> None:
    return None


NOOP_PROGRESS: ProgressCallback = _noop_progress


def schema_defaults(schema: dict[str, Any]) -> dict[str, Any]:
    """Top-level defaults declared in a JSON Schema's `properties`."""
    props = schema.get("properties", {})
    return {k: v["default"] for k, v in props.items() if isinstance(v, dict) and "default" in v}


def resolve_params(schema: dict[str, Any], params: dict[str, Any] | None) -> dict[str, Any]:
    """Merge user params over schema defaults and validate. Raises `ValueError` with all messages."""
    merged = {**schema_defaults(schema), **(params or {})}
    errors = sorted(Draft202012Validator(schema).iter_errors(merged), key=lambda e: list(e.path))
    if errors:
        raise ValueError(
            "; ".join(f"{'.'.join(str(p) for p in e.path) or '<root>'}: {e.message}" for e in errors)
        )
    return merged


class AlgorithmPlugin(ABC):
    name: ClassVar[str]
    kind: ClassVar[PluginKind]
    version: ClassVar[str]
    display_name: ClassVar[str]
    description: ClassVar[str]
    param_schema: ClassVar[dict[str, Any]]
    # Param overrides used only by the registry smoke test (e.g. fewer epochs) to keep reloads fast.
    smoke_params: ClassVar[dict[str, Any]] = {}

    def __init__(self, params: dict[str, Any] | None = None) -> None:
        self.params: dict[str, Any] = resolve_params(self.param_schema, params)

    @abstractmethod
    def save(self, directory: Path) -> None: ...

    @classmethod
    @abstractmethod
    def load(cls, directory: Path) -> AlgorithmPlugin: ...

    # helpers for implementations -------------------------------------------------------------
    def _write_params(self, directory: Path) -> None:
        directory.mkdir(parents=True, exist_ok=True)
        (directory / "params.json").write_text(json.dumps(self.params))

    @staticmethod
    def _read_params(directory: Path) -> dict[str, Any]:
        return json.loads((directory / "params.json").read_text())


class SupervisedPlugin(AlgorithmPlugin):
    kind: ClassVar[PluginKind] = "supervised"

    @abstractmethod
    def fit(
        self,
        X_train: np.ndarray,
        y_train: np.ndarray,
        X_val: np.ndarray,
        y_val: np.ndarray,
        feature_names: list[str],
        progress: ProgressCallback = NOOP_PROGRESS,
    ) -> dict[str, Any]:
        """Train; return a JSON-serialisable training history."""

    @abstractmethod
    def predict_proba(self, X: np.ndarray) -> np.ndarray:
        """P(fraud) with shape (n,)."""

    def explain(
        self, X: np.ndarray, feature_names: list[str], top_k: int = 5
    ) -> list[list[tuple[str, float]]]:
        """Local sensitivity: set each feature to 0 (= training mean after scaling) and measure Δ probability.

        One batched predict call per row keeps single-row latency low.
        """
        results: list[list[tuple[str, float]]] = []
        n_features = X.shape[1]
        for row in X:
            base = float(self.predict_proba(row.reshape(1, -1))[0])
            perturbed = np.repeat(row.reshape(1, -1), n_features, axis=0)
            perturbed[np.arange(n_features), np.arange(n_features)] = 0.0
            deltas = base - self.predict_proba(perturbed)
            order = np.argsort(-np.abs(deltas))[:top_k]
            results.append([(feature_names[i], float(deltas[i])) for i in order if deltas[i] != 0.0])
        return results


class AnomalyPlugin(AlgorithmPlugin):
    kind: ClassVar[PluginKind] = "anomaly"

    @abstractmethod
    def fit(
        self, X: np.ndarray, feature_names: list[str], progress: ProgressCallback = NOOP_PROGRESS
    ) -> dict[str, Any]: ...

    @abstractmethod
    def score(self, X: np.ndarray) -> np.ndarray:
        """Anomaly score in [0, 1], higher = more anomalous."""


class ClusteringPlugin(AlgorithmPlugin):
    kind: ClassVar[PluginKind] = "clustering"
    supports_predict: ClassVar[bool] = False  # False → platform uses nearest-centroid assignment

    @abstractmethod
    def fit(
        self, X: np.ndarray, feature_names: list[str], progress: ProgressCallback = NOOP_PROGRESS
    ) -> dict[str, Any]: ...

    @abstractmethod
    def labels(self) -> np.ndarray:
        """Cluster labels of the training rows (-1 = noise)."""

    def predict(self, X: np.ndarray) -> np.ndarray:
        raise NotImplementedError(f"{self.name} does not support predict")


PLUGIN_BASES: dict[str, type[Any]] = {
    "supervised": SupervisedPlugin,
    "anomaly": AnomalyPlugin,
    "clustering": ClusteringPlugin,
}


class EmpiricalCdf:
    """Maps raw anomaly scores to [0, 1] by their rank within the training distribution.

    Gives every anomaly plugin the same, interpretable scale: 0.99 = more anomalous than 99% of training data.
    """

    def __init__(self, reference: np.ndarray, max_points: int = 10_000) -> None:
        ref = np.sort(np.asarray(reference, dtype=np.float64).ravel())
        if ref.size > max_points:
            ref = ref[np.linspace(0, ref.size - 1, max_points).astype(int)]
        self.reference = ref

    def __call__(self, raw: np.ndarray) -> np.ndarray:
        if self.reference.size == 0:
            return np.zeros_like(np.asarray(raw, dtype=np.float64))
        ranks = np.searchsorted(self.reference, np.asarray(raw, dtype=np.float64), side="right")
        return np.clip(ranks / self.reference.size, 0.0, 1.0)

    def to_list(self) -> list[float]:
        return [float(v) for v in self.reference]

    @classmethod
    def from_list(cls, values: list[float]) -> EmpiricalCdf:
        return cls(np.asarray(values, dtype=np.float64))
