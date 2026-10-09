"""Example external ML plugin: k-nearest-neighbour distance anomaly detector.

Drop-in plugin for ml-service (see docs/technical/ml-plugins.md):
  1. copy this file into the plugins folder (mounted at /plugins in the ml-service container)
  2. POST /api/v1/ml/algorithms/reload  (platform admin)
  3. select "knn_distance_anomaly" as the project's unsupervised.anomaly_algorithm

A plugin only works with numpy arrays; it must not access the database or the network.
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any, ClassVar

import joblib
import numpy as np
from sklearn.neighbors import NearestNeighbors

from ml_service.plugins.base import NOOP_PROGRESS, AnomalyPlugin, EmpiricalCdf, ProgressCallback


class KnnDistanceAnomaly(AnomalyPlugin):
    name = "knn_distance_anomaly"
    version = "1.0.0"
    display_name = "k-NN distance anomaly (example plugin)"
    description = "Anomaly score = mean distance to the k nearest training neighbours (rank-normalised)."
    param_schema: ClassVar[dict[str, Any]] = {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "additionalProperties": False,
        "properties": {
            "k": {"type": "integer", "minimum": 1, "maximum": 200, "default": 10},
            "metric": {"type": "string", "enum": ["euclidean", "manhattan"], "default": "euclidean"},
        },
    }

    def __init__(self, params: dict[str, Any] | None = None) -> None:
        super().__init__(params)
        self._nn: NearestNeighbors | None = None
        self._cdf: EmpiricalCdf | None = None

    def fit(
        self, X: np.ndarray, feature_names: list[str], progress: ProgressCallback = NOOP_PROGRESS
    ) -> dict[str, Any]:
        k = min(int(self.params["k"]), max(len(X) - 1, 1))
        self._nn = NearestNeighbors(n_neighbors=k + 1, metric=self.params["metric"]).fit(X)
        # +1 neighbour and drop column 0: a training point's nearest neighbour is itself
        distances, _ = self._nn.kneighbors(X)
        self._cdf = EmpiricalCdf(distances[:, 1:].mean(axis=1))
        progress(1.0)
        return {"k_used": k}

    def score(self, X: np.ndarray) -> np.ndarray:
        if self._nn is None or self._cdf is None:
            raise RuntimeError("model not fitted")
        distances, _ = self._nn.kneighbors(X, n_neighbors=self._nn.n_neighbors - 1)
        return self._cdf(distances.mean(axis=1))

    def save(self, directory: Path) -> None:
        self._write_params(directory)
        joblib.dump(self._nn, directory / "nn.joblib")
        (directory / "cdf.json").write_text(json.dumps(self._cdf.to_list() if self._cdf else []))

    @classmethod
    def load(cls, directory: Path) -> KnnDistanceAnomaly:
        plugin = cls(cls._read_params(directory))
        plugin._nn = joblib.load(directory / "nn.joblib")
        plugin._cdf = EmpiricalCdf.from_list(json.loads((directory / "cdf.json").read_text()))
        return plugin


PLUGINS = [KnnDistanceAnomaly]
