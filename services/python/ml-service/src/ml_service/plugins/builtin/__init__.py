"""Built-in algorithm plugins."""

from ml_service.plugins.builtin.anomaly import PLUGINS as ANOMALY_PLUGINS
from ml_service.plugins.builtin.clustering import PLUGINS as CLUSTERING_PLUGINS
from ml_service.plugins.builtin.supervised import PLUGINS as SUPERVISED_PLUGINS

BUILTIN_PLUGINS = [*SUPERVISED_PLUGINS, *ANOMALY_PLUGINS, *CLUSTERING_PLUGINS]

__all__ = ["BUILTIN_PLUGINS"]
