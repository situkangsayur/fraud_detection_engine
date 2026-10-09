from __future__ import annotations

import shutil
from pathlib import Path

from ml_service.plugins.registry import PluginRegistry
from tests.conftest import REPO_ROOT

INVALID_ABSTRACT = """
from ml_service.plugins.base import AnomalyPlugin
class Broken(AnomalyPlugin):
    name = "broken_plugin"
    version = "0.1"
    display_name = "Broken"
    description = "missing score()"
    param_schema = {"type": "object", "properties": {}}
    def fit(self, X, feature_names, progress=None): return {}
    def save(self, directory): pass
    @classmethod
    def load(cls, directory): return cls()
PLUGINS = [Broken]
"""

BAD_SCHEMA = """
from ml_service.plugins.base import AnomalyPlugin
import numpy as np
class BadSchema(AnomalyPlugin):
    name = "bad_schema_plugin"
    version = "0.1"
    display_name = "Bad schema"
    description = "defaults violate schema"
    param_schema = {"type": "object", "properties": {"k": {"type": "integer", "default": "x"}}}
    def fit(self, X, feature_names, progress=None): return {}
    def score(self, X): return np.zeros(len(X))
    def save(self, directory): pass
    @classmethod
    def load(cls, directory): return cls()
PLUGINS = [BadSchema]
"""

WRONG_RANGE = """
from ml_service.plugins.base import AnomalyPlugin
import numpy as np
class OutOfRange(AnomalyPlugin):
    name = "out_of_range_plugin"
    version = "0.1"
    display_name = "Out of range"
    description = "scores outside [0,1]"
    param_schema = {"type": "object", "properties": {}}
    def fit(self, X, feature_names, progress=None): return {}
    def score(self, X): return np.full(len(X), 5.0)
    def save(self, directory): pass
    @classmethod
    def load(cls, directory): return cls()
PLUGINS = [OutOfRange]
"""


def _registry(plugin_dir: Path) -> PluginRegistry:
    reg = PluginRegistry(plugin_dir)
    reg.load_all()
    return reg


def test_builtins_available_without_plugin_dir(tmp_path: Path) -> None:
    reg = _registry(tmp_path / "missing")
    assert sum(e.status == "available" for e in reg.entries()) == 11
    assert all(e.source == "builtin" for e in reg.entries())


def test_external_plugins_valid_and_invalid(tmp_path: Path) -> None:
    shutil.copy(REPO_ROOT / "plugins" / "example_knn_anomaly.py", tmp_path / "knn.py")
    (tmp_path / "broken.py").write_text(INVALID_ABSTRACT)
    (tmp_path / "bad_schema.py").write_text(BAD_SCHEMA)
    (tmp_path / "range.py").write_text(WRONG_RANGE)
    (tmp_path / "no_plugins.py").write_text("X = 1\n")
    (tmp_path / "syntax.py").write_text("def (:\n")
    (tmp_path / "clash.py").write_text(
        "from ml_service.plugins.builtin.anomaly import IsolationForestPlugin\n"
        "class Dup(IsolationForestPlugin):\n    pass\nPLUGINS = [Dup]\n"
    )
    (tmp_path / "_private.py").write_text("raise RuntimeError('must be skipped')\n")
    reg = _registry(tmp_path)

    knn = reg.get("knn_distance_anomaly")
    assert knn is not None and knn.status == "available" and knn.source == "plugin"
    assert reg.plugin_class("knn_distance_anomaly", "anomaly").name == "knn_distance_anomaly"

    for name, fragment in [
        ("broken_plugin", "abstract"),
        ("bad_schema_plugin", "x"),
        ("out_of_range_plugin", "[0, 1]"),
    ]:
        entry = reg.get(name)
        assert entry is not None and entry.status == "invalid", name
        assert fragment in (entry.error or ""), entry.error

    module_errors = {m.module: m.error for m in reg.invalid_modules()}
    assert "PLUGINS" in module_errors["no_plugins.py"]
    assert "SyntaxError" in module_errors["syntax.py"]
    assert "clashes" in module_errors["clash.py"]
    assert "_private.py" not in module_errors
    assert reg.get("isolation_forest").source == "builtin"  # type: ignore[union-attr]


def test_reload_picks_up_new_and_removed_plugins(tmp_path: Path) -> None:
    reg = _registry(tmp_path)
    assert reg.get("knn_distance_anomaly") is None
    shutil.copy(REPO_ROOT / "plugins" / "example_knn_anomaly.py", tmp_path / "knn.py")
    reg.load_all()
    assert reg.get("knn_distance_anomaly") is not None
    (tmp_path / "knn.py").unlink()
    reg.load_all()
    assert reg.get("knn_distance_anomaly") is None


def test_plugin_class_rejects_wrong_kind(tmp_path: Path) -> None:
    import pytest

    reg = _registry(tmp_path)
    with pytest.raises(ValueError, match="expected supervised"):
        reg.plugin_class("kmeans", "supervised")
    with pytest.raises(KeyError):
        reg.plugin_class("nope", "supervised")
