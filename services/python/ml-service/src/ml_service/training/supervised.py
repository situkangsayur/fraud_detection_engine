"""Supervised training job: labelled events → feature pipeline → plugin.fit → metrics → artifacts."""

from __future__ import annotations

import math
from dataclasses import dataclass
from typing import Any
from uuid import UUID

import numpy as np
from sklearn.model_selection import train_test_split

from ml_service.config import get_settings
from ml_service.db import tenant_session
from ml_service.features.catalog import FEATURE_SET_VERSION
from ml_service.features.pipeline import FeaturePipeline, build_row
from ml_service.logging import get_logger
from ml_service.plugins.base import SupervisedPlugin
from ml_service.plugins.registry import get_registry
from ml_service.repository import load_training_rows, update_model
from ml_service.training.common import (
    TrainingError,
    artifact_dir,
    fail_model,
    progress_reporter,
    write_meta,
)
from ml_service.training.metrics import permutation_importance, supervised_metrics

log = get_logger(__name__)

MIN_LABELLED = 20
MIN_PER_CLASS = 2


@dataclass(frozen=True)
class SupervisedJob:
    tenant_id: UUID
    project_id: UUID
    model_id: UUID
    algorithm: str
    params: dict[str, Any]
    features_config: dict[str, Any]
    since_days: int | None
    # Unlabelled events older than this many days count as legit (None → labelled events only).
    label_maturity_days: int | None = None


def time_split(
    X: np.ndarray, y: np.ndarray, val_fraction: float = 0.2
) -> tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray, str]:
    """Time-ordered split (rows are oldest → newest); falls back to a stratified split when a side lacks a class."""
    cut = math.floor(len(X) * (1 - val_fraction))
    X_tr, X_va, y_tr, y_va = X[:cut], X[cut:], y[:cut], y[cut:]
    if len(np.unique(y_tr)) == 2 and len(np.unique(y_va)) == 2:
        return X_tr, X_va, y_tr, y_va, "time"
    X_tr, X_va, y_tr, y_va = train_test_split(X, y, test_size=val_fraction, stratify=y, random_state=42)
    return X_tr, X_va, y_tr, y_va, "stratified_fallback"


def run_supervised(job: SupervisedJob) -> None:
    directory = artifact_dir(job.tenant_id, job.project_id, job.model_id)
    try:
        _run(job, directory)
    except TrainingError as exc:
        log.info("training_rejected", model_id=str(job.model_id), reason=str(exc))
        fail_model(job.tenant_id, job.model_id, str(exc), directory)
    except Exception as exc:
        log.exception("training_failed", model_id=str(job.model_id))
        fail_model(job.tenant_id, job.model_id, f"{type(exc).__name__}: {exc}", directory)


def _run(job: SupervisedJob, directory: Any) -> None:
    settings = get_settings()
    plugin_cls = get_registry().plugin_class(job.algorithm, "supervised")
    pipeline = FeaturePipeline.from_config(job.features_config)

    with tenant_session(job.tenant_id) as conn:
        rows = load_training_rows(
            conn,
            job.project_id,
            since_days=job.since_days,
            limit=settings.max_training_rows,
            labelled_only=True,
            include_payload=bool(pipeline.extras),
            mature_unlabelled_days=job.label_maturity_days,
        )
    y = np.array([1 if r["label"] == "fraud" else 0 for r in rows], dtype=np.int64)
    implicit_legit = sum(1 for r in rows if r["label"] is None)
    n_pos, n_neg = int(y.sum()), int(len(y) - y.sum())
    if len(rows) < MIN_LABELLED or n_pos < MIN_PER_CLASS or n_neg < MIN_PER_CLASS:
        raise TrainingError(
            f"not enough labelled data: {len(rows)} labelled events ({n_pos} fraud, {n_neg} legit); "
            f"need ≥ {MIN_LABELLED} with ≥ {MIN_PER_CLASS} per class"
        )

    feature_rows = [build_row(r["features"], r["payload"], pipeline.extras) for r in rows]
    cut = int(len(feature_rows) * 0.8)
    pipeline.fit(feature_rows[:cut] if cut >= MIN_LABELLED // 2 else feature_rows)  # fit on train side only
    X = pipeline.transform(feature_rows)
    names = pipeline.feature_names
    X_tr, X_va, y_tr, y_va, split = time_split(X, y)

    report = progress_reporter(job.tenant_id, job.model_id, 0.05, 0.85)
    plugin: SupervisedPlugin = plugin_cls(job.params)  # type: ignore[assignment]
    history = plugin.fit(X_tr, y_tr, X_va, y_va, names, report)

    val_proba = plugin.predict_proba(X_va)
    metrics = supervised_metrics(y_va, val_proba)
    metrics["train"] = {"n": len(y_tr), "positives": int(y_tr.sum())}
    metrics["split"] = split
    metrics["class_balance"] = {
        "fraud": n_pos,
        "legit": n_neg,
        "fraud_rate": round(n_pos / len(y), 6),
        "implicit_legit": implicit_legit,
        "label_maturity_days": job.label_maturity_days,
    }
    metrics["feature_importance"] = permutation_importance(plugin, X_va, y_va, names)
    report(0.95)

    directory.mkdir(parents=True, exist_ok=True)
    pipeline.save(directory / "pipeline.joblib")
    plugin.save(directory / "supervised")
    write_meta(
        directory,
        {
            "kind": "supervised",
            "algorithm": {"name": plugin_cls.name, "version": plugin_cls.version},
            "feature_names": names,
            "feature_set_version": FEATURE_SET_VERSION,
        },
    )
    with tenant_session(job.tenant_id) as conn:
        update_model(
            conn,
            job.model_id,
            status="ready",
            progress=1.0,
            metrics=metrics,
            training_history=history,
            feature_names=names,
            artifact_path=str(directory),
            trained_rows=len(y_tr),
            training_finished_at="now",
            error=None,
        )
    log.info("training_done", model_id=str(job.model_id), pr_auc=metrics["pr_auc"], rows=len(rows))
