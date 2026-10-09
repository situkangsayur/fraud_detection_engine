"""Synthetic smoke test every plugin must pass before it becomes selectable."""

from __future__ import annotations

import tempfile
from pathlib import Path

import numpy as np

from ml_service.plugins.base import AlgorithmPlugin, AnomalyPlugin, ClusteringPlugin, SupervisedPlugin


def synthetic_dataset(
    n: int = 240, d: int = 6, fraud_rate: float = 0.1, seed: int = 7
) -> tuple[np.ndarray, np.ndarray]:
    rng = np.random.default_rng(seed)
    X = rng.normal(size=(n, d)).astype(np.float32)
    y = (rng.random(n) < fraud_rate).astype(np.int64)
    y[:3] = 1  # guarantee positives
    X[y == 1] += 2.5
    return X, y


def smoke_test(cls: type[AlgorithmPlugin]) -> None:
    """fit → predict/score → save → load → identical outputs. Raises on any failure."""
    params = {**getattr(cls, "smoke_params", {})}
    plugin = cls(params)
    X, y = synthetic_dataset()
    names = [f"f{i}" for i in range(X.shape[1])]
    split = int(len(X) * 0.8)
    with tempfile.TemporaryDirectory() as tmp:
        directory = Path(tmp) / "artifact"
        directory.mkdir()
        if isinstance(plugin, SupervisedPlugin):
            plugin.fit(X[:split], y[:split], X[split:], y[split:], names)
            out = np.asarray(plugin.predict_proba(X))
            _check_shape_range(out, len(X), "predict_proba")
            plugin.explain(X[:2], names, top_k=3)
            plugin.save(directory)
            reloaded = cls.load(directory)
            assert isinstance(reloaded, SupervisedPlugin)
            _check_same(out, np.asarray(reloaded.predict_proba(X)))
        elif isinstance(plugin, AnomalyPlugin):
            plugin.fit(X, names)
            out = np.asarray(plugin.score(X))
            _check_shape_range(out, len(X), "score")
            plugin.save(directory)
            reloaded_a = cls.load(directory)
            assert isinstance(reloaded_a, AnomalyPlugin)
            _check_same(out, np.asarray(reloaded_a.score(X)))
        elif isinstance(plugin, ClusteringPlugin):
            plugin.fit(X, names)
            labels = np.asarray(plugin.labels())
            if labels.shape != (len(X),):
                raise ValueError(f"labels() must return shape ({len(X)},), got {labels.shape}")
            plugin.save(directory)
            reloaded_c = cls.load(directory)
            assert isinstance(reloaded_c, ClusteringPlugin)
            if getattr(cls, "supports_predict", False):
                _check_same(
                    np.asarray(plugin.predict(X), dtype=float), np.asarray(reloaded_c.predict(X), dtype=float)
                )
        else:
            raise TypeError("unknown plugin kind")


def _check_shape_range(out: np.ndarray, n: int, what: str) -> None:
    if out.shape != (n,):
        raise ValueError(f"{what} must return shape ({n},), got {out.shape}")
    if not np.all(np.isfinite(out)) or out.min() < 0 or out.max() > 1:
        raise ValueError(f"{what} must return finite values in [0, 1]")


def _check_same(a: np.ndarray, b: np.ndarray) -> None:
    if not np.allclose(a, b, atol=1e-5):
        raise ValueError("outputs differ after save/load round trip")
