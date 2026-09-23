"""Build the inferred schema (list of field descriptors) from sample records."""

from __future__ import annotations

import math
from datetime import date, datetime
from typing import Any

from app.inference.flatten import flatten
from app.inference.pii import detect_pii, mask
from app.inference.types import distinct_ratio, infer_type, most_common


def _jsonable(v: Any) -> Any:
    if isinstance(v, datetime | date):
        return v.isoformat()
    if isinstance(v, float) and (math.isnan(v) or math.isinf(v)):
        return None
    if isinstance(v, dict | list):
        return None  # container samples are not shown (children are)
    if hasattr(v, "item"):  # numpy scalar
        return v.item()
    return v


def infer_schema(records: list[dict[str, Any]]) -> list[dict[str, Any]]:
    flat = [flatten(r) for r in records]
    order: list[str] = []
    seen: set[str] = set()
    for f in flat:
        for k in f:
            if k not in seen:
                seen.add(k)
                order.append(k)
    n = len(flat) or 1
    fields: list[dict[str, Any]] = []
    for path in order:
        values = [f.get(path) for f in flat]
        present = [
            v for v in values if v is not None and v != "" and not (isinstance(v, float) and math.isnan(v))
        ]
        inferred, dt_fmt = infer_type(path, present)
        pii = None if path.endswith("[]") else detect_pii(path, present[:200])
        samples = [mask(_jsonable(v), pii) for v in most_common(present, 5)]
        fields.append(
            {
                "path": path,
                "inferred_type": inferred,
                "datetime_format": dt_fmt,
                "null_ratio": round(1 - len(present) / n, 4),
                "distinct_ratio": distinct_ratio(present),
                "sample_values": [s for s in samples if s is not None],
                "pii": pii,
            }
        )
    return fields
