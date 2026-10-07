"""Literature-referenced rulesets ("LIT") per dataset, from the rule catalogue (R01–R40).

Rule code = RL-LIT-<catalogue id>; the description carries the source. [S] = threshold taken from the source,
[P] = placeholder from the catalogue example (not tuned on the test window). Only rules whose inputs exist in a
dataset are included; omissions are listed in OMITTED.
"""

from __future__ import annotations

from typing import Any

F = lambda p: {"type": "field", "path": p}  # noqa: E731
C = lambda v: {"type": "const", "value": v}  # noqa: E731
H = lambda p: {"type": "hist", "path": p}  # noqa: E731


def rule(code: str, name: str, source: str, kind: str, definition: dict[str, Any], typ: list[str],
         ev: list[str], risk: float, weight: float = 1.0) -> dict[str, Any]:
    return {
        "code": f"RL-LIT-{code.upper()}", "name": name, "description": f"Source: {source}", "kind": kind,
        "typologies": typ, "event_types": ev, "risk_score": risk, "trapped_score": 0, "action": "score",
        "on_trapped": "ignore", "missing_as_no_match": True, "definition": definition, "weight": weight,
    }


def simple(when: dict[str, Any]) -> dict[str, Any]:
    return {"kind": "simple", "when": when, "scoring": "binary"}


def cond(left: dict, op: str, right: dict | None = None) -> dict[str, Any]:
    c: dict[str, Any] = {"left": left, "op": op}
    if right is not None:
        c["right"] = right
    return c


def velocity(group_by: list[str], window: str, fn: str, field: str | None, op: str, value: float,
             event_types: list[str], statistic: dict | None = None, min_samples: int = 1,
             include_current: bool = True) -> dict[str, Any]:
    agg: dict[str, Any] = {"fn": fn}
    if field:
        agg["field"] = field
    d: dict[str, Any] = {"kind": "velocity", "history_event_types": event_types, "group_by": group_by,
                         "window": {"duration": window}, "aggregate": agg, "statistic": statistic,
                         "include_current": include_current, "min_samples": min_samples,
                         "compare": {"op": op, "right": C(value)}}
    return d


def graph(metric: str, kinds: list[str], op: str, value: float, depth: int = 3) -> dict[str, Any]:
    return {"kind": "graph", "metric": metric, "link_kinds": kinds, "include_similar": False,
            "max_depth": depth, "compare": {"op": op, "right": C(value)}}


T = ["transaction"]
ZSCORE = {"fn": "zscore"}

VISA26 = "Visa (2026) Enumeration attack mitigation guidance V1.3, p.7-8"
VISA21 = "Visa (2021) VBN AI11312 card-testing"
PSD2 = "Commission Delegated Regulation (EU) 2018/389 (RTS on SCA)"


def tabformer() -> list[dict[str, Any]]:
    tc = ["carding"]
    return [
        rule("R01", "Card testing: many authorisations on one card", f"{VISA26}; Stripe card-testing guidance. ≥5 [S], 10 min [P]",
             "velocity", velocity(["instrument_fingerprint"], "10m", "count", None, "gte", 5, T), tc, T, 50),
        rule("R02", "Burst of small-value transactions at one merchant", f"{VISA26}; {VISA21}. amount < USD 10 [S], 1 h [P], ≥10 [P]",
             "composite", {"kind": "composite",
                           "gate": {"all": [cond(F("event.amount"), "lt", C(10))]},
                           "history_filter": {"all": [cond(H("amount"), "lt", C(10))]},
                           "velocity": {k: v for k, v in velocity(["merchant_id"], "1h", "count", None, "gte", 10, T).items() if k != "kind"}},
             tc, T, 40),
        rule("R03", "Authorisation error indicating card testing (bad CVV / PIN / card number / expiration)",
             f"{VISA21} (fraud-related decline codes); {VISA26} p.8. Error list mapped from TabFormer 'Errors?' [S-equivalent]",
             "simple", simple({"any": [cond(F("source.errors"), "contains", C(x)) for x in
                                       ("Bad CVV", "Bad PIN", "Bad Card Number", "Bad Expiration")]}), tc, T, 45),
        rule("R04", "Many distinct cards at one merchant within a BIN", f"{VISA21}; {VISA26} p.8. 1 h [P], ≥20 [P]",
             "velocity", velocity(["merchant_id", "card_bin"], "1h", "distinct_count", "instrument_fingerprint", "gte", 20, T), tc, T, 40),
        rule("R05", "Many cards per customer account", f"{VISA26} p.8; Stripe card_count_for_* (industry). 24 h [P], ≥3 [P]",
             "velocity", velocity(["customer_id"], "24h", "distinct_count", "instrument_fingerprint", "gte", 3, T), tc, T, 35),
        rule("R08", "Location change within one hour (merchant city as location proxy)",
             "Le Borgne et al. (2022) Reproducible ML for credit card fraud detection (handbook): <1 h [S]; proxy: >1 distinct merchant city in 1 h",
             "velocity", velocity(["customer_id"], "1h", "distinct_count", "geo_city", "gte", 2, T), tc, T, 30),
        rule("R09", "Amount above the highest PSD2 TRA exemption threshold (EUR 500 → USD 500 1:1)",
             f"{PSD2} Art.18-19 + Annex (ETV 100/250/500) [S]",
             "simple", simple({"all": [cond(F("event.amount"), "gt", C(500))]}), tc, T, 20),
        rule("R10a", "Single remote payment above PSD2 low-value limit (EUR 30 → USD 30)", f"{PSD2} Art.16(a) [S]",
             "simple", simple({"all": [cond(F("event.amount"), "gt", C(30))]}), tc, T, 5),
        rule("R10b", "Cumulative low-value spend above EUR 100 (24 h window)", f"{PSD2} Art.16(b) cumulative EUR 100 [S], 24 h window [P]",
             "velocity", velocity(["instrument_fingerprint"], "24h", "sum", "amount", "gt", 100, T), tc, T, 10),
        rule("R10c", "More than five consecutive low-value payments (24 h window)", f"{PSD2} Art.16(b) five transactions [S], 24 h [P]",
             "velocity", velocity(["instrument_fingerprint"], "24h", "count", None, "gt", 5, T), tc, T, 10),
        rule("R11", "Amount abnormal versus customer history (z-score > 3)",
             f"{PSD2} Art.18(2)(c)(i) abnormal spending; Fawcett & Provost (1997). z > 3 [P], 30 d [P]",
             "velocity", velocity(["customer_id"], "30d", "avg", "amount", "gt", 3, T, statistic=ZSCORE, min_samples=5,
                                  include_current=False), tc, T, 35),
        rule("R12", "Night-time transaction (unusual hour proxy)", "Correa Bahnsen et al. (2016); Whitrow et al. (2009). night 00-05 as proxy [P]",
             "simple", simple({"all": [cond(F("features.is_night"), "eq", C(1))]}), tc, T, 10),
    ]


def ecom() -> list[dict[str, Any]]:
    tp = ["promo_abuse"]
    return [
        rule("R31a", "Many accounts on one device (1 day)", "OWASP OAT-019; OWASP API6:2023; FATF (2020) VA red flags; Stripe 1-day window [S], ≥3 [P]",
             "velocity", velocity(["device_id"], "1d", "distinct_count", "customer_id", "gte", 3, T), tp, T, 50),
        rule("R31b", "Many accounts on one IP (1 day)", "OWASP OAT-019; Stripe 1-day window [S], ≥3 [P]",
             "velocity", velocity(["ip_address"], "1d", "distinct_count", "customer_id", "gte", 3, T), tp, T, 40),
        rule("R32a", "Shares a device or IP with another account", "Liu et al. (2018) GEM (Alipay); Fiarni et al. (2022). shared ≥1 [P]",
             "graph", graph("shared_entity_count", ["device", "ip"], "gte", 1), tp, T, 30),
        rule("R32b", "Large connected component of accounts", "Liu et al. (2018) GEM; FATF (2023) CEF. component ≥5 [P]",
             "graph", graph("component_size", ["device", "ip"], "gte", 5), tp, T, 30),
        rule("R20a", "Machine-speed checkout: purchase < 1 s after sign-up", "OWASP API6:2023 (<1 s) [S]; FATF (2023) CEF",
             "simple", simple({"all": [cond(F("features.account_age_days"), "lt", C(1 / 86400))]}), ["promo_abuse"], T, 70),
        rule("R20b", "Purchase shortly after sign-up (< 1 h)", "OWASP OAT-019 new-account abuse; configurable short window 1 h [P]",
             "simple", simple({"all": [cond(F("features.account_age_days"), "lt", C(1 / 24))]}), ["promo_abuse"], T, 30),
        rule("R11", "Amount abnormal versus customer history (z-score > 3)", f"{PSD2} Art.18(2)(c)(i); Fawcett & Provost (1997). z > 3 [P]",
             "velocity", velocity(["customer_id"], "30d", "avg", "amount", "gt", 3, T, statistic=ZSCORE, min_samples=5,
                                  include_current=False), tp, T, 30),
    ]


def aml() -> list[dict[str, Any]]:
    tm = ["money_mule"]
    return [
        rule("R36", "Fan-in: many senders to one recipient (24 h)", "FATF (2020) VA red flags; Altman et al. (2023) AMLworld (fan-in k≥2 [S]); PPATK (2026). ≥10 [P], 24 h [P]",
             "velocity", velocity(["recipient_fingerprint"], "24h", "distinct_count", "customer_id", "gte", 10, T), tm, T, 50),
        rule("R37", "Fan-out: one account pays many recipients (24 h)", "Altman et al. (2023); FATF (2020) VA red flags. 24 h [S FATF], ≥10 [P]",
             "velocity", velocity(["customer_id"], "24h", "distinct_count", "recipient_fingerprint", "gte", 10, T), tm, T, 50),
        rule("R39", "New account moving a high value", "FATF (2023) CEF; FATF (2020) VA. account age < 7 d [P] and amount > 10,000 [P]",
             "simple", simple({"all": [cond(F("features.account_age_days"), "lt", C(7)), cond(F("event.amount"), "gt", C(10000))]}), tm, T, 35),
        rule("R40", "Close to a confirmed launderer in the graph (≤2 hops)", "FATF (2023) CEF; Altman et al. (2023); Van Vlasselaer et al. (2015); Akoglu et al. (2015). ≤2 [P]",
             "graph", graph("distance_to_fraud", ["bank_account"], "lte", 2), tm, T, 60),
        rule("R40b", "Shares a recipient account with other senders", "Altman et al. (2023) gather/scatter patterns; Akoglu et al. (2015). shared ≥1 [P]",
             "graph", graph("shared_entity_count", ["bank_account"], "gte", 1), tm, T, 15),
        rule("R11", "Amount abnormal versus account history (z-score > 3)", f"{PSD2} Art.18(2)(c)(i); UU 8/2010 Ps.1(5); Fawcett & Provost (1997). z > 3 [P]",
             "velocity", velocity(["customer_id"], "30d", "avg", "amount", "gt", 3, T, statistic=ZSCORE, min_samples=5,
                                  include_current=False), tm, T, 30),
    ]


def rba() -> list[dict[str, Any]]:
    ta, L = ["account_takeover"], ["login"]
    return [
        rule("R14", "Five or more failed authentication attempts (24 h)", f"{PSD2} Art.4(3)(b) ≤5 attempts [S]; NIST SP 800-63B-4; PCI DSS 4.0.1 Req. 8.3.4",
             "simple", simple({"all": [cond(F("features.failed_logins_24h"), "gte", C(5))]}), ta, L, 45),
        rule("R15", "Credential stuffing: one IP tries many accounts (10 min)", "OWASP OAT-008; FATF (2023) CEF. ≥20 accounts [P], 10 min [P]",
             "velocity", velocity(["ip_address"], "10m", "distinct_count", "customer_id", "gte", 20, L), ta, L, 50),
        rule("R16", "Login from a new device and an unusual country", f"Wiefling et al. (2019); Freeman et al. (2016); {PSD2} Art.18(2)(c)(ii)",
             "simple", simple({"all": [cond(F("features.is_new_device"), "eq", C(1)), cond(F("features.geo_country_mismatch"), "eq", C(1))]}), ta, L, 60),
        rule("R16b", "Login from a new device", "Wiefling et al. (2019) risk-based authentication; Doerfler et al. (2019)",
             "simple", simple({"all": [cond(F("features.is_new_device"), "eq", C(1))]}), ta, L, 20),
    ]


OMITTED = {
    "tabformer": "R06 (no 3DS/OTP events), R07 (no card/IP country), R13 (merchant-level monthly ratio, not per event)",
    "ecom": "R07 (only IP country; no card/billing country to compare), R33 (no names/emails)",
    "aml": "R26 (no balances), R38 (inflow of an account not linkable to its outgoing events in this mapping), R39 cash threshold (multi-currency)",
    "rba": "R18 (no external VPN/Tor/ASN list; 'Is Attack IP' is a label-like field and is not used), R11 (no amounts)",
}

BUILDERS = {"tabformer": tabformer, "ecom": ecom, "aml": aml, "rba": rba}
EVENT_TYPES = {"rba": ["login"]}
TYPOLOGIES = {"tabformer": ["carding"], "ecom": ["promo_abuse"], "aml": ["money_mule"], "rba": ["account_takeover"]}


def ruleset_for(ds: str) -> dict[str, Any]:
    return {"code": "RS-LIT", "name": "Literature-referenced rules",
            "description": f"Rules from the referenced catalogue (R01-R40). Omitted: {OMITTED.get(ds, '-')}",
            "event_types": EVENT_TYPES.get(ds, ["transaction"]), "typologies": TYPOLOGIES[ds],
            "aggregation": "probabilistic_or", "max_score": 100, "rules": BUILDERS[ds]()}
