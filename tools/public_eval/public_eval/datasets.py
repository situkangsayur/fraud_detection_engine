"""Dataset preparers: raw public file → chronological records (custom flat shape) + truth + mapping + meta.

Records carry `ext_id` and `ts` (RFC 3339, shifted so the newest event is ~1 h before now). Ground truth is
written separately and never sent to the platform. Raw files are untrusted data: only parsed, never executed.
"""

from __future__ import annotations

import csv
import io
import ipaddress
import random
import tarfile
import zipfile
from bisect import bisect_right
from collections import defaultdict
from dataclasses import dataclass, field
from datetime import datetime, timedelta, timezone
from typing import Any

from public_eval.common import RAW_DIR, iso, log, shift_to_now


@dataclass
class Prepared:
    name: str
    records: list[dict[str, Any]]
    truth: dict[str, dict[str, Any]]
    mapping: dict[str, Any]
    stage: str
    description: str
    customer_key: str | None = "cust"
    ml_config: dict[str, Any] | None = None
    graph_config: dict[str, Any] | None = None
    meta: dict[str, Any] = field(default_factory=dict)


def _f(path: str, *transform: dict[str, Any]) -> dict[str, Any]:
    spec: dict[str, Any] = {"from": path}
    if transform:
        spec["transform"] = list(transform)
    return spec


TS = {"fn": "parse_datetime", "format": "rfc3339"}


def _finish(name: str, rows: list[tuple[datetime, dict[str, Any], dict[str, Any]]]) -> tuple[list, dict]:
    """rows = (raw_dt, record_without_ts, truth). Sort, shift to now, assign ts."""
    rows.sort(key=lambda r: r[0])
    off = shift_to_now([r[0] for r in rows])
    recs, truth = [], {}
    for dt, rec, t in rows:
        rec["ts"] = iso(dt + off)
        recs.append(rec)
        truth[rec["ext_id"]] = t
    fraud = sum(1 for t in truth.values() if t["is_fraud"])
    span = (rows[-1][0] - rows[0][0]).total_seconds() / 86400
    log(f"{name}: {len(recs)} events, {fraud} fraud ({fraud / max(1, len(recs)):.3%}), span {span:.1f} days")
    return recs, truth



_MANUAL_ISO = {
    "Korea Republic of": "KR", "Taiwan; Republic of China (ROC)": "TW", "Iran (ISLAMIC Republic Of)": "IR",
    "Turkey": "TR", "Palestinian Territory Occupied": "PS", "Moldova Republic of": "MD",
    "Croatia (LOCAL Name: Hrvatska)": "HR", "Bosnia and Herzegowina": "BA", "Libyan Arab Jamahiriya": "LY",
    "Slovakia (SLOVAK Republic)": "SK", "Macau": "MO", "Wallis and Futuna Islands": "WF",
    "Tanzania United Republic of": "TZ", "Cape Verde": "CV", "Congo The Democratic Republic of The": "CD",
    "Swaziland": "SZ", "Virgin Islands (BRITISH)": "VG", "Virgin Islands (U.S.)": "VI",
    "St. Pierre and Miquelon": "PM", "Micronesia Federated States of": "FM", "Bonaire; Sint Eustatius; Saba": "BQ",
    "Korea Democratic People's Republic of": "KP", "European Union": None,
}


def _iso2(name: str | None) -> str | None:
    if not name:
        return None
    if name in _MANUAL_ISO:
        return _MANUAL_ISO[name]
    import pycountry

    try:
        return pycountry.countries.lookup(name).alpha_2
    except LookupError:
        try:
            return pycountry.countries.search_fuzzy(name)[0].alpha_2
        except LookupError:
            return None


# ------------------------------------------------------------------------------------------- e-commerce
def ecom(seed: int) -> Prepared:
    """Fraud_Data e-commerce (120k-row copy). One purchase per user; device_id + IP shared across users."""
    d = RAW_DIR / "ecom"
    ranges: list[tuple[float, float, str]] = []
    with open(d / "IpAddress_to_Country.csv", newline="") as fh:
        for r in csv.DictReader(fh):
            ranges.append((float(r["lower_bound_ip_address"]), float(r["upper_bound_ip_address"]), _iso2(r["country"])))
    ranges.sort()
    lows = [r[0] for r in ranges]

    def country(ip: float) -> str | None:
        i = bisect_right(lows, ip) - 1
        return ranges[i][2] if i >= 0 and ranges[i][0] <= ip <= ranges[i][1] else None

    rows = []
    with open(d / "fraud.csv", newline="") as fh:
        for r in csv.DictReader(fh):
            ipf = float(r["ip_address"])
            try:
                ip = str(ipaddress.IPv4Address(int(ipf) % (2**32)))
            except ValueError:
                ip = None
            dt = datetime.strptime(r["purchase_time"], "%Y-%m-%d %H:%M:%S").replace(tzinfo=timezone.utc)
            su = datetime.strptime(r["signup_time"], "%Y-%m-%d %H:%M:%S").replace(tzinfo=timezone.utc)
            uid = f"EC-U{r['user_id']}"
            rec = {
                "ext_id": f"EC-{r['user_id']}", "cust": uid, "amount": float(r["purchase_value"]),
                "device": r["device_id"], "ip": ip, "country": country(ipf), "signup_raw": su,
                "source_channel": r["source"], "browser": r["browser"], "sex": r["sex"], "age": int(r["age"]),
            }
            rows.append((dt, rec, {"is_fraud": r["class"] == "1", "typology": "other"}))
    # shift signup times by the same offset as purchases
    rows.sort(key=lambda x: x[0])
    off = shift_to_now([r[0] for r in rows])
    for _, rec, _ in rows:
        su = rec.pop("signup_raw")
        rec["signup"] = iso(su + off)
    recs, truth = _finish("ecom", rows)
    mapping = {
        "event_type": {"const": "transaction"},
        "event": {
            "external_id": _f("ext_id"), "occurred_at": _f("ts", TS),
            "customer_external_id": _f("cust"), "amount": _f("amount", {"fn": "to_number"}),
            "currency": {"const": "USD"}, "channel": {"const": "web"}, "device_id": _f("device"),
            "ip_address": _f("ip"), "geo_country": _f("country"),
        },
        "customer": {"registered_at": _f("signup", TS), "attributes": {"age": _f("age", {"fn": "to_number"})}},
    }
    return Prepared("ecom", recs, truth, mapping, "promo",
                    "Fraud_Data e-commerce purchases (public copy, 120k rows): device and IP shared across accounts",
                    graph_config={"link_kinds": ["device", "ip"], "include_similar": False, "max_depth": 3},
                    meta={"source": "github.com/lu-16/FraudDetection (Fraud_Data copy)", "licence": "unknown (internal test only)",
                          "synthetic": "unknown origin, likely synthetic", "typology": "e-commerce purchase fraud (generic)"})


# ------------------------------------------------------------------------------------------- TabFormer
def tabformer(seed: int, months: int = 4, target_events: int = 80_000) -> Prepared:
    """IBM TabFormer synthetic card transactions. Window = the `months`-month period with the most fraud; users = all users
    with fraud in the window (up to half the target) plus a random sample of others (whole histories inside the window)."""
    tgz = RAW_DIR / "tabformer" / "transactions.tgz"
    by_user: dict[str, list[tuple[datetime, dict[str, str]]]] = defaultdict(list)
    log("tabformer: scanning transactions.tgz (24M rows, a few minutes)")
    # pass 1: count transactions and fraud per month, then pick the `months`-month window with the most fraud
    per_month: dict[tuple[int, int], list[int]] = defaultdict(lambda: [0, 0])
    with tarfile.open(tgz, "r:gz") as tf:
        member = next(m for m in tf.getmembers() if m.name.endswith(".csv"))
        fh = io.TextIOWrapper(tf.extractfile(member), newline="")  # type: ignore[arg-type]
        rd = csv.reader(fh)
        header = next(rd)
        idx = {h: i for i, h in enumerate(header)}
        for row in rd:
            c = per_month[(int(row[idx["Year"]]), int(row[idx["Month"]]))]
            c[0] += 1
            c[1] += row[idx["Is Fraud?"]] == "Yes"
    keys = sorted(per_month)
    best, best_f = 0, -1
    for i in range(len(keys) - months + 1):
        f = sum(per_month[k][1] for k in keys[i:i + months])
        if f > best_f:
            best, best_f = i, f
    y0, m0 = keys[best]
    y1, m1 = keys[best + months - 1]
    start = datetime(y0, m0, 1, tzinfo=timezone.utc)
    newest = (datetime(y1 + (m1 == 12), m1 % 12 + 1, 1, tzinfo=timezone.utc)) - timedelta(seconds=1)
    log(f"tabformer: newest {newest:%Y-%m-%d}, window from {start:%Y-%m-%d}")
    with tarfile.open(tgz, "r:gz") as tf:
        member = next(m for m in tf.getmembers() if m.name.endswith(".csv"))
        fh = io.TextIOWrapper(tf.extractfile(member), newline="")  # type: ignore[arg-type]
        rd = csv.reader(fh)
        next(rd)
        for row in rd:
            y = int(row[idx["Year"]])
            if y < start.year or y > newest.year:
                continue
            hh, mi = (int(x) for x in row[idx["Time"]].split(":"))
            dt = datetime(y, int(row[idx["Month"]]), int(row[idx["Day"]]), hh, mi, tzinfo=timezone.utc)
            if dt < start or dt > newest:
                continue
            by_user[row[idx["User"]]].append((dt, dict(zip(header, row))))
    rng = random.Random(f"tabformer:{seed}")
    fraud_users = sorted(u for u, rows in by_user.items() if any(r["Is Fraud?"] == "Yes" for _, r in rows))
    others = sorted(set(by_user) - set(fraud_users))
    rng.shuffle(others)
    rng.shuffle(fraud_users)
    chosen, total = [], 0
    for u in fraud_users:  # users with fraud in the window, up to half the target (whole histories)
        if total >= target_events // 2:
            break
        chosen.append(u)
        total += len(by_user[u])
    n_fraud_users = len(chosen)
    for u in others:
        if total >= target_events:
            break
        chosen.append(u)
        total += len(by_user[u])
    rows = []
    seq = 0
    for u in chosen:
        for dt, r in by_user[u]:
            seq += 1
            card = f"4{int(r['User']):06d}{int(r['Card']):02d}0000000"[:16]  # synthetic PAN-like id → hashed
            err = r.get("Errors?", "") or ""
            rec = {
                "ext_id": f"TF-{seq:08d}", "cust": f"TF-U{r['User']}", "card": card,
                "amount": float(r["Amount"].replace("$", "").replace(",", "")), "chip": r["Use Chip"],
                "merchant": r["Merchant Name"], "mcc": r["MCC"], "city": r["Merchant City"],
                "state": r["Merchant State"] or None, "zip": r["Zip"] or None, "errors": err or None,
                "has_error": bool(err),
            }
            rows.append((dt, rec, {"is_fraud": r["Is Fraud?"] == "Yes", "typology": "carding"}))
    recs, truth = _finish("tabformer", rows)
    mapping = {
        "event_type": {"const": "transaction"},
        "event": {
            "external_id": _f("ext_id"), "occurred_at": _f("ts", TS), "customer_external_id": _f("cust"),
            "amount": _f("amount", {"fn": "to_number"}), "currency": {"const": "USD"},
            "channel": {"from": "chip", "transform": [{"fn": "value_map", "map": {"Online Transaction": "web",
                        "Swipe Transaction": "other", "Chip Transaction": "other"}, "default": "other"}]},
            "payment_method": {"const": "card"},
            "instrument_fingerprint": _f("card", {"fn": "hash_pan"}),
            "card_bin": _f("card", {"fn": "pan_bin", "length": 6}), "card_last4": _f("card", {"fn": "pan_last4"}),
            "merchant_id": _f("merchant"), "merchant_category": _f("mcc"), "geo_city": _f("city"),
        },
        "drop_fields": ["card"],
    }
    return Prepared("tabformer", recs, truth, mapping, "pre_payment",
                    "IBM TabFormer synthetic card transactions (user, card, merchant, MCC, chip/online, error codes)",
                    meta={"source": "github.com/IBM/TabFormer data/credit_card/transactions.tgz", "licence": "Apache-2.0",
                          "synthetic": True, "window_months": months, "window": f"{start:%Y-%m} to {newest:%Y-%m}", "users": len(chosen),
                          "fraud_users": n_fraud_users, "fraud_users_in_window": len(fraud_users), "sampling": "users with fraud in window up to half the target + random others"})


# ------------------------------------------------------------------------------------------- AMLworld
def _aml_patterns() -> dict[tuple[str, str, str, str], str]:
    out: dict[tuple[str, str, str, str], str] = {}
    cur = None
    with open(RAW_DIR / "aml-hi-small" / "HI-Small_Patterns.txt", newline="") as fh:
        for line in fh:
            line = line.strip()
            if line.startswith("BEGIN LAUNDERING ATTEMPT"):
                cur = line.split("-", 1)[1].split(":")[0].strip()
            elif line.startswith("END LAUNDERING ATTEMPT"):
                cur = None
            elif cur and line:
                p = next(csv.reader([line]))
                out[(p[0], p[2], p[4], p[7])] = cur  # timestamp, from account, to account, amount paid
    return out


def aml(seed: int, target_events: int = 80_000, max_account_tx: int = 500) -> Prepared:
    """IBM AMLworld HI-Small: sender accounts = customers; recipient account → recipient_fingerprint
    (bank-account entity). Accounts = all accounts in a random half of the laundering attempts + random others."""
    d = RAW_DIR / "aml-hi-small"
    pats = _aml_patterns()
    rng = random.Random(f"aml:{seed}")
    by_sender: dict[str, list[list[str]]] = defaultdict(list)
    with open(d / "HI-Small_Trans.csv", newline="") as fh:
        rd = csv.reader(fh)
        header = next(rd)
        for p in rd:
            by_sender[f"{p[1]}|{p[2]}"].append(p)
    # accounts touching laundering (as senders) from the transaction file
    laund_senders = sorted({s for s, rows in by_sender.items() if any(r[10] == "1" for r in rows)})
    rng.shuffle(laund_senders)
    laund_accounts: set[str] = set()
    total = 0
    for a in laund_senders:  # whole histories; skip hub accounts (> max_account_tx) to keep the sample small
        if len(by_sender[a]) > max_account_tx:
            continue
        if total >= target_events // 2:
            break
        laund_accounts.add(a)
        total += len(by_sender[a])
    others = sorted(a for a in set(by_sender) - set(laund_senders) if len(by_sender[a]) <= max_account_tx)
    rng.shuffle(others)
    chosen = set(laund_accounts)
    for a in others:
        if total >= target_events:
            break
        chosen.add(a)
        total += len(by_sender[a])
    rows = []
    seq = 0
    for a in chosen:
        for p in by_sender[a]:
            seq += 1
            dt = datetime.strptime(p[0], "%Y/%m/%d %H:%M").replace(tzinfo=timezone.utc)
            typ = pats.get((p[0], p[2], p[4], p[7]))
            is_l = p[10] == "1"
            rec = {
                "ext_id": f"AML-{seq:08d}", "cust": f"AML-{p[1]}-{p[2]}", "to_account": f"{int(p[3])}{int(p[4], 16)}",  # bank + hex account → digits for hash_account
                "to_bank": p[3], "amount": float(p[7]), "currency": p[8], "format": p[9],
            }
            rows.append((dt, rec, {"is_fraud": is_l, "typology": "money_mule", "pattern": (typ or ("UNLISTED" if is_l else None))}))
    recs, truth = _finish("aml", rows)
    mapping = {
        "event_type": {"const": "transaction"},
        "event": {
            "external_id": _f("ext_id"), "occurred_at": _f("ts", TS), "customer_external_id": _f("cust"),
            "amount": _f("amount", {"fn": "to_number"}), "currency": {"const": "USD"},
            "payment_method": {"from": "format", "transform": [{"fn": "value_map", "map": {
                "Credit Card": "card", "Wire": "bank_transfer", "ACH": "bank_transfer", "Cheque": "other",
                "Cash": "other", "Reinvestment": "other", "Bitcoin": "other"}, "default": "other"}]},
            "recipient_fingerprint": _f("to_account", {"fn": "hash_account"}),
        },
        "drop_fields": ["to_account"],
    }
    return Prepared("aml", recs, truth, mapping, "payout",
                    "IBM AMLworld HI-Small synthetic transfers with labelled laundering patterns",
                    graph_config={"link_kinds": ["bank_account"], "include_similar": False, "max_depth": 3},
                    meta={"source": "IBM AMLworld HI-Small (Hugging Face mirror OsamaMIT/IBM-AML-HI-Small)",
                          "licence": "CDLA-Sharing-1.0", "synthetic": True, "accounts": len(chosen),
                          "laundering_accounts": len(laund_accounts), "sampling": f"laundering senders (whole histories, <= {max_account_tx} tx) up to half the target + random others"})


# ------------------------------------------------------------------------------------------- RBA
def rba(seed: int, target_events: int = 110_000, max_user_events: int = 2_000) -> Prepared:
    """RBA login dataset (Wiefling et al.). Users = all users with an ATO login + random others."""
    z = RAW_DIR / "rba" / "rba-dataset.zip"
    log("rba: pass 1 — finding account-takeover users (33M rows)")
    ato_users: set[str] = set()
    counts: dict[str, int] = defaultdict(int)
    with zipfile.ZipFile(z) as zf:
        name = next(n for n in zf.namelist() if n.endswith(".csv"))
        with zf.open(name) as raw:
            rd = csv.reader(io.TextIOWrapper(raw, newline=""))
            header = next(rd)
            idx = {h: i for i, h in enumerate(header)}
            iu, iato = idx["User ID"], idx["Is Account Takeover"]
            for p in rd:
                counts[p[iu]] += 1
                if p[iato] in ("True", "true", "1"):
                    ato_users.add(p[iu])
    rng = random.Random(f"rba:{seed}")
    others = sorted(u for u, c in counts.items() if u not in ato_users and c <= max_user_events)
    rng.shuffle(others)
    chosen = set(ato_users)
    total = sum(counts[u] for u in chosen)
    for u in others:
        if total >= target_events:
            break
        chosen.add(u)
        total += counts[u]
    log(f"rba: {len(ato_users)} ATO users, {len(chosen)} users chosen, ~{total} logins; pass 2")
    rows = []
    seq = 0
    with zipfile.ZipFile(z) as zf:
        with zf.open(name) as raw:
            rd = csv.reader(io.TextIOWrapper(raw, newline=""))
            next(rd)
            for p in rd:
                if p[idx["User ID"]] not in chosen:
                    continue
                seq += 1
                ts = p[idx["Login Timestamp"]]
                dt = datetime.fromisoformat(ts.replace(" ", "T")).replace(tzinfo=timezone.utc)
                ua = p[idx["User Agent String"]]
                dev = f"{p[idx['Device Type']]}|{p[idx['OS Name and Version']]}|{p[idx['Browser Name and Version']]}"
                rec = {
                    "ext_id": f"RBA-{seq:09d}", "cust": f"RBA-U{p[idx['User ID']]}", "ip": p[idx["IP Address"]],
                    "country": p[idx["Country"]] or None, "city": p[idx["City"]] or None, "asn": p[idx["ASN"]],
                    "ua": ua[:400], "device": dev[:200], "success": p[idx["Login Successful"]] in ("True", "true", "1"),
                    "rtt": p[idx["Round-Trip Time [ms]"]] or None,
                }
                is_ato = p[idx["Is Account Takeover"]] in ("True", "true", "1")
                rows.append((dt, rec, {"is_fraud": is_ato, "typology": "account_takeover"}))
    # ATO labels exist only in the first ~9 months of the source data: end the window 3 days after the last ATO
    last_ato = max(dt for dt, _, t in rows if t["is_fraud"])
    rows = [r for r in rows if r[0] <= last_ato + timedelta(days=3)]
    recs, truth = _finish("rba", rows)
    mapping = {
        "event_type": {"const": "login"},
        "event": {
            "external_id": _f("ext_id"), "occurred_at": _f("ts", TS), "customer_external_id": _f("cust"),
            "channel": {"const": "web"}, "ip_address": _f("ip"), "geo_country": _f("country", {"fn": "uppercase"}),
            "geo_city": _f("city"), "user_agent": _f("ua"), "device_id": _f("device"),
            "login_success": _f("success", {"fn": "to_bool"}),
        },
    }
    return Prepared("rba", recs, truth, mapping, "account_security",
                    "RBA login dataset (synthesized from real SSO logs): IP, ASN, geo, user agent, success, ATO label",
                    graph_config={"link_kinds": ["device", "ip"], "include_similar": False, "max_depth": 2},
                    meta={"source": "Zenodo 10.5281/zenodo.6782156", "licence": "CC BY 4.0", "synthetic": "synthesized from real logs",
                          "ato_users": len(ato_users), "users": len(chosen),
                          "note": "'Is Attack IP' is label-like and is NOT mapped; window ends 3 days after the last ATO label"})


# ------------------------------------------------------------------------------------------- ULB
def ulb(seed: int, legit: int = 60_000) -> Prepared:
    """ULB credit card (real, PCA). No entity ids → ML-only benchmark: each event its own customer;
    V1–V28 + Amount as extra source fields for the models. All 492 frauds + `legit` random legit rows."""
    import pandas as pd

    df = pd.read_parquet(RAW_DIR / "ulb" / "creditcard.parquet")
    df["Class"] = df["Class"].astype(str).str.strip("'").astype(int)
    fr = df[df["Class"] == 1]
    lg = df[df["Class"] == 0].sample(n=legit, random_state=seed)
    sub = pd.concat([fr, lg]).sort_values("Time")
    base = datetime(2013, 9, 1, tzinfo=timezone.utc)
    rows = []
    for i, r in enumerate(sub.itertuples(index=False)):
        dt = base + timedelta(seconds=float(r.Time))
        rec = {"ext_id": f"ULB-{i:07d}", "cust": f"ULB-C{i:07d}", "amount": float(r.Amount)}
        for k in range(1, 29):
            rec[f"V{k}"] = float(getattr(r, f"V{k}"))
        rows.append((dt, rec, {"is_fraud": int(r.Class) == 1, "typology": "carding"}))
    recs, truth = _finish("ulb", rows)
    mapping = {
        "event_type": {"const": "transaction"},
        "event": {"external_id": _f("ext_id"), "occurred_at": _f("ts", TS), "customer_external_id": _f("cust"),
                  "amount": _f("amount", {"fn": "to_number"}), "currency": {"const": "EUR"},
                  "payment_method": {"const": "card"}},
    }
    extra = [f"source.V{k}" for k in range(1, 29)]
    return Prepared("ulb", recs, truth, mapping, "pre_payment",
                    "ULB credit card fraud (real, PCA features V1-V28); ML-only benchmark, no entity ids",
                    customer_key=None,
                    ml_config={"supervised": {"algorithm": "mlp_backprop", "params": {}},
                               "unsupervised": {"anomaly_algorithm": "isolation_forest", "anomaly_params": {},
                                                "clustering_algorithm": "hdbscan", "clustering_params": {}},
                               "features": {"include": ["*"], "exclude": [], "extra_source_fields": extra}},
                    meta={"source": "OpenML 1597", "licence": "DbCL v1.0", "synthetic": False,
                          "sampling": f"all 492 fraud + {legit} random legit (prevalence raised)"})


PREPARERS = {"ecom": ecom, "tabformer": tabformer, "aml": aml, "rba": rba, "ulb": ulb}
