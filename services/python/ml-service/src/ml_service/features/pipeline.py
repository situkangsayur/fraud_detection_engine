"""FeaturePipeline: dict rows → dense float32 matrix. Fitted on training data and persisted with the model.

* numeric: median imputation (training medians) → standardisation → clip to ±10σ
* categorical: top-N categories (training) + "__other__" bucket, one-hot
* missing keys are imputed; unknown keys are ignored; unknown categories map to "__other__" — but only when
  "__other__" occurred in training; otherwise they encode as all-zeros (an untrained column must not move scores)
* `source.*` extra fields (project ml_config.features.extra_source_fields) are typed at fit time
"""

from __future__ import annotations

import math
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

import joblib
import numpy as np

from ml_service.features.catalog import CATEGORICAL_FEATURES, FEATURE_SET_VERSION, NUMERIC_FEATURES

OTHER = "__other__"
MAX_CATEGORIES = 20
CLIP_Z = 10.0


def _to_float(value: Any) -> float | None:
    if value is None or isinstance(value, dict | list):
        return None
    if isinstance(value, bool):
        return 1.0 if value else 0.0
    try:
        out = float(value)
    except (TypeError, ValueError):
        return None
    return out if math.isfinite(out) else None


def get_path(obj: Any, path: str) -> Any:
    """Resolve a dotted path (`a.b[0].c`) into nested dicts/lists; None when absent."""
    cur = obj
    for part in path.replace("]", "").replace("[", ".").split("."):
        if part == "":
            continue
        if isinstance(cur, dict):
            cur = cur.get(part)
        elif isinstance(cur, list) and part.isdigit() and int(part) < len(cur):
            cur = cur[int(part)]
        else:
            return None
        if cur is None:
            return None
    return cur


def select_features(config: dict[str, Any] | None) -> tuple[list[str], list[str], list[str]]:
    """(numeric, categorical, extra_source_fields) from a project's `ml_config.features`."""
    cfg = config or {}
    include = cfg.get("include") or ["*"]
    exclude = set(cfg.get("exclude") or [])
    wanted = None if "*" in include else set(include)
    numeric = [f for f in NUMERIC_FEATURES if (wanted is None or f in wanted) and f not in exclude]
    categorical = [f for f in CATEGORICAL_FEATURES if (wanted is None or f in wanted) and f not in exclude]
    extras = [
        p for p in (cfg.get("extra_source_fields") or []) if isinstance(p, str) and p.startswith("source.")
    ]
    return numeric, categorical, extras


def build_row(
    features: dict[str, Any] | None, source: dict[str, Any] | None, extras: list[str]
) -> dict[str, Any]:
    """Merge event features with extra `source.*` fields into one flat row."""
    row = dict(features or {})
    for path in extras:
        row[path] = get_path(source or {}, path[len("source.") :])
    return row


@dataclass
class FeaturePipeline:
    numeric: list[str]
    categorical: list[str]
    extras: list[str] = field(default_factory=list)
    feature_set_version: int = FEATURE_SET_VERSION
    medians: dict[str, float] = field(default_factory=dict)
    means: dict[str, float] = field(default_factory=dict)
    stds: dict[str, float] = field(default_factory=dict)
    categories: dict[str, list[str]] = field(default_factory=dict)
    other_seen: dict[str, bool] = field(default_factory=dict)
    numeric_columns: list[str] = field(default_factory=list)  # numeric + numeric extras (fit time)
    categorical_columns: list[str] = field(default_factory=list)
    fitted: bool = False

    @classmethod
    def from_config(cls, config: dict[str, Any] | None) -> FeaturePipeline:
        numeric, categorical, extras = select_features(config)
        return cls(numeric=numeric, categorical=categorical, extras=extras)

    # ----------------------------------------------------------------------------- fit
    def fit(self, rows: list[dict[str, Any]]) -> FeaturePipeline:
        numeric_cols = list(self.numeric)
        categorical_cols = list(self.categorical)
        for path in self.extras:  # type extras from data: ≥90% parse as float → numeric
            values = [r.get(path) for r in rows if r.get(path) is not None]
            parsed = [_to_float(v) for v in values]
            if values and sum(p is not None for p in parsed) >= 0.9 * len(values):
                numeric_cols.append(path)
            else:
                categorical_cols.append(path)
        for col in numeric_cols:
            parsed = [_to_float(r.get(col)) for r in rows]
            present = np.array([v for v in parsed if v is not None], dtype=np.float64)
            median = float(np.median(present)) if present.size else 0.0
            arr = np.array([median if v is None else v for v in parsed], dtype=np.float64)
            std = float(arr.std()) if arr.size else 1.0
            self.medians[col] = median
            self.means[col] = float(arr.mean()) if arr.size else 0.0
            self.stds[col] = std if std > 1e-12 else 1.0
        for col in categorical_cols:
            counts: dict[str, int] = {}
            for r in rows:
                v = r.get(col)
                if v is not None:
                    counts[str(v)] = counts.get(str(v), 0) + 1
            top = sorted(counts, key=lambda k: (-counts[k], k))[:MAX_CATEGORIES]
            self.categories[col] = [*top, OTHER]
            self.other_seen[col] = any(r.get(col) is None or str(r.get(col)) not in top for r in rows)
        self.numeric_columns = numeric_cols
        self.categorical_columns = categorical_cols
        self.fitted = True
        return self

    # ----------------------------------------------------------------------------- transform
    @property
    def feature_names(self) -> list[str]:
        names = list(self.numeric_columns)
        for col in self.categorical_columns:
            names += [f"{col}={c}" for c in self.categories[col]]
        return names

    def transform(self, rows: list[dict[str, Any]]) -> np.ndarray:
        if not self.fitted:
            raise RuntimeError("pipeline not fitted")
        n_num = len(self.numeric_columns)
        width = len(self.feature_names)
        out = np.zeros((len(rows), width), dtype=np.float32)
        for i, row in enumerate(rows):
            for j, col in enumerate(self.numeric_columns):
                v = _to_float(row.get(col))
                x = self.medians[col] if v is None else v
                out[i, j] = max(-CLIP_Z, min(CLIP_Z, (x - self.means[col]) / self.stds[col]))
            offset = n_num
            for col in self.categorical_columns:
                cats = self.categories[col]
                v = row.get(col)
                key = str(v) if v is not None else OTHER
                if key in cats and key != OTHER:
                    out[i, offset + cats.index(key)] = 1.0
                elif self.other_seen.get(col, True):
                    out[i, offset + len(cats) - 1] = 1.0
                offset += len(cats)
        return out

    def fit_transform(self, rows: list[dict[str, Any]]) -> np.ndarray:
        return self.fit(rows).transform(rows)

    def raw_numeric(self, rows: list[dict[str, Any]]) -> np.ndarray:
        """Imputed, unscaled numeric matrix (for human-readable cluster profiles)."""
        out = np.zeros((len(rows), len(self.numeric_columns)), dtype=np.float64)
        for i, row in enumerate(rows):
            for j, col in enumerate(self.numeric_columns):
                v = _to_float(row.get(col))
                out[i, j] = self.medians[col] if v is None else v
        return out

    # ----------------------------------------------------------------------------- persistence
    def save(self, path: Path) -> None:
        joblib.dump(self.__dict__, path)

    @classmethod
    def load(cls, path: Path) -> FeaturePipeline:
        state = joblib.load(path)
        obj = cls(numeric=state["numeric"], categorical=state["categorical"])
        obj.__dict__.update(state)
        return obj
