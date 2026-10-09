"""Built-in clustering plugins."""

from __future__ import annotations

from pathlib import Path
from typing import Any, ClassVar

import joblib
import numpy as np
from sklearn.cluster import DBSCAN, HDBSCAN, KMeans
from sklearn.mixture import GaussianMixture

from ml_service.plugins.base import NOOP_PROGRESS, ClusteringPlugin, ProgressCallback


class _SklearnClustering(ClusteringPlugin):
    def __init__(self, params: dict[str, Any] | None = None) -> None:
        super().__init__(params)
        self._estimator: Any = None
        self._labels: np.ndarray | None = None

    def _make(self, n_rows: int) -> Any:
        raise NotImplementedError

    def fit(
        self, X: np.ndarray, feature_names: list[str], progress: ProgressCallback = NOOP_PROGRESS
    ) -> dict[str, Any]:
        self._estimator = self._make(len(X))
        if hasattr(self._estimator, "fit_predict"):
            self._labels = np.asarray(self._estimator.fit_predict(X), dtype=np.int64)
        else:
            self._estimator.fit(X)
            self._labels = np.asarray(self._estimator.predict(X), dtype=np.int64)
        progress(1.0)
        uniq = set(self._labels.tolist())
        return {"n_clusters": len(uniq - {-1}), "noise_points": int((self._labels == -1).sum())}

    def labels(self) -> np.ndarray:
        if self._labels is None:
            raise RuntimeError("model not fitted")
        return self._labels

    def predict(self, X: np.ndarray) -> np.ndarray:
        if not self.supports_predict:
            raise NotImplementedError(f"{self.name} does not support predict")
        return np.asarray(self._estimator.predict(X), dtype=np.int64)

    def save(self, directory: Path) -> None:
        self._write_params(directory)
        # Non-predicting estimators keep O(n) training state; only the labels are needed after fit.
        if self.supports_predict:
            joblib.dump(self._estimator, directory / "model.joblib")
        np.save(directory / "labels.npy", self.labels())

    @classmethod
    def load(cls, directory: Path) -> _SklearnClustering:
        plugin = cls(cls._read_params(directory))
        if cls.supports_predict:
            plugin._estimator = joblib.load(directory / "model.joblib")
        plugin._labels = np.load(directory / "labels.npy")
        return plugin


class HdbscanPlugin(_SklearnClustering):
    name = "hdbscan"
    version = "1.0.0"
    display_name = "HDBSCAN"
    description = "Hierarchical density clustering; finds clusters of varying density and marks noise (-1)."
    supports_predict = False
    param_schema: ClassVar[dict[str, Any]] = {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "additionalProperties": False,
        "properties": {
            "min_cluster_size": {"type": "integer", "minimum": 2, "default": 15},
            "min_samples": {"type": ["integer", "null"], "minimum": 1, "default": None},
            "cluster_selection_epsilon": {"type": "number", "minimum": 0, "default": 0.0},
        },
    }

    def _make(self, n_rows: int) -> Any:
        p = self.params
        return HDBSCAN(
            min_cluster_size=min(int(p["min_cluster_size"]), max(n_rows, 2)),
            min_samples=p["min_samples"],
            cluster_selection_epsilon=float(p["cluster_selection_epsilon"]),
        )


class KMeansPlugin(_SklearnClustering):
    name = "kmeans"
    version = "1.0.0"
    display_name = "K-means"
    description = "Partitions data into k spherical clusters. No noise label."
    supports_predict = True
    param_schema: ClassVar[dict[str, Any]] = {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "additionalProperties": False,
        "properties": {
            "n_clusters": {"type": "integer", "minimum": 2, "maximum": 500, "default": 8},
            "n_init": {"type": "integer", "minimum": 1, "maximum": 100, "default": 10},
            "max_iter": {"type": "integer", "minimum": 10, "default": 300},
            "seed": {"type": "integer", "default": 42},
        },
    }

    def _make(self, n_rows: int) -> Any:
        p = self.params
        return KMeans(
            n_clusters=min(int(p["n_clusters"]), max(n_rows, 1)),
            n_init=int(p["n_init"]),
            max_iter=int(p["max_iter"]),
            random_state=int(p["seed"]),
        )


class DbscanPlugin(_SklearnClustering):
    name = "dbscan"
    version = "1.0.0"
    display_name = "DBSCAN"
    description = "Density clustering with a fixed radius (eps); points in no dense region are noise (-1)."
    supports_predict = False
    param_schema: ClassVar[dict[str, Any]] = {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "additionalProperties": False,
        "properties": {
            "eps": {"type": "number", "exclusiveMinimum": 0, "default": 0.5},
            "min_samples": {"type": "integer", "minimum": 1, "default": 5},
            "metric": {
                "type": "string",
                "enum": ["euclidean", "manhattan", "cosine"],
                "default": "euclidean",
            },
        },
    }

    def _make(self, n_rows: int) -> Any:
        p = self.params
        return DBSCAN(eps=float(p["eps"]), min_samples=int(p["min_samples"]), metric=p["metric"])


class GaussianMixturePlugin(_SklearnClustering):
    name = "gaussian_mixture"
    version = "1.0.0"
    display_name = "Gaussian mixture"
    description = "Soft clustering with a mixture of Gaussians (hard assignment = most likely component)."
    supports_predict = True
    param_schema: ClassVar[dict[str, Any]] = {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "additionalProperties": False,
        "properties": {
            "n_components": {"type": "integer", "minimum": 1, "maximum": 200, "default": 8},
            "covariance_type": {
                "type": "string",
                "enum": ["full", "tied", "diag", "spherical"],
                "default": "diag",
            },
            "max_iter": {"type": "integer", "minimum": 10, "default": 200},
            "seed": {"type": "integer", "default": 42},
        },
    }

    def _make(self, n_rows: int) -> Any:
        p = self.params
        return GaussianMixture(
            n_components=min(int(p["n_components"]), max(n_rows, 1)),
            covariance_type=p["covariance_type"],
            max_iter=int(p["max_iter"]),
            random_state=int(p["seed"]),
            reg_covar=1e-4,
        )


PLUGINS: list[type[ClusteringPlugin]] = [HdbscanPlugin, KMeansPlugin, DbscanPlugin, GaussianMixturePlugin]
