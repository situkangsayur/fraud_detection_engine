from __future__ import annotations

import numpy as np

from ml_service.plugins.builtin.supervised import LogisticRegressionPlugin
from ml_service.plugins.smoke import synthetic_dataset
from ml_service.training.metrics import (
    calibration_bins,
    permutation_importance,
    supervised_metrics,
    threshold_metrics,
)
from ml_service.training.supervised import time_split


def test_supervised_metrics_perfect_and_degenerate() -> None:
    y = np.array([0, 0, 1, 1])
    m = supervised_metrics(y, np.array([0.1, 0.2, 0.8, 0.9]))
    assert m["roc_auc"] == 1.0 and m["pr_auc"] == 1.0
    assert m["confusion_matrix"] == {"tp": 2, "fp": 0, "tn": 2, "fn": 0}
    assert [t["threshold"] for t in m["thresholds"]] == [0.3, 0.5, 0.7]
    single = supervised_metrics(np.array([0, 0]), np.array([0.1, 0.2]))
    assert single["roc_auc"] is None and single["pr_auc"] is None


def test_threshold_metrics_values() -> None:
    t = threshold_metrics(np.array([1, 0, 1, 0]), np.array([0.9, 0.6, 0.4, 0.1]), 0.5)
    assert t["precision"] == 0.5 and t["recall"] == 0.5 and t["f1"] == 0.5 and t["flagged_rate"] == 0.5


def test_calibration_bins_cover_all_rows() -> None:
    p = np.linspace(0, 1, 101)
    bins = calibration_bins((p > 0.5).astype(int), p)
    assert len(bins) == 10 and sum(b["count"] for b in bins) == 101


def test_permutation_importance_ranks_informative_feature_first() -> None:
    rng = np.random.default_rng(0)
    X = rng.normal(size=(500, 3)).astype(np.float32)
    y = (X[:, 1] > 0.8).astype(int)
    plugin = LogisticRegressionPlugin()
    plugin.fit(X, y, X, y, ["noise_a", "signal", "noise_b"])
    ranked = permutation_importance(plugin, X, y, ["noise_a", "signal", "noise_b"])
    assert ranked[0]["feature"] == "signal"


def test_time_split_falls_back_when_validation_has_no_positives() -> None:
    X, _ = synthetic_dataset(n=100, d=2)
    y = np.zeros(100, dtype=int)
    y[:10] = 1  # all positives are old → time split has none in validation
    _, _, y_tr, y_va, how = time_split(X, y)
    assert how == "stratified_fallback" and y_va.sum() > 0 and y_tr.sum() > 0
    y2 = np.tile([0, 0, 0, 1], 25)
    assert time_split(X, y2)[4] == "time"
