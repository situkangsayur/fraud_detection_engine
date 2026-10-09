"""Unsupervised training job: anomaly plugin + clustering plugin on the same feature matrix.

Stores per-event anomaly score, cluster and 2-D PCA coordinates (ml.event_anomaly) and per-cluster profiles
(ml.clusters) so analysts can inspect groups of anomalies.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Any
from uuid import UUID

import joblib
import numpy as np
from sklearn.decomposition import PCA
from sklearn.metrics import roc_auc_score, silhouette_score

from ml_service.config import get_settings
from ml_service.db import tenant_session
from ml_service.features.catalog import FEATURE_SET_VERSION
from ml_service.features.pipeline import FeaturePipeline, build_row
from ml_service.logging import get_logger
from ml_service.plugins.base import AnomalyPlugin, ClusteringPlugin
from ml_service.plugins.registry import get_registry
from ml_service.repository import load_training_rows, replace_clusters, replace_event_anomaly, update_model
from ml_service.training.common import TrainingError, artifact_dir, fail_model, progress_reporter, write_meta

log = get_logger(__name__)
MIN_ROWS = 20


@dataclass(frozen=True)
class UnsupervisedJob:
    tenant_id: UUID
    project_id: UUID
    model_id: UUID
    anomaly_algorithm: str
    anomaly_params: dict[str, Any]
    clustering_algorithm: str
    clustering_params: dict[str, Any]
    features_config: dict[str, Any]
    since_days: int | None


@dataclass
class ClusterAssigner:
    """Nearest-centroid assignment for clusterers without `predict`; far points (> radius) become noise (-1)."""

    cluster_ids: list[int] = field(default_factory=list)
    centroids: np.ndarray = field(default_factory=lambda: np.zeros((0, 0)))
    radii: np.ndarray = field(default_factory=lambda: np.zeros(0))

    @classmethod
    def fit(cls, X: np.ndarray, labels: np.ndarray) -> ClusterAssigner:
        ids = sorted(int(c) for c in set(labels.tolist()) if c != -1)
        if not ids:
            return cls()
        centroids = np.stack([X[labels == c].mean(axis=0) for c in ids])
        radii = np.array(
            [
                float(np.percentile(np.linalg.norm(X[labels == c] - centroids[i], axis=1), 95)) + 1e-9
                for i, c in enumerate(ids)
            ]
        )
        return cls(ids, centroids, radii)

    def assign(self, X: np.ndarray) -> np.ndarray:
        if not self.cluster_ids:
            return np.full(len(X), -1, dtype=np.int64)
        dist = np.linalg.norm(X[:, None, :] - self.centroids[None, :, :], axis=2)
        nearest = dist.argmin(axis=1)
        within = dist[np.arange(len(X)), nearest] <= self.radii[nearest]
        return np.where(within, np.array(self.cluster_ids)[nearest], -1).astype(np.int64)


def run_unsupervised(job: UnsupervisedJob) -> None:
    directory = artifact_dir(job.tenant_id, job.project_id, job.model_id)
    try:
        _run(job, directory)
    except TrainingError as exc:
        fail_model(job.tenant_id, job.model_id, str(exc), directory)
    except Exception as exc:
        log.exception("unsupervised_training_failed", model_id=str(job.model_id))
        fail_model(job.tenant_id, job.model_id, f"{type(exc).__name__}: {exc}", directory)


def cluster_summaries(
    X: np.ndarray,
    raw_numeric: np.ndarray,
    numeric_names: list[str],
    feature_names: list[str],
    labels: np.ndarray,
    fraud: np.ndarray,
    labelled: np.ndarray,
) -> list[dict[str, Any]]:
    overall_mean = X.mean(axis=0)
    overall_std = X.std(axis=0)
    overall_std[overall_std < 1e-9] = 1.0
    out: list[dict[str, Any]] = []
    for cid in sorted(set(labels.tolist())):
        mask = labels == cid
        size = int(mask.sum())
        n_lab = int((mask & labelled).sum())
        fraud_rate = float((mask & labelled & fraud).sum() / n_lab) if n_lab else None
        centroid = X[mask].mean(axis=0)
        smd = (centroid - overall_mean) / overall_std
        order = np.argsort(-np.abs(smd))[:5]
        out.append(
            {
                "cluster_id": int(cid),
                "size": size,
                "labeled_count": n_lab,
                "fraud_rate": fraud_rate,
                "centroid": [round(float(v), 5) for v in centroid],
                "profile": {
                    n: round(float(v), 5)
                    for n, v in zip(numeric_names, raw_numeric[mask].mean(axis=0), strict=True)
                },
                "top_features": [
                    {
                        "feature": feature_names[i],
                        "smd": round(float(smd[i]), 4),
                        "cluster_mean": round(float(centroid[i]), 4),
                        "overall_mean": round(float(overall_mean[i]), 4),
                    }
                    for i in order
                ],
            }
        )
    return out


def _run(job: UnsupervisedJob, directory: Path) -> None:
    settings = get_settings()
    registry = get_registry()
    anomaly_cls = registry.plugin_class(job.anomaly_algorithm, "anomaly")
    clustering_cls = registry.plugin_class(job.clustering_algorithm, "clustering")
    pipeline = FeaturePipeline.from_config(job.features_config)

    with tenant_session(job.tenant_id) as conn:
        rows = load_training_rows(
            conn,
            job.project_id,
            since_days=job.since_days,
            limit=settings.max_unsupervised_rows,
            labelled_only=False,
            include_payload=bool(pipeline.extras),
        )
    if len(rows) < MIN_ROWS:
        raise TrainingError(f"not enough events: {len(rows)} (need ≥ {MIN_ROWS})")

    feature_rows = [build_row(r["features"], r["payload"], pipeline.extras) for r in rows]
    X = pipeline.fit_transform(feature_rows)
    names = pipeline.feature_names
    labelled = np.array([r["label"] is not None for r in rows])
    fraud = np.array([r["label"] == "fraud" for r in rows])

    anomaly: AnomalyPlugin = anomaly_cls(job.anomaly_params)  # type: ignore[assignment]
    anomaly_history = anomaly.fit(X, names, progress_reporter(job.tenant_id, job.model_id, 0.05, 0.45))
    scores = anomaly.score(X)

    clustering: ClusteringPlugin = clustering_cls(job.clustering_params)  # type: ignore[assignment]
    clustering_history = clustering.fit(X, names, progress_reporter(job.tenant_id, job.model_id, 0.45, 0.8))
    labels = np.asarray(clustering.labels(), dtype=np.int64)
    assigner = ClusterAssigner.fit(X, labels)

    pca = PCA(n_components=2, random_state=42) if X.shape[1] >= 2 else None
    coords = pca.fit_transform(X) if pca is not None else np.zeros((len(X), 2))

    clusters = cluster_summaries(
        X, pipeline.raw_numeric(feature_rows), pipeline.numeric_columns, names, labels, fraud, labelled
    )
    metrics = _metrics(X, scores, labels, fraud, labelled)

    directory.mkdir(parents=True, exist_ok=True)
    pipeline.save(directory / "pipeline.joblib")
    anomaly.save(directory / "anomaly")
    clustering.save(directory / "clustering")
    joblib.dump(assigner, directory / "assigner.joblib")
    if pca is not None:
        joblib.dump(pca, directory / "pca.joblib")
    write_meta(
        directory,
        {
            "kind": "unsupervised",
            "anomaly": {"name": anomaly_cls.name, "version": anomaly_cls.version},
            "clustering": {
                "name": clustering_cls.name,
                "version": clustering_cls.version,
                "supports_predict": bool(getattr(clustering_cls, "supports_predict", False)),
            },
            "feature_names": names,
            "feature_set_version": FEATURE_SET_VERSION,
        },
    )

    anomaly_rows = [
        {
            "event_id": str(r["event_id"]),
            "anomaly_score": float(s),
            "cluster_id": int(c),
            "pca_x": float(xy[0]),
            "pca_y": float(xy[1]),
        }
        for r, s, c, xy in zip(rows, scores, labels, coords, strict=True)
    ]
    with tenant_session(job.tenant_id) as conn:
        replace_event_anomaly(conn, job.tenant_id, job.model_id, anomaly_rows)
        replace_clusters(conn, job.tenant_id, job.model_id, clusters)
        update_model(
            conn,
            job.model_id,
            status="ready",
            progress=1.0,
            metrics=metrics,
            feature_names=names,
            training_history={"anomaly": anomaly_history, "clustering": clustering_history},
            artifact_path=str(directory),
            trained_rows=len(rows),
            training_finished_at="now",
            error=None,
        )
    log.info(
        "unsupervised_training_done",
        model_id=str(job.model_id),
        rows=len(rows),
        clusters=metrics["n_clusters"],
    )


def _metrics(
    X: np.ndarray, scores: np.ndarray, labels: np.ndarray, fraud: np.ndarray, labelled: np.ndarray
) -> dict[str, Any]:
    cluster_ids = set(labels.tolist()) - {-1}
    metrics: dict[str, Any] = {
        "n_rows": len(X),
        "n_clusters": len(cluster_ids),
        "noise_ratio": round(float((labels == -1).mean()), 6),
        "anomaly_score_quantiles": {
            q: round(float(np.quantile(scores, float(q))), 6) for q in ("0.5", "0.9", "0.95", "0.99")
        },
        "labelled_rows": int(labelled.sum()),
        "anomaly_roc_auc_vs_labels": None,
        "silhouette": None,
    }
    lab_scores, lab_fraud = scores[labelled], fraud[labelled]
    if len(set(lab_fraud.tolist())) == 2:
        metrics["anomaly_roc_auc_vs_labels"] = round(float(roc_auc_score(lab_fraud, lab_scores)), 6)
    mask = labels != -1
    if len(cluster_ids) >= 2 and mask.sum() > len(cluster_ids):
        idx = np.flatnonzero(mask)
        if len(idx) > 2000:
            idx = np.random.default_rng(42).choice(idx, 2000, replace=False)
        if len(set(labels[idx].tolist())) >= 2:
            metrics["silhouette"] = round(float(silhouette_score(X[idx], labels[idx])), 6)
    return metrics
