"""Validation and resolution of a project's `ml_config` against the plugin registry."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from ml_service.plugins.base import resolve_params
from ml_service.plugins.registry import PluginRegistry

DEFAULT_ML_CONFIG: dict[str, Any] = {
    "supervised": {"algorithm": "mlp_backprop", "params": {}},
    "unsupervised": {
        "anomaly_algorithm": "isolation_forest",
        "anomaly_params": {},
        "clustering_algorithm": "hdbscan",
        "clustering_params": {},
    },
    "features": {"include": ["*"], "exclude": [], "extra_source_fields": []},
}


@dataclass(frozen=True)
class ResolvedAlgorithm:
    name: str
    params: dict[str, Any]


def _check_algorithm(
    registry: PluginRegistry, name: Any, params: Any, kind: str, path: str, errors: list[dict[str, str]]
) -> ResolvedAlgorithm | None:
    if not isinstance(name, str) or not name:
        errors.append({"path": path, "message": "algorithm name is required"})
        return None
    entry = registry.get(name)
    if entry is None:
        errors.append({"path": path, "message": f"unknown algorithm '{name}'"})
        return None
    if entry.kind != kind:
        errors.append({"path": path, "message": f"'{name}' is a {entry.kind} algorithm, expected {kind}"})
        return None
    if entry.status != "available":
        errors.append({"path": path, "message": f"algorithm '{name}' is {entry.status}"})
        return None
    if params is not None and not isinstance(params, dict):
        errors.append({"path": f"{path}_params", "message": "params must be an object"})
        return None
    try:
        resolved = resolve_params(entry.param_schema, params or {})
    except ValueError as exc:
        errors.append({"path": f"{path}.params", "message": str(exc)})
        return None
    return ResolvedAlgorithm(name, resolved)


def validate_ml_config(registry: PluginRegistry, config: Any) -> list[dict[str, str]]:
    errors: list[dict[str, str]] = []
    if not isinstance(config, dict):
        return [{"path": "ml_config", "message": "must be an object"}]
    sup = config.get("supervised", DEFAULT_ML_CONFIG["supervised"])
    if not isinstance(sup, dict):
        errors.append({"path": "supervised", "message": "must be an object"})
    else:
        _check_algorithm(
            registry, sup.get("algorithm"), sup.get("params"), "supervised", "supervised.algorithm", errors
        )
    uns = config.get("unsupervised", DEFAULT_ML_CONFIG["unsupervised"])
    if not isinstance(uns, dict):
        errors.append({"path": "unsupervised", "message": "must be an object"})
    else:
        _check_algorithm(
            registry,
            uns.get("anomaly_algorithm"),
            uns.get("anomaly_params"),
            "anomaly",
            "unsupervised.anomaly_algorithm",
            errors,
        )
        _check_algorithm(
            registry,
            uns.get("clustering_algorithm"),
            uns.get("clustering_params"),
            "clustering",
            "unsupervised.clustering_algorithm",
            errors,
        )
    feats = config.get("features", DEFAULT_ML_CONFIG["features"])
    if not isinstance(feats, dict):
        errors.append({"path": "features", "message": "must be an object"})
    else:
        for key in ("include", "exclude", "extra_source_fields"):
            value = feats.get(key, [])
            if not isinstance(value, list) or not all(isinstance(v, str) for v in value):
                errors.append({"path": f"features.{key}", "message": "must be a list of strings"})
        for i, p in enumerate(feats.get("extra_source_fields", []) or []):
            if isinstance(p, str) and not p.startswith("source."):
                errors.append(
                    {"path": f"features.extra_source_fields[{i}]", "message": "must start with 'source.'"}
                )
    return errors


def resolve_algorithm(
    registry: PluginRegistry, name: str, params: dict[str, Any] | None, kind: str, path: str
) -> tuple[ResolvedAlgorithm | None, list[dict[str, str]]]:
    errors: list[dict[str, str]] = []
    return _check_algorithm(registry, name, params, kind, path, errors), errors
