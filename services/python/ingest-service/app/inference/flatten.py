"""Flatten nested records into dotted paths.

`{"user": {"id": 1, "tags": ["a"]}, "items": [{"sku": "x"}]}` →
`{"user.id": 1, "user.tags[0]": "a", "items[0].sku": "x"}` for inspection. Array element 0 is sampled
(`[0]`), and the array itself is also reported under `path[]` so users can map whole arrays.
"""

from __future__ import annotations

from typing import Any


def flatten(
    record: Any, prefix: str = "", out: dict[str, Any] | None = None, max_depth: int = 8
) -> dict[str, Any]:
    out = {} if out is None else out
    if max_depth < 0:
        out[prefix] = record
        return out
    if isinstance(record, dict):
        if not record and prefix:
            out[prefix] = record
        for k, v in record.items():
            key = f"{prefix}.{k}" if prefix else str(k)
            flatten(v, key, out, max_depth - 1)
    elif isinstance(record, list):
        out[f"{prefix}[]"] = record
        if record:
            flatten(record[0], f"{prefix}[0]", out, max_depth - 1)
    else:
        out[prefix] = record
    return out


def get_path(record: Any, path: str) -> Any:
    """Resolve a dotted path with `[n]` indexes (same syntax as mapping `from`)."""
    cur = record
    for part in _split(path):
        if cur is None:
            return None
        if isinstance(part, int):
            if not isinstance(cur, list) or part >= len(cur):
                return None
            cur = cur[part]
        else:
            if not isinstance(cur, dict):
                return None
            cur = cur.get(part)
    return cur


def _split(path: str) -> list[str | int]:
    parts: list[str | int] = []
    for seg in path.split("."):
        while "[" in seg:
            name, rest = seg.split("[", 1)
            if name:
                parts.append(name)
            idx, seg = rest.split("]", 1)
            if idx:
                parts.append(int(idx))
        if seg:
            parts.append(seg)
    return parts
