from __future__ import annotations

from pathlib import Path

from ml_service.ml_config import DEFAULT_ML_CONFIG, validate_ml_config
from ml_service.plugins.registry import PluginRegistry


def _reg(tmp_path: Path) -> PluginRegistry:
    reg = PluginRegistry(tmp_path)
    reg.load_all()
    return reg


def test_default_config_is_valid(tmp_path: Path) -> None:
    assert validate_ml_config(_reg(tmp_path), DEFAULT_ML_CONFIG) == []


def test_invalid_config_reports_every_problem(tmp_path: Path) -> None:
    errors = validate_ml_config(
        _reg(tmp_path),
        {
            "supervised": {"algorithm": "kmeans", "params": {}},
            "unsupervised": {
                "anomaly_algorithm": "nope",
                "clustering_algorithm": "hdbscan",
                "clustering_params": {"min_cluster_size": 1},
            },
            "features": {"include": "all", "extra_source_fields": ["order.total"]},
        },
    )
    paths = {e["path"] for e in errors}
    assert "supervised.algorithm" in paths
    assert "unsupervised.anomaly_algorithm" in paths
    assert "unsupervised.clustering_algorithm.params" in paths
    assert "features.include" in paths
    assert "features.extra_source_fields[0]" in paths


def test_disabled_algorithm_rejected(tmp_path: Path) -> None:
    reg = _reg(tmp_path)
    reg.mark_disabled({"mlp_backprop"})
    errors = validate_ml_config(reg, {"supervised": {"algorithm": "mlp_backprop"}})
    assert errors and "disabled" in errors[0]["message"]
