"""Loaded (in-memory) models ready for low-latency inference."""

from __future__ import annotations

import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

import joblib
import numpy as np

from ml_service.features.pipeline import FeaturePipeline, build_row
from ml_service.plugins.base import AnomalyPlugin, ClusteringPlugin, SupervisedPlugin
from ml_service.plugins.registry import get_registry
from ml_service.training.common import read_meta
from ml_service.training.unsupervised import ClusterAssigner


@dataclass
class LoadedSupervised:
    model_id: str
    version: int
    algorithm: str
    pipeline: FeaturePipeline
    plugin: SupervisedPlugin
    loaded_at: float = field(default_factory=time.monotonic)

    @classmethod
    def load(cls, model_id: str, version: int, directory: Path) -> LoadedSupervised:
        meta = read_meta(directory)
        name = meta["algorithm"]["name"]
        plugin_cls = get_registry().plugin_class(name, "supervised")
        plugin = plugin_cls.load(directory / "supervised")
        return cls(model_id, version, name, FeaturePipeline.load(directory / "pipeline.joblib"), plugin)  # type: ignore[arg-type]

    def predict(
        self, features: dict[str, Any], source: dict[str, Any] | None, explain: bool, top_k: int = 5
    ) -> tuple[float, list[dict[str, Any]]]:
        X = self.pipeline.transform([build_row(features, source, self.pipeline.extras)])
        proba = float(self.plugin.predict_proba(X)[0])
        top: list[dict[str, Any]] = []
        if explain:
            top = [
                {"name": n, "contribution": round(c, 6)}
                for n, c in self.plugin.explain(X, self.pipeline.feature_names, top_k)[0]
            ]
        return proba, top

    def predict_batch(self, items: list[tuple[dict[str, Any], dict[str, Any] | None]]) -> np.ndarray:
        X = self.pipeline.transform([build_row(f, s, self.pipeline.extras) for f, s in items])
        return self.plugin.predict_proba(X)


@dataclass
class LoadedUnsupervised:
    model_id: str
    version: int
    pipeline: FeaturePipeline
    anomaly: AnomalyPlugin
    clustering: ClusteringPlugin | None  # only when the plugin supports predict
    assigner: ClusterAssigner
    cluster_fraud_rates: dict[int, float | None]
    loaded_at: float = field(default_factory=time.monotonic)

    @classmethod
    def load(
        cls, model_id: str, version: int, directory: Path, fraud_rates: dict[int, float | None]
    ) -> LoadedUnsupervised:
        meta = read_meta(directory)
        registry = get_registry()
        anomaly_cls = registry.plugin_class(meta["anomaly"]["name"], "anomaly")
        clustering: ClusteringPlugin | None = None
        if meta["clustering"].get("supports_predict"):
            clustering_cls = registry.plugin_class(meta["clustering"]["name"], "clustering")
            clustering = clustering_cls.load(directory / "clustering")  # type: ignore[assignment]
        return cls(
            model_id=model_id,
            version=version,
            pipeline=FeaturePipeline.load(directory / "pipeline.joblib"),
            anomaly=anomaly_cls.load(directory / "anomaly"),  # type: ignore[arg-type]
            clustering=clustering,
            assigner=joblib.load(directory / "assigner.joblib"),
            cluster_fraud_rates=fraud_rates,
        )

    def score(
        self, features: dict[str, Any], source: dict[str, Any] | None
    ) -> tuple[float, int, float | None]:
        X = self.pipeline.transform([build_row(features, source, self.pipeline.extras)])
        anomaly = float(self.anomaly.score(X)[0])
        cluster = (
            int(self.clustering.predict(X)[0])
            if self.clustering is not None
            else int(self.assigner.assign(X)[0])
        )
        return anomaly, cluster, self.cluster_fraud_rates.get(cluster)
