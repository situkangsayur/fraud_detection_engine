"""Mapping suggestion: inferred schema → mapping JSON (docs/technical/data-sources.md §3) + confidences.

Scoring per (source field, canonical target):
    score = name_similarity × type_factor × context_factor, overridden upward by strong value patterns
    (a Luhn-valid PAN column is the card; an email-shaped column is the customer email; …).
Assignment is greedy by score (each target once, each source once — except a PAN source, which feeds
instrument_fingerprint + card_bin + card_last4). Only suggestions ≥ MIN_CONFIDENCE are kept.
"""

from __future__ import annotations

import json
import re
from dataclasses import dataclass
from functools import lru_cache
from pathlib import Path
from typing import Any

from rapidfuzz import fuzz

from app.inference.types import RFC3339, UNIX_MS, UNIX_S

MIN_CONFIDENCE = 0.5
SYNONYMS_PATH = Path(__file__).with_name("synonyms.json")
DEFAULT_TZ = "Asia/Jakarta"


@dataclass(frozen=True)
class Candidate:
    source: str
    target: str
    score: float
    reason: str


@lru_cache
def load_dictionary() -> dict[str, Any]:
    data: dict[str, Any] = json.loads(SYNONYMS_PATH.read_text(encoding="utf-8"))
    return data


def canonical_targets() -> list[str]:
    return list(load_dictionary()["targets"].keys())


def normalize_name(name: str) -> str:
    name = re.sub(r"\[\d*\]", "", name)
    name = re.sub(r"([a-z0-9])([A-Z])", r"\1_\2", name)
    name = re.sub(r"[^A-Za-z0-9]+", "_", name).strip("_").lower()
    return name


def _leaf(path: str) -> str:
    return normalize_name(path.rsplit(".", 1)[-1])


def _parents(path: str) -> list[str]:
    return [normalize_name(p) for p in path.split(".")[:-1]]


def name_similarity(path: str, synonyms: list[str]) -> float:
    leaf = _leaf(path)
    full = normalize_name(path)
    best = 0.0
    for syn in synonyms:
        if leaf == syn or full == syn:
            return 1.0
        s = max(fuzz.ratio(leaf, syn), fuzz.ratio(full, syn)) / 100.0
        # weak fuzzy matches (and any fuzzy match on very short names) are heavily discounted
        if s < 0.75 or min(len(leaf), len(syn)) < 4:
            s *= 0.6
        # token containment ("nominal_transaksi" contains "nominal")
        leaf_tokens = set(leaf.split("_"))
        syn_tokens = set(syn.split("_"))
        if syn_tokens and syn_tokens <= leaf_tokens:
            s = max(s, 0.85 if len(syn_tokens) > 1 or len(syn) >= 4 else 0.7)
        best = max(best, s)
    return best


def _type_factor(field: dict[str, Any], accepted: list[str]) -> float:
    t = field["inferred_type"]
    if t in accepted:
        return 1.0
    if t in ("integer", "number") and "string" in accepted:
        return 0.9
    if t == "string" and ("integer" in accepted or "number" in accepted):
        return 0.6
    return 0.3


def _context_factor(field: dict[str, Any], target: str) -> float:
    ctx = set(load_dictionary()["customer_context"])
    parents = set(_parents(field["path"]))
    under_customer = bool(parents & ctx)
    if target.startswith("customer.") and under_customer:
        return 1.1
    if (
        target == "event.customer_external_id"
        and under_customer
        and _leaf(field["path"]) in ("id", "uid", "no")
    ):
        return 1.3
    if target.startswith("event.") and target != "event.customer_external_id" and under_customer:
        return 0.8
    return 1.0


PII_TARGET = {
    "pan": "event.instrument_fingerprint",
    "email": "customer.email",
    "phone": "customer.phone",
    "account_number": "event.recipient_fingerprint",
    "name": "customer.full_name",
}


def score_candidates(fields: list[dict[str, Any]]) -> list[Candidate]:
    d = load_dictionary()
    out: list[Candidate] = []
    for f in fields:
        if f["path"].endswith("[]") or f["inferred_type"] in ("object", "array"):
            continue
        for target, spec in d["targets"].items():
            sim = name_similarity(f["path"], spec["synonyms"])
            score = sim * _type_factor(f, spec["types"]) * _context_factor(f, target)
            reason = f"name~{sim:.2f}"
            pii = f.get("pii")
            if pii and PII_TARGET.get(pii) == target:
                score = max(score, 0.95 if pii in ("pan", "email") else 0.8)
                reason += f", values look like {pii}"
            elif pii == "pan" and target != "event.instrument_fingerprint":
                score *= 0.2
            elif target in ("event.instrument_fingerprint", "event.recipient_fingerprint") and sim < 0.85:
                score *= 0.3  # never hash a column into a card/account fingerprint on a weak name match
            if target == "event.ip_address" and f.get("_ipv4"):
                score = max(score, 0.9)
                reason += ", values look like IPv4"
            if score > 0.2:
                out.append(Candidate(f["path"], target, round(min(score, 1.0), 3), reason))
    return sorted(out, key=lambda c: -c.score)


def _numeric_transform(field: dict[str, Any]) -> list[dict[str, Any]]:
    samples = [str(s) for s in field.get("sample_values", [])]
    if field["inferred_type"] in ("integer", "number") and all(
        re.fullmatch(r"-?\d+(\.\d+)?", s) for s in samples
    ):
        return [{"fn": "to_number"}]
    indo = any(re.search(r"\d\.\d{3}(,|$)", s) or re.search(r",\d{1,2}$", s) for s in samples)
    return [{"fn": "to_number", "locale": "id"}] if indo else [{"fn": "to_number"}]


def _datetime_transform(field: dict[str, Any]) -> list[dict[str, Any]]:
    fmt = field.get("datetime_format")
    if fmt in (UNIX_S, UNIX_MS, RFC3339):
        return [{"fn": "parse_datetime", "format": fmt}]
    if fmt:
        tz = [] if "%z" in fmt else [("timezone", DEFAULT_TZ)]
        t: dict[str, Any] = {"fn": "parse_datetime", "format": fmt}
        t.update(dict(tz))
        return [t]
    return [{"fn": "parse_datetime", "format": RFC3339}]


def _transforms_for(target: str, field: dict[str, Any]) -> list[dict[str, Any]]:
    d = load_dictionary()
    types = d["targets"].get(target, {}).get("types", [])
    if target == "event.instrument_fingerprint":
        return [{"fn": "hash_pan"}]
    if target == "event.recipient_fingerprint":
        return [{"fn": "hash_account"}]
    if target == "customer.phone":
        return [{"fn": "normalize_phone", "default_country": "ID"}]
    if target == "customer.email":
        return [{"fn": "trim"}, {"fn": "lowercase"}]
    if "datetime" in types:
        return _datetime_transform(field)
    if (types and types[0] in ("number",)) or target.endswith(("amount", "latitude", "longitude")):
        return _numeric_transform(field)
    if target in (
        "event.external_id",
        "event.customer_external_id",
        "event.ref_transaction_id",
        "event.merchant_id",
    ):
        return [{"fn": "to_string"}]
    if target in ("event.geo_country", "event.issuer_country", "event.currency"):
        return [{"fn": "trim"}, {"fn": "uppercase"}]
    if target == "event.login_success":
        return [{"fn": "to_bool"}]
    if target == "customer.kyc_level":
        return [{"fn": "to_number"}]
    return []


def _event_type_spec(
    fields: list[dict[str, Any]], samples: list[dict[str, Any]] | None, default_event_type: str | None
) -> tuple[dict[str, Any], float, str | None]:
    d = load_dictionary()
    best: tuple[float, dict[str, Any] | None] = (0.0, None)
    for f in fields:
        if f["inferred_type"] != "string" or (
            f["distinct_ratio"] > 0.2 and len(f.get("sample_values", [])) > 3
        ):
            continue
        sim = name_similarity(f["path"], d["event_type_field"])
        if sim > best[0]:
            best = (sim, f)
    default = default_event_type or "transaction"
    if best[1] is None or best[0] < 0.7:
        return {"const": default}, 1.0, None
    field = best[1]
    values: set[str] = {str(v) for v in field.get("sample_values", [])}
    if samples:
        from app.inference.flatten import get_path

        values |= {str(v) for r in samples if (v := get_path(r, field["path"])) not in (None, "")}
    value_map: dict[str, str] = {}
    for raw in sorted(values)[:100]:
        norm = normalize_name(raw)
        mapped = None
        for canon, syns in d["event_type_values"].items():
            if norm in syns or any(s in norm.split("_") for s in syns):
                mapped = canon
                break
        value_map[raw] = mapped or norm
    return (
        {"from": field["path"], "value_map": value_map, "default": default},
        round(best[0], 3),
        field["path"],
    )


def _label_spec(fields: list[dict[str, Any]]) -> tuple[dict[str, Any] | None, list[str]]:
    d = load_dictionary()
    used: list[str] = []
    label_field = None
    for f in fields:
        leaf = _leaf(f["path"])
        vals = {str(v).lower() for v in f.get("sample_values", [])}
        binary = f["inferred_type"] == "bool" or vals <= {
            "0",
            "1",
            "true",
            "false",
            "y",
            "n",
            "yes",
            "no",
            "ya",
            "tidak",
            "0.0",
            "1.0",
        }
        if binary and (leaf in d["label_field"] or name_similarity(f["path"], d["label_field"]) >= 0.9):
            label_field = f
            break
    if not label_field:
        return None, used
    used.append(label_field["path"])
    spec: dict[str, Any] = {
        "from": label_field["path"],
        "fraud_values": [1, "1", "true", "True", "TRUE", "Y", "y", "yes", "ya", True, 1.0],
    }
    for f in fields:
        if name_similarity(f["path"], d["fraud_type_field"]) >= 0.9:
            spec["fraud_type"] = {"from": f["path"], "default": "other"}
            used.append(f["path"])
            break
    else:
        spec["fraud_type"] = {"const": "other"}
    return spec, used


def suggest_mapping(
    fields: list[dict[str, Any]],
    samples: list[dict[str, Any]] | None = None,
    default_event_type: str | None = None,
) -> dict[str, Any]:
    """Return {suggested_mapping, confidence, unmapped_fields, notes}."""
    from app.inference.pii import is_ipv4

    if samples:
        from app.inference.flatten import get_path

        for f in fields:
            vals = [get_path(r, f["path"]) for r in samples[:200]]
            vals = [v for v in vals if v not in (None, "")]
            f["_ipv4"] = bool(vals) and sum(1 for v in vals if is_ipv4(v)) / len(vals) >= 0.8
    d = load_dictionary()
    notes: list[str] = []
    confidence: dict[str, float] = {}
    event_type, et_conf, et_source = _event_type_spec(fields, samples, default_event_type)
    confidence["event_type"] = et_conf
    label, label_sources = _label_spec(fields)
    used_sources: set[str] = set(label_sources)
    if et_source:
        used_sources.add(et_source)
    assigned: dict[str, Candidate] = {}
    for c in score_candidates(fields):
        if c.score < MIN_CONFIDENCE or c.target in assigned or c.source in used_sources:
            continue
        assigned[c.target] = c
        used_sources.add(c.source)
    by_path = {f["path"]: f for f in fields}
    event: dict[str, Any] = {}
    customer: dict[str, Any] = {}
    for target, c in sorted(assigned.items()):
        spec: dict[str, Any] = {"from": c.source}
        tr = _transforms_for(target, by_path[c.source])
        if tr:
            spec["transform"] = tr
        section, name = target.split(".", 1)
        (event if section == "event" else customer)[name] = spec
        confidence[target] = c.score
        if target == "event.instrument_fingerprint":
            event["card_bin"] = {"from": c.source, "transform": [{"fn": "pan_bin", "length": 6}]}
            event["card_last4"] = {"from": c.source, "transform": [{"fn": "pan_last4"}]}
            confidence["event.card_bin"] = confidence["event.card_last4"] = c.score
    for req in ("external_id", "occurred_at", "customer_external_id"):
        if req not in event:
            notes.append(f"required field event.{req} could not be matched — map it manually")
    if "currency" not in event:
        event["currency"] = {"const": "IDR"}
        confidence["event.currency"] = 0.5
        notes.append("no currency column found; defaulted to const IDR")
    drop: list[str] = []
    for f in fields:
        leaf = _leaf(f["path"])
        sensitive = f.get("pii") in ("pan", "account_number") or leaf in d["drop_hints"]
        if sensitive and not f["path"].endswith("[]"):
            drop.append(f["path"])
    # source columns feeding a hashing transform are always dropped from the stored payload (raw PAN/account)
    for spec in event.values():
        if any(t.get("fn") in ("hash_pan", "hash_account") for t in spec.get("transform", [])):
            drop.append(spec["from"])
    mapping: dict[str, Any] = {"event_type": event_type, "event": event}
    if customer:
        mapping["customer"] = customer
    if label:
        mapping["label"] = label
        confidence["label"] = 0.9
        notes.append(f"label column detected: {label['from']} — events will be labelled (source=dataset)")
    if drop:
        mapping["drop_fields"] = sorted(set(drop))
    mapped_sources = {
        s
        for spec in list(event.values()) + list(customer.values())
        if isinstance(spec, dict) and isinstance(s := spec.get("from"), str)
    } | used_sources
    unmapped = [
        f["path"]
        for f in fields
        if f["path"] not in mapped_sources
        and not f["path"].endswith("[]")
        and f["inferred_type"] not in ("object",)
    ]
    for f in fields:
        f.pop("_ipv4", None)
    return {
        "suggested_mapping": mapping,
        "confidence": confidence,
        "unmapped_fields": unmapped,
        "notes": notes,
    }


def apply_llm_suggestions(
    result: dict[str, Any],
    fields: list[dict[str, Any]],
    suggestions: list[dict[str, Any]],
    min_conf: float = 0.6,
) -> dict[str, Any]:
    """Merge LLM suggestions for still-unassigned targets from still-unmapped sources."""
    mapping = result["suggested_mapping"]
    by_path = {f["path"]: f for f in fields}
    unmapped = set(result["unmapped_fields"])
    targets = set(canonical_targets())
    for s in sorted(suggestions, key=lambda x: -float(x.get("confidence", 0))):
        src, tgt, conf = s.get("source_path"), s.get("target"), float(s.get("confidence", 0))
        if conf < min_conf or src not in unmapped or tgt not in targets or src not in by_path:
            continue
        section, name = tgt.split(".", 1)
        bucket = mapping["event"] if section == "event" else mapping.setdefault("customer", {})
        if name in bucket:
            continue
        spec: dict[str, Any] = {"from": src}
        tr = _transforms_for(tgt, by_path[src])
        if tr:
            spec["transform"] = tr
        bucket[name] = spec
        result["confidence"][tgt] = round(conf * 0.9, 3)
        unmapped.discard(src)
        result["notes"].append(f"LLM suggested {src} → {tgt}: {s.get('reason', '')}".strip())
    result["unmapped_fields"] = [p for p in result["unmapped_fields"] if p in unmapped]
    return result
