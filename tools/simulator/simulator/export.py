"""Write a dataset file in the custom shape — demonstrates the file-import path (inspect → mapping → job).

CSV uses dotted headers (`pengguna.id`), which the ingest-service un-flattens back into nested objects.
Ground truth is included as `is_fraud` / `jenis_fraud` so the mapping's optional `label` section can be used.
"""

from __future__ import annotations

import csv
import json
from pathlib import Path
from typing import Any

from simulator.scenarios import Dataset


def _flatten(rec: Any, prefix: str = "") -> dict[str, Any]:
    out: dict[str, Any] = {}
    if isinstance(rec, dict):
        for k, v in rec.items():
            out.update(_flatten(v, f"{prefix}.{k}" if prefix else k))
    elif prefix:
        out[prefix] = rec
    return out


def export(ds: Dataset, path: Path, fmt: str = "csv", delimiter: str = ",") -> int:
    rows = []
    for e in ds.events:
        rec = dict(e.record)
        rec["is_fraud"] = 1 if e.is_fraud else 0
        rec["jenis_fraud"] = e.fraud_type or ""
        rows.append(rec)
    path.parent.mkdir(parents=True, exist_ok=True)
    if fmt == "jsonl":
        with path.open("w", encoding="utf-8") as fh:
            for r in rows:
                fh.write(json.dumps(r, ensure_ascii=False) + "\n")
        return len(rows)
    flat = [_flatten(r) for r in rows]
    headers: list[str] = []
    seen: set[str] = set()
    for f in flat:
        for k in f:
            if k not in seen:
                seen.add(k)
                headers.append(k)
    with path.open("w", encoding="utf-8", newline="") as fh:
        w = csv.DictWriter(fh, fieldnames=headers, delimiter=delimiter)
        w.writeheader()
        for f in flat:
            w.writerow({k: ("" if v is None else v) for k, v in f.items()})
    return len(rows)
