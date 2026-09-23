"""Value type inference incl. datetime format guessing (Indonesian + ISO + unix)."""

from __future__ import annotations

import math
import re
from collections import Counter
from datetime import UTC, datetime
from typing import Any

# (strftime format, label) — tried in order; day-first formats come first (Indonesian convention).
DATETIME_FORMATS: list[str] = [
    "%Y-%m-%dT%H:%M:%S.%f%z",
    "%Y-%m-%dT%H:%M:%S%z",
    "%Y-%m-%dT%H:%M:%S.%f",
    "%Y-%m-%dT%H:%M:%S",
    "%Y-%m-%d %H:%M:%S.%f",
    "%Y-%m-%d %H:%M:%S",
    "%Y-%m-%d %H:%M",
    "%Y-%m-%d",
    "%d/%m/%Y %H:%M:%S",
    "%d/%m/%Y %H:%M",
    "%d/%m/%Y",
    "%d-%m-%Y %H:%M:%S",
    "%d-%m-%Y %H:%M",
    "%d-%m-%Y",
    "%d.%m.%Y %H:%M:%S",
    "%d.%m.%Y",
    "%m/%d/%Y %H:%M:%S",
    "%m/%d/%Y",
    "%Y/%m/%d %H:%M:%S",
    "%Y/%m/%d",
    "%d %b %Y %H:%M",
    "%d %b %Y",
    "%d %B %Y",
]
RFC3339 = "rfc3339"
UNIX_S = "unix_s"
UNIX_MS = "unix_ms"
# plausible unix range: 2000-01-01 .. 2100-01-01
_UNIX_MIN, _UNIX_MAX = 946_684_800, 4_102_444_800
_TRUE = {"true", "yes", "y", "t", "ya"}
_FALSE = {"false", "no", "n", "f", "tidak"}


def _is_int_str(s: str) -> bool:
    return s.lstrip("-").isdigit()


def parse_number(value: Any) -> float | None:
    """Parse numbers incl. Indonesian locale ("1.500.000,50") and English ("1,500,000.50")."""
    if isinstance(value, bool) or value is None:
        return None
    if isinstance(value, int | float):
        return None if isinstance(value, float) and math.isnan(value) else float(value)
    s = str(value).strip().replace("Rp", "").replace("IDR", "").replace(" ", "")
    if not s:
        return None
    if "," in s and "." in s:
        s = s.replace(".", "").replace(",", ".") if s.rfind(",") > s.rfind(".") else s.replace(",", "")
    elif "," in s:
        head, _, tail = s.rpartition(",")
        s = s.replace(",", "") if len(tail) == 3 and head else s.replace(",", ".")
    elif s.count(".") > 1 or re.fullmatch(r"[1-9]\d{0,2}(\.\d{3})+", s):
        s = s.replace(".", "")  # Indonesian thousands separators: 12.000 → 12000
    try:
        return float(s)
    except ValueError:
        return None


def guess_datetime_format(samples: list[str]) -> str | None:
    vals = [s.strip() for s in samples if isinstance(s, str) and s.strip()]
    if not vals:
        return None
    for fmt in DATETIME_FORMATS:
        ok = 0
        for v in vals:
            try:
                datetime.strptime(v, fmt)
                ok += 1
            except ValueError:
                pass
        if ok / len(vals) >= 0.9:
            return RFC3339 if fmt.startswith("%Y-%m-%dT") else fmt
    return None


def unix_kind(nums: list[float]) -> str | None:
    if not nums or any(not float(n).is_integer() for n in nums):
        return None
    if all(_UNIX_MIN <= n <= _UNIX_MAX for n in nums):
        return UNIX_S
    if all(_UNIX_MIN * 1000 <= n <= _UNIX_MAX * 1000 for n in nums):
        return UNIX_MS
    return None


def infer_type(path: str, values: list[Any]) -> tuple[str, str | None]:
    """Return (inferred_type, datetime_format) for a column's sample values."""
    vals = [v for v in values if v is not None and not (isinstance(v, float) and math.isnan(v)) and v != ""]
    if not vals:
        return "string", None
    if all(isinstance(v, dict) for v in vals):
        return "object", None
    if all(isinstance(v, list) for v in vals):
        return "array", None
    if all(isinstance(v, datetime) for v in vals):
        return "datetime", RFC3339
    if all(isinstance(v, bool) for v in vals):
        return "bool", None
    strs = [str(v).strip() for v in vals]
    if all(s.lower() in _TRUE | _FALSE for s in strs) and len({s.lower() for s in strs}) <= 2:
        return "bool", None
    # phone numbers / account numbers / ids made of digits with leading zeros or '+' stay strings
    digit_like = [v.strip() for v in vals if isinstance(v, str) and re.fullmatch(r"\+?[\d \-]+", v.strip())]
    if digit_like and any(d.startswith("+") or (d.startswith("0") and len(d) > 1) for d in digit_like):
        return "string", None
    leaf = path.lower()
    time_hint = any(h in leaf for h in ("time", "date", "tgl", "tanggal", "waktu", "_at", "timestamp", "ts"))
    if all(isinstance(v, int | float) and not isinstance(v, bool) for v in vals) or all(
        _is_int_str(s) for s in strs
    ):
        nums = (
            [float(v) for v in vals]
            if all(isinstance(v, int | float) for v in vals)
            else [float(s) for s in strs]
        )
        if time_hint:
            uk = unix_kind(nums)
            if uk:
                return "datetime", uk
        # long digit strings with leading zeros (phones, account numbers) stay strings
        if any(isinstance(v, str) and (v.strip().startswith("0") and len(v.strip()) > 1) for v in vals):
            return "string", None
        if any(isinstance(v, str) and len(v.strip()) > 15 for v in vals):
            return "string", None
        return ("integer" if all(n.is_integer() for n in nums) else "number"), None
    fmt = guess_datetime_format(strs)
    if fmt:
        return "datetime", fmt
    parsed = [parse_number(s) for s in strs]
    if all(p is not None for p in parsed) and any(ch.isdigit() for ch in strs[0]):
        return ("integer" if all(p is not None and p.is_integer() for p in parsed) else "number"), None
    return "string", None


def to_utc_iso(value: Any, fmt: str | None) -> str | None:
    """Best-effort conversion used for sorting by occurred_at before sending batches."""
    if value is None or value == "":
        return None
    try:
        if isinstance(value, datetime):
            dt = value
        elif fmt == UNIX_S:
            dt = datetime.fromtimestamp(float(value), UTC)
        elif fmt == UNIX_MS:
            dt = datetime.fromtimestamp(float(value) / 1000, UTC)
        elif fmt in (None, RFC3339):
            dt = datetime.fromisoformat(str(value).replace("Z", "+00:00"))
        else:
            dt = datetime.strptime(str(value), fmt)
        if dt.tzinfo is None:
            dt = dt.replace(tzinfo=UTC)
        return dt.astimezone(UTC).isoformat()
    except (ValueError, OverflowError, OSError):
        return None


def distinct_ratio(values: list[Any]) -> float:
    vals = [repr(v) for v in values if v is not None]
    return round(len(set(vals)) / len(vals), 4) if vals else 0.0


def most_common(values: list[Any], n: int = 5) -> list[Any]:
    c = Counter(repr(v) for v in values if v is not None)
    keep = {k for k, _ in c.most_common(n)}
    out: list[Any] = []
    seen: set[str] = set()
    for v in values:
        r = repr(v)
        if r in keep and r not in seen:
            out.append(v)
            seen.add(r)
    return out
