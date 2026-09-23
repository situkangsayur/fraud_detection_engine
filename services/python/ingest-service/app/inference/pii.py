"""PII detection heuristics and masking."""

from __future__ import annotations

import re
from typing import Any

EMAIL_RE = re.compile(r"^[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}$")
# Indonesian mobile (08xx / 628xx / +628xx) or generic E.164
PHONE_RE = re.compile(r"^(\+?62|0)8[1-9][0-9]{6,11}$|^\+[1-9][0-9]{7,14}$")
DIGITS_RE = re.compile(r"\D")
IPV4_RE = re.compile(r"^(?:(?:25[0-5]|2[0-4]\d|1?\d?\d)\.){3}(?:25[0-5]|2[0-4]\d|1?\d?\d)$")
ISO2_RE = re.compile(r"^[A-Z]{2}$")
NAME_HINTS = ("name", "nama", "fullname", "full_name", "holder", "pemilik")
ACCOUNT_HINTS = ("rekening", "account", "acct", "norek", "no_rek", "beneficiary", "iban", "va_number")


def luhn_ok(digits: str) -> bool:
    total, alt = 0, False
    for ch in reversed(digits):
        d = ord(ch) - 48
        if alt:
            d *= 2
            if d > 9:
                d -= 9
        total += d
        alt = not alt
    return total % 10 == 0


def is_pan(value: Any) -> bool:
    if value is None or isinstance(value, bool):
        return False
    s = str(value).strip()
    if not re.fullmatch(r"[0-9 \-]{13,23}", s):
        return False
    digits = DIGITS_RE.sub("", s)
    return 13 <= len(digits) <= 19 and luhn_ok(digits)


def is_email(value: Any) -> bool:
    return isinstance(value, str) and bool(EMAIL_RE.match(value.strip()))


def is_phone(value: Any) -> bool:
    if value is None or isinstance(value, bool):
        return False
    s = re.sub(r"[\s\-().]", "", str(value))
    return bool(PHONE_RE.match(s))


def is_ipv4(value: Any) -> bool:
    return isinstance(value, str) and bool(IPV4_RE.match(value.strip()))


def detect_pii(path: str, samples: list[Any]) -> str | None:
    """Return a PII kind (pan|email|phone|account_number|name) or None, by majority of non-null samples."""
    vals = [v for v in samples if v not in (None, "")]
    leaf = path.lower().rsplit(".", 1)[-1]
    if vals:
        n = len(vals)
        for kind, fn in (("pan", is_pan), ("email", is_email), ("phone", is_phone)):
            if sum(1 for v in vals if fn(v)) / n >= 0.8:
                return kind
        if any(h in leaf for h in ACCOUNT_HINTS):
            digits_share = sum(1 for v in vals if re.fullmatch(r"[0-9\- ]{6,20}", str(v))) / n
            if digits_share >= 0.8:
                return "account_number"
    if any(h in leaf for h in NAME_HINTS) and vals and all(isinstance(v, str) for v in vals):
        return "name"
    return None


def mask(value: Any, kind: str | None) -> Any:
    if value is None or kind is None:
        return value
    s = str(value)
    if kind == "pan":
        d = DIGITS_RE.sub("", s)
        return f"{d[:6]}{'*' * max(0, len(d) - 10)}{d[-4:]}"
    if kind == "account_number":
        d = DIGITS_RE.sub("", s)
        return f"{'*' * max(0, len(d) - 4)}{d[-4:]}"
    if kind == "email":
        local, _, domain = s.partition("@")
        return f"{local[:1]}***@{domain}"
    if kind == "phone":
        return f"{s[:4]}****{s[-3:]}" if len(s) > 7 else "****"
    if kind == "name":
        return " ".join(p[:1] + "***" for p in s.split())
    return value
