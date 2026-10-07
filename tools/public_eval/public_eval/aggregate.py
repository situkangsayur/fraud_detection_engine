"""Collect result-*.json files into one JSON and Markdown tables (mean ± std over seeds when available).

  python -m public_eval.aggregate [--out results-public.json] [--md evaluation-public-tables.md]
"""

from __future__ import annotations

import argparse
import json
import statistics
from collections import defaultdict
from typing import Any

from public_eval.common import WORK_DIR

ORDER = ["tabformer", "ecom", "aml", "rba", "ulb"]
NAMES = {"tabformer": "TabFormer (card)", "ecom": "E-commerce (Fraud_Data)", "aml": "AMLworld HI-Small",
         "rba": "RBA login", "ulb": "ULB credit card"}


def _fmt(vals: list[float | None], pct: bool = False, nd: int = 3) -> str:
    v = [x for x in vals if x is not None]
    if not v:
        return "–"
    k = 100 if pct else 1
    m = statistics.mean(v) * k
    if len(v) > 1:
        sd = statistics.stdev(v) * k
        return f"{m:.1f} ± {sd:.1f}" if pct else f"{m:.{nd}f} ± {sd:.{nd}f}"
    return f"{m:.1f}" if pct else f"{m:.{nd}f}"


def collect() -> dict[tuple[str, str], list[dict[str, Any]]]:
    out: dict[tuple[str, str], list[dict[str, Any]]] = defaultdict(list)
    for f in sorted(WORK_DIR.glob("*-s*/result-*.json")):
        r = json.loads(f.read_text())
        if r.get("metrics", {}).get("events"):
            out[(r["dataset"], r["config"])].append(r)
    return out


def tables(res: dict[tuple[str, str], list[dict[str, Any]]]) -> str:
    lines = ["| Dataset | Config | Seeds | Scored events | Fraud | Recall % | Precision % | FPR % | AUC fused | AUC rules | AUC superv. | AUC anomaly | AUC graph |",
             "|---|---|---|---|---|---|---|---|---|---|---|---|---|"]
    for ds in ORDER:
        for cfg in ("tpl", "lit", "ml"):
            rs = res.get((ds, cfg))
            if not rs:
                continue
            m = [r["metrics"] for r in rs]
            lines.append("| " + " | ".join([
                NAMES[ds], cfg, str(len(rs)), _fmt([x["events"] for x in m], nd=0), _fmt([x["fraud"] for x in m], nd=0),
                _fmt([x["recall"] for x in m], True), _fmt([x["precision"] for x in m], True), _fmt([x["fpr"] for x in m], True),
                _fmt([x["auc"]["final"] for x in m]), _fmt([x["auc"].get("rules") for x in m]),
                _fmt([x["auc"].get("supervised") for x in m]), _fmt([x["auc"].get("unsupervised") for x in m]),
                _fmt([x["auc"].get("graph") for x in m])]) + " |")
    lines += ["", "Per-project review threshold calibrated to FPR ≤ 5% (same scores):", "",
              "| Dataset | Config | Threshold | Recall % | Precision % |", "|---|---|---|---|---|"]
    for ds in ORDER:
        for cfg in ("tpl", "lit", "ml"):
            rs = res.get((ds, cfg))
            if not rs:
                continue
            c = [r["metrics"].get("calibrated") or {} for r in rs]
            lines.append(f"| {NAMES[ds]} | {cfg} | {_fmt([x.get('threshold') for x in c], nd=1)} | "
                         f"{_fmt([x.get('recall') for x in c], True)} | {_fmt([x.get('precision') for x in c], True)} |")
    lines += ["", "Rule hits on the scored window (seed 1):", ""]
    for ds in ORDER:
        for cfg in ("tpl", "lit"):
            rs = res.get((ds, cfg))
            if not rs:
                continue
            h = rs[0]["metrics"].get("rule_hits") or {}
            if not h:
                continue
            lines += [f"**{NAMES[ds]} / {cfg}**", "", "| Rule | Matched | Matched fraud | Precision % | Trapped |", "|---|---|---|---|---|"]
            for code, v in sorted(h.items(), key=lambda kv: -kv[1]["matched"]):
                lines.append(f"| {code} | {v['matched']} | {v['matched_fraud']} | {100 * v['precision']:.1f} | {v['trapped']} |")
            lines.append("")
    rb = [(ds, cfg, rs[0]["metrics"].get("recall_by_type") or {}) for (ds, cfg), rs in sorted(res.items())]
    lines += ["Recall by typology / laundering pattern (seed 1):", ""]
    for ds, cfg, d in rb:
        if d:
            lines.append(f"- {NAMES[ds]} / {cfg}: " + ", ".join(f"{k} {100 * v['recall']:.0f}% (n={v['fraud']})" for k, v in d.items()))
    return "\n".join(lines)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=str(WORK_DIR / "results-public.json"))
    ap.add_argument("--md", default=str(WORK_DIR / "evaluation-public-tables.md"))
    a = ap.parse_args()
    res = collect()
    flat = {f"{ds}/{cfg}": [{k: r[k] for k in ("seed", "project_id", "split", "labels", "models", "metrics", "meta", "ingest_history", "ingest_scored") if k in r} | ({"ruleset": r["ruleset"]} if "ruleset" in r else {})
                            for r in rs] for (ds, cfg), rs in sorted(res.items())}
    with open(a.out, "w") as fh:
        json.dump(flat, fh, indent=2, default=str)
    md = tables(res)
    with open(a.md, "w") as fh:
        fh.write(md + "\n")
    print(md)


if __name__ == "__main__":
    main()
