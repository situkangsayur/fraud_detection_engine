"""Evaluation metrics (ml-plugins.md §4)."""

from __future__ import annotations

from itertools import pairwise
from typing import Any

import numpy as np
from sklearn.metrics import average_precision_score, roc_auc_score

from ml_service.plugins.base import SupervisedPlugin

THRESHOLDS = (0.3, 0.5, 0.7)


def _safe(fn: Any, y: np.ndarray, p: np.ndarray) -> float | None:
    if len(np.unique(y)) < 2:
        return None
    return float(fn(y, p))


def confusion(y: np.ndarray, p: np.ndarray, threshold: float) -> dict[str, int]:
    pred = p >= threshold
    return {
        "tp": int(np.sum(pred & (y == 1))),
        "fp": int(np.sum(pred & (y == 0))),
        "tn": int(np.sum(~pred & (y == 0))),
        "fn": int(np.sum(~pred & (y == 1))),
    }


def threshold_metrics(y: np.ndarray, p: np.ndarray, threshold: float) -> dict[str, float]:
    c = confusion(y, p, threshold)
    precision = c["tp"] / (c["tp"] + c["fp"]) if c["tp"] + c["fp"] else 0.0
    recall = c["tp"] / (c["tp"] + c["fn"]) if c["tp"] + c["fn"] else 0.0
    f1 = 2 * precision * recall / (precision + recall) if precision + recall else 0.0
    return {
        "threshold": threshold,
        "precision": round(precision, 6),
        "recall": round(recall, 6),
        "f1": round(f1, 6),
        "flagged_rate": round(float(np.mean(p >= threshold)) if len(p) else 0.0, 6),
    }


def calibration_bins(y: np.ndarray, p: np.ndarray, bins: int = 10) -> list[dict[str, float | int]]:
    edges = np.linspace(0.0, 1.0, bins + 1)
    out: list[dict[str, float | int]] = []
    for lo, hi in pairwise(edges):
        mask = (p >= lo) & ((p < hi) if hi < 1.0 else (p <= hi))
        count = int(mask.sum())
        out.append(
            {
                "bin_lower": round(float(lo), 2),
                "bin_upper": round(float(hi), 2),
                "count": count,
                "mean_predicted": round(float(p[mask].mean()), 6) if count else 0.0,
                "observed_rate": round(float(y[mask].mean()), 6) if count else 0.0,
            }
        )
    return out


def supervised_metrics(y: np.ndarray, p: np.ndarray) -> dict[str, Any]:
    y = np.asarray(y).astype(int)
    p = np.asarray(p, dtype=np.float64)
    return {
        "roc_auc": _safe(roc_auc_score, y, p),
        "pr_auc": _safe(average_precision_score, y, p),
        "thresholds": [threshold_metrics(y, p, t) for t in THRESHOLDS],
        "confusion_matrix": confusion(y, p, 0.5),
        "calibration": calibration_bins(y, p),
        "n": len(y),
        "positives": int(y.sum()),
    }


def permutation_importance(
    plugin: SupervisedPlugin,
    X: np.ndarray,
    y: np.ndarray,
    feature_names: list[str],
    *,
    max_rows: int = 2000,
    seed: int = 42,
    top_k: int = 25,
) -> list[dict[str, float | str]]:
    """Drop in PR-AUC when a column is shuffled (validation set). Falls back to ROC-AUC-free Brier delta."""
    rng = np.random.default_rng(seed)
    if len(X) > max_rows:
        idx = rng.choice(len(X), max_rows, replace=False)
        X, y = X[idx], y[idx]
    if len(np.unique(y)) < 2 or len(X) == 0:
        return []
    base = float(average_precision_score(y, plugin.predict_proba(X)))
    scores: list[dict[str, float | str]] = []
    for j, name in enumerate(feature_names):
        column = X[:, j]
        if np.all(column == column[0]):
            continue
        Xp = X.copy()
        Xp[:, j] = rng.permutation(column)
        drop = base - float(average_precision_score(y, plugin.predict_proba(Xp)))
        scores.append({"feature": name, "importance": round(drop, 6)})
    scores.sort(key=lambda s: -float(s["importance"]))
    return scores[:top_k]
