"""File readers → iterators of JSON-safe record dicts.

Supported: CSV/TSV (delimiter sniffing + encoding detection), JSON array, JSON Lines, Parquet, Excel (.xlsx).
Tabular formats keep cell values as strings (preserves leading zeros in phones/account numbers); dotted
headers (`user.id`) are un-flattened into nested objects so mapping paths behave like JSON sources.
"""

from __future__ import annotations

import csv
import json
import math
from collections.abc import Iterator
from datetime import date, datetime
from decimal import Decimal
from pathlib import Path
from typing import Any

import pandas as pd
import pyarrow.parquet as pq
from charset_normalizer import from_bytes

from app.errors import ProblemError

FORMATS = ("csv", "tsv", "json", "jsonl", "parquet", "xlsx")


def detect_format(file_name: str, head: bytes) -> str:
    name = file_name.lower()
    for ext, fmt in (
        (".tsv", "tsv"),
        (".csv", "csv"),
        (".jsonl", "jsonl"),
        (".ndjson", "jsonl"),
        (".json", "json"),
        (".parquet", "parquet"),
        (".pq", "parquet"),
        (".xlsx", "xlsx"),
    ):
        if name.endswith(ext):
            if fmt == "json" and _looks_jsonl(head):
                return "jsonl"
            return fmt
    if head.startswith(b"PAR1"):
        return "parquet"
    if head.startswith(b"PK"):
        return "xlsx"
    stripped = head.lstrip()
    if stripped.startswith(b"["):
        return "json"
    if stripped.startswith(b"{"):
        return "jsonl"
    return "csv"


def _looks_jsonl(head: bytes) -> bool:
    lines = [ln for ln in head.splitlines() if ln.strip()][:2]
    return len(lines) >= 2 and all(ln.strip().startswith(b"{") and ln.strip().endswith(b"}") for ln in lines)


def detect_encoding(path: Path) -> str:
    with path.open("rb") as fh:
        raw = fh.read(256 * 1024)
    if raw.startswith(b"\xef\xbb\xbf"):
        return "utf-8-sig"
    best = from_bytes(raw).best()
    enc = best.encoding if best else "utf-8"
    return "utf-8" if enc in ("ascii",) else enc


def sniff_delimiter(sample: str, default: str = ",") -> str:
    try:
        return csv.Sniffer().sniff(sample, delimiters=",;\t|").delimiter
    except csv.Error:
        counts = {d: sample.count(d) for d in (",", ";", "\t", "|")}
        best = max(counts, key=lambda k: counts[k])
        return best if counts[best] else default


def jsonable(v: Any) -> Any:
    if v is None:
        return None
    if isinstance(v, float):
        return None if math.isnan(v) or math.isinf(v) else v
    if isinstance(v, pd.Timestamp):
        return None if pd.isna(v) else v.isoformat()
    if isinstance(v, datetime | date):
        return v.isoformat()
    if isinstance(v, Decimal):
        return float(v)
    if isinstance(v, bytes):
        return v.decode("utf-8", "replace")
    if isinstance(v, dict):
        return {str(k): jsonable(x) for k, x in v.items()}
    if isinstance(v, list | tuple):
        return [jsonable(x) for x in v]
    if hasattr(v, "tolist"):  # numpy array / scalar
        return jsonable(v.tolist())
    if v is pd.NaT:
        return None
    return v


def unflatten(row: dict[str, Any]) -> dict[str, Any]:
    """`{"user.id": 1, "user": None}` → `{"user": {"id": 1}}`. A null/empty parent column (written by
    exporters for rows where the whole object was null) never shadows real nested values."""
    out: dict[str, Any] = {}
    for key in sorted(row, key=lambda k: k.count(".")):  # parents first
        val = row[key]
        if "." not in key:
            if not (val is None and isinstance(out.get(key), dict)):
                out[key] = val
            continue
        cur = out
        parts = key.split(".")
        ok = True
        for p in parts[:-1]:
            nxt = cur.get(p)
            if nxt is None or nxt == "":
                nxt = cur[p] = {}
            if not isinstance(nxt, dict):
                ok = False
                break
            cur = nxt
        if ok:
            cur[parts[-1]] = val
        else:
            out[key] = val
    return out


def _clean_tabular_row(row: dict[str, Any]) -> dict[str, Any]:
    cleaned = {
        str(k).strip(): (None if (isinstance(v, str) and v == "") else jsonable(v))
        for k, v in row.items()
        if k is not None and str(k).strip() != ""
    }
    return unflatten(cleaned)


def iter_records(path: Path, fmt: str, chunk_size: int = 5000) -> Iterator[list[dict[str, Any]]]:
    """Yield lists of records (chunks). Never loads more than one chunk for streaming formats."""
    try:
        if fmt in ("csv", "tsv"):
            enc = detect_encoding(path)
            with path.open("r", encoding=enc, errors="replace") as fh:
                sample = fh.read(64 * 1024)
            delim = "\t" if fmt == "tsv" else sniff_delimiter(sample)
            reader = pd.read_csv(
                path,
                sep=delim,
                dtype=str,
                keep_default_na=False,
                encoding=enc,
                encoding_errors="replace",
                chunksize=chunk_size,
                engine="python",
            )
            for df in reader:
                yield [
                    _clean_tabular_row({str(k): v for k, v in r.items()})
                    for r in df.to_dict(orient="records")
                ]
        elif fmt == "jsonl":
            enc = detect_encoding(path)
            buf: list[dict[str, Any]] = []
            with path.open("r", encoding=enc, errors="replace") as fh:
                for n, line in enumerate(fh, 1):
                    line = line.strip()
                    if not line:
                        continue
                    try:
                        obj = json.loads(line)
                    except json.JSONDecodeError as e:
                        raise ProblemError(
                            422, "Unreadable file", f"invalid JSON on line {n}: {e.msg}"
                        ) from e
                    buf.append(obj if isinstance(obj, dict) else {"value": obj})
                    if len(buf) >= chunk_size:
                        yield buf
                        buf = []
            if buf:
                yield buf
        elif fmt == "json":
            enc = detect_encoding(path)
            data = json.loads(path.read_text(encoding=enc, errors="replace"))
            if isinstance(data, dict):
                # {"data": [...]} / {"records": [...]} envelopes
                arrays = [v for v in data.values() if isinstance(v, list)]
                data = arrays[0] if len(arrays) == 1 else [data]
            if not isinstance(data, list):
                raise ProblemError(422, "Unreadable file", "JSON must be an array of objects")
            for i in range(0, len(data), chunk_size):
                yield [r if isinstance(r, dict) else {"value": r} for r in data[i : i + chunk_size]]
        elif fmt == "parquet":
            pf = pq.ParquetFile(path)
            for batch in pf.iter_batches(batch_size=chunk_size):
                yield [unflatten({k: jsonable(v) for k, v in r.items()}) for r in batch.to_pylist()]
        elif fmt == "xlsx":
            df = pd.read_excel(path, dtype=str, keep_default_na=False, engine="openpyxl")
            rows = [{str(k): v for k, v in r.items()} for r in df.to_dict(orient="records")]
            for i in range(0, len(rows), chunk_size):
                yield [_clean_tabular_row(r) for r in rows[i : i + chunk_size]]
        else:
            raise ProblemError(415, "Unsupported format", f"format '{fmt}' is not supported")
    except ProblemError:
        raise
    except (ValueError, OSError, UnicodeDecodeError, json.JSONDecodeError) as e:
        raise ProblemError(422, "Unreadable file", f"could not read {fmt}: {e}") from e


def read_sample(path: Path, fmt: str, limit: int) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    for chunk in iter_records(path, fmt, chunk_size=min(limit, 5000)):
        out.extend(chunk)
        if len(out) >= limit:
            break
    return out[:limit]


def estimate_rows(path: Path, fmt: str) -> int | None:
    try:
        if fmt == "parquet":
            return int(pq.ParquetFile(path).metadata.num_rows)
        if fmt in ("csv", "tsv", "jsonl"):
            with path.open("rb") as fh:
                lines = sum(1 for _ in fh)
            return max(0, lines - (1 if fmt in ("csv", "tsv") else 0))
    except OSError:
        return None
    return None
