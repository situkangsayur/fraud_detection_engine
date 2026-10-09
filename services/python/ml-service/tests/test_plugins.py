from __future__ import annotations

from pathlib import Path

import numpy as np
import pytest

from ml_service.plugins.base import EmpiricalCdf, SupervisedPlugin, resolve_params, schema_defaults
from ml_service.plugins.builtin import BUILTIN_PLUGINS
from ml_service.plugins.builtin.supervised import MlpBackpropPlugin
from ml_service.plugins.registry import validate_plugin_class
from ml_service.plugins.smoke import smoke_test, synthetic_dataset


@pytest.mark.parametrize("cls", BUILTIN_PLUGINS, ids=lambda c: c.name)
def test_builtin_plugin_passes_contract_and_smoke_test(cls: type) -> None:
    validate_plugin_class(cls, run_smoke=False)
    smoke_test(cls)


def test_expected_builtin_catalogue() -> None:
    names = {c.name: c.kind for c in BUILTIN_PLUGINS}
    assert names == {
        "mlp_backprop": "supervised",
        "logistic_regression": "supervised",
        "gradient_boosting": "supervised",
        "random_forest": "supervised",
        "isolation_forest": "anomaly",
        "local_outlier_factor": "anomaly",
        "autoencoder": "anomaly",
        "hdbscan": "clustering",
        "kmeans": "clustering",
        "dbscan": "clustering",
        "gaussian_mixture": "clustering",
    }


@pytest.mark.parametrize(
    "cls", [c for c in BUILTIN_PLUGINS if issubclass(c, SupervisedPlugin)], ids=lambda c: c.name
)
def test_supervised_plugins_learn_signal_on_imbalanced_data(cls: type, tmp_path: Path) -> None:
    X, y = synthetic_dataset(n=600, d=8, fraud_rate=0.05, seed=3)
    plugin = cls({**cls.smoke_params, **({"epochs": 30} if cls is MlpBackpropPlugin else {})})
    plugin.fit(X[:480], y[:480], X[480:], y[480:], [f"f{i}" for i in range(8)])
    proba = plugin.predict_proba(X[480:])
    assert proba[y[480:] == 1].mean() > proba[y[480:] == 0].mean() + 0.2
    explained = plugin.explain(X[:1], [f"f{i}" for i in range(8)], top_k=3)
    assert len(explained) == 1 and len(explained[0]) <= 3


def test_mlp_history_records_early_stopping() -> None:
    X, y = synthetic_dataset(n=300, d=4, seed=1)
    plugin = MlpBackpropPlugin({"epochs": 40, "patience": 2, "hidden_layers": [8]})
    history = plugin.fit(X[:240], y[:240], X[240:], y[240:], ["a", "b", "c", "d"])
    assert history["epochs_run"] <= 40
    assert history["pos_weight"] > 1
    assert 0 <= history["best_epoch"] < history["epochs_run"]


def test_mlp_keeps_training_when_pr_auc_saturates() -> None:
    """Separable data: PR-AUC is 1.0 after epoch 1; validation loss must keep driving training/calibration."""
    X, y = synthetic_dataset(n=400, d=4, fraud_rate=0.15, seed=5)
    X[y == 1] += 4.0
    plugin = MlpBackpropPlugin({"epochs": 30, "hidden_layers": [16]})
    history = plugin.fit(X[:320], y[:320], X[320:], y[320:], ["a", "b", "c", "d"])
    assert history["best_epoch"] > 0
    proba = plugin.predict_proba(X[320:])
    assert proba[y[320:] == 1].min() > 0.8 and proba[y[320:] == 0].max() < 0.5


def test_resolve_params_applies_defaults_and_rejects_invalid() -> None:
    schema = MlpBackpropPlugin.param_schema
    assert resolve_params(schema, {})["hidden_layers"] == [64, 32]
    assert schema_defaults(schema)["lr"] == 0.001
    with pytest.raises(ValueError, match="dropout"):
        resolve_params(schema, {"dropout": 5})
    with pytest.raises(ValueError, match="Additional properties"):
        resolve_params(schema, {"unknown": 1})


def test_empirical_cdf_maps_to_unit_interval() -> None:
    cdf = EmpiricalCdf(np.arange(100, dtype=float))
    out = cdf(np.array([-5.0, 49.5, 1000.0]))
    assert out[0] == 0.0 and out[2] == 1.0 and 0.49 < out[1] < 0.51
    assert np.allclose(EmpiricalCdf.from_list(cdf.to_list())(np.array([10.0])), cdf(np.array([10.0])))


def test_example_external_plugin_passes_smoke_test() -> None:
    import importlib.util

    from tests.conftest import REPO_ROOT

    path = REPO_ROOT / "plugins" / "example_knn_anomaly.py"
    spec = importlib.util.spec_from_file_location("example_knn_anomaly_test", path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    for cls in module.PLUGINS:
        validate_plugin_class(cls, run_smoke=True)
