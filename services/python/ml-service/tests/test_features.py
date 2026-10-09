from __future__ import annotations

from pathlib import Path

import numpy as np

from ml_service.features.catalog import CATEGORICAL_FEATURES, NUMERIC_FEATURES
from ml_service.features.pipeline import OTHER, FeaturePipeline, build_row, get_path, select_features


def _rows() -> list[dict]:
    return [
        {"amount": 100, "is_night": 0, "event_type": "transaction", "channel": "web", "unknown_key": "x"},
        {"amount": 300, "is_night": 1, "event_type": "login", "channel": "mobile_app"},
        {"amount": None, "event_type": "transaction"},
    ]


def test_select_features_include_exclude_and_extras() -> None:
    numeric, categorical, extras = select_features(
        {"include": ["amount", "channel"], "exclude": ["channel"], "extra_source_fields": ["source.a", "bad"]}
    )
    assert numeric == ["amount"] and categorical == [] and extras == ["source.a"]
    numeric, categorical, _ = select_features(None)
    assert numeric == list(NUMERIC_FEATURES) and categorical == list(CATEGORICAL_FEATURES)


def test_missing_keys_are_imputed_and_unknown_categories_bucketed() -> None:
    pipe = FeaturePipeline.from_config({"include": ["amount", "is_night", "event_type", "channel"]})
    X = pipe.fit_transform(_rows())
    assert X.shape == (3, len(pipe.feature_names)) and np.isfinite(X).all()
    # median of [100, 300] = 200 → row 3 amount imputed to 200 → z = (200 - mean)/std
    idx = pipe.feature_names.index("amount")
    assert abs(X[2, idx] - (200 - pipe.means["amount"]) / pipe.stds["amount"]) < 1e-5
    out = pipe.transform([{"event_type": "never_seen", "channel": None}])
    # every training row had an event_type in the top categories → "__other__" never trained → all zeros
    ev_cols = [i for i, n in enumerate(pipe.feature_names) if n.startswith("event_type=")]
    assert out[0, ev_cols].sum() == 0.0
    # channel was missing in a training row → "__other__" was trained → used for unknown/missing
    assert out[0, pipe.feature_names.index(f"channel={OTHER}")] == 1.0


def test_extra_source_fields_typed_from_data_and_persisted(tmp_path: Path) -> None:
    pipe = FeaturePipeline.from_config(
        {"include": ["amount"], "extra_source_fields": ["source.order.items", "source.tier"]}
    )
    rows = [
        build_row({"amount": i}, {"order": {"items": i % 3}, "tier": ["gold", "silver"][i % 2]}, pipe.extras)
        for i in range(10)
    ]
    X = pipe.fit_transform(rows)
    assert "source.order.items" in pipe.numeric_columns and "source.tier" in pipe.categorical_columns
    pipe.save(tmp_path / "p.joblib")
    loaded = FeaturePipeline.load(tmp_path / "p.joblib")
    assert loaded.feature_names == pipe.feature_names
    assert np.allclose(loaded.transform(rows), X)


def test_get_path_handles_nesting_and_arrays() -> None:
    doc = {"a": {"b": [{"c": 5}]}}
    assert get_path(doc, "a.b[0].c") == 5
    assert get_path(doc, "a.b[3].c") is None
    assert get_path(doc, "x.y") is None


def test_constant_column_does_not_divide_by_zero() -> None:
    pipe = FeaturePipeline.from_config({"include": ["amount"]})
    X = pipe.fit_transform([{"amount": 7}, {"amount": 7}])
    assert np.all(X == 0)
