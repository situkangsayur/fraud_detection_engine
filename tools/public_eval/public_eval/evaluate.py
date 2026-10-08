"""Join stored decisions with the local truth file and compute detection metrics."""

from __future__ import annotations

import json
from collections import Counter, defaultdict
from typing import Any

import numpy as np
from sklearn.metrics import roc_auc_score

from public_eval.common import query

ENGINES = ("rules", "supervised", "unsupervised", "graph")


def _auc(y: np.ndarray, s: np.ndarray) -> float | None:
    if len(set(y.tolist())) < 2:
        return None
    return float(roc_auc_score(y, s))


def fetch(pid: str) -> list[dict[str, Any]]:
    rows = query(
        """SELECT e.external_id, d.decision, d.final_score, d.engine_scores, d.rule_results, d.degraded
           FROM core.decisions d JOIN core.events e ON e.id = d.event_id WHERE d.project_id = %s""",
        (pid,),
    )
    return [
        {"ext": r[0], "decision": r[1], "score": float(r[2]), "engines": r[3] or {}, "rules": r[4] or [],
         "degraded": list(r[5] or [])}
        for r in rows
    ]


def metrics(pid: str, truth: dict[str, dict[str, Any]], fpr_target: float = 0.05) -> dict[str, Any]:
    rows = [r for r in fetch(pid) if r["ext"] in truth]
    if not rows:
        return {"events": 0}
    y = np.array([1 if truth[r["ext"]]["is_fraud"] else 0 for r in rows])
    s = np.array([r["score"] for r in rows])
    flagged = np.array([r["decision"] in ("review", "decline") for r in rows])
    declined = np.array([r["decision"] == "decline" for r in rows])
    tp, fp = int((flagged & (y == 1)).sum()), int((flagged & (y == 0)).sum())
    out: dict[str, Any] = {
        "events": len(rows), "fraud": int(y.sum()),
        "recall": tp / max(1, int(y.sum())), "precision": tp / max(1, tp + fp),
        "fpr": fp / max(1, int((y == 0).sum())),
        "decline_precision": int((declined & (y == 1)).sum()) / max(1, int(declined.sum())),
        "decisions": dict(Counter(r["decision"] for r in rows)),
        "auc": {"final": _auc(y, s)},
        "degraded_rate": float(np.mean([bool(r["degraded"]) for r in rows])),
        "degraded": dict(Counter(d for r in rows for d in r["degraded"])),
    }
    for eng in ENGINES:
        vals = [r["engines"].get(eng) for r in rows]
        present = [v is not None for v in vals]
        if not any(present):
            out["auc"][eng] = None
            out.setdefault("engine_missing", []).append(eng)
            continue
        ev = np.array([float(v) if v is not None else 0.0 for v in vals])
        out["auc"][eng] = _auc(y, ev)
        out.setdefault("engine_nonzero_share", {})[eng] = float(np.mean(ev > 0))
    # the same headline metrics on events scored with all engines available (not degraded)
    nd = np.array([not r["degraded"] for r in rows])
    if nd.any():
        yn, sn, fn = y[nd], s[nd], flagged[nd]
        tpn, fpn = int((fn & (yn == 1)).sum()), int((fn & (yn == 0)).sum())
        out["non_degraded"] = {"events": int(nd.sum()), "fraud": int(yn.sum()), "auc_final": _auc(yn, sn),
                               "recall": tpn / max(1, int(yn.sum())), "precision": tpn / max(1, tpn + fpn),
                               "fpr": fpn / max(1, int((yn == 0).sum()))}
    # threshold for FPR <= target
    legit = np.sort(s[y == 0])
    if len(legit):
        k = int(np.floor(len(legit) * (1 - fpr_target)))
        thr = float(legit[min(k, len(legit) - 1)]) + 1e-6
        f = s >= thr
        tp2, fp2 = int((f & (y == 1)).sum()), int((f & (y == 0)).sum())
        out["calibrated"] = {"fpr_target": fpr_target, "threshold": thr,
                             "recall": tp2 / max(1, int(y.sum())), "precision": tp2 / max(1, tp2 + fp2),
                             "fpr": fp2 / max(1, int((y == 0).sum()))}
    # per typology / pattern recall
    by: dict[str, list[int]] = defaultdict(lambda: [0, 0])
    for r, fl in zip(rows, flagged):
        t = truth[r["ext"]]
        if t["is_fraud"]:
            key = t.get("pattern") or t["typology"]
            by[key][0] += 1
            by[key][1] += int(fl)
    out["recall_by_type"] = {k: {"fraud": v[0], "recall": v[1] / max(1, v[0])} for k, v in sorted(by.items())}
    # rule hits
    hits: dict[str, list[int]] = defaultdict(lambda: [0, 0, 0])  # matched, matched∧fraud, trapped
    for r in rows:
        is_f = truth[r["ext"]]["is_fraud"]
        for it in r["rules"]:
            code = it.get("rule_code") or it.get("rule_id")
            if it.get("outcome") == "match":
                hits[code][0] += 1
                hits[code][1] += int(is_f)
            elif it.get("outcome") == "trapped":
                hits[code][2] += 1
    out["rule_hits"] = {k: {"matched": v[0], "matched_fraud": v[1], "trapped": v[2],
                            "precision": v[1] / max(1, v[0])} for k, v in sorted(hits.items())}
    return out


def dumps(m: dict[str, Any]) -> str:
    return json.dumps(m, indent=2, default=str)
