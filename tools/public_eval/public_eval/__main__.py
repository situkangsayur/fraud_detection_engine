"""CLI.

  python -m public_eval prepare <dataset> [--seed N]
  python -m public_eval run <dataset> [--seed N] [--config tpl|lit|ml] [--history-share 0.7]
  python -m public_eval evaluate <dataset> [--seed N] [--config ...]

Datasets: ecom, tabformer, aml, rba, ulb. Configs: tpl = stage-template rules; lit = literature ruleset only
(template none); ml = template none, no rules (ULB benchmark).
"""

from __future__ import annotations

import argparse
import json
import pickle
from typing import Any

from public_eval import common
from public_eval.common import WORK_DIR, log
from public_eval.datasets import PREPARERS, Prepared


def prepared(ds: str, seed: int, refresh: bool = False) -> Prepared:
    d = WORK_DIR / f"{ds}-s{seed}"
    d.mkdir(parents=True, exist_ok=True)
    f = d / "prepared.pkl"
    if f.exists() and not refresh:
        return pickle.loads(f.read_bytes())
    p = PREPARERS[ds](seed)
    f.write_bytes(pickle.dumps(p))
    (d / "truth.json").write_text(json.dumps(p.truth))
    (d / "meta.json").write_text(json.dumps(p.meta, indent=2, default=str))
    return p


TAG = "v3"  # bump to start fresh projects


def slug(ds: str, config: str, seed: int) -> str:
    return f"{ds}-{config}-s{seed}-{TAG}"


def run(ds: str, seed: int, config: str, history_share: float) -> dict[str, Any]:
    p = prepared(ds, seed)
    s = common.sessions()
    template = p.stage if config == "tpl" else "none"
    pid, key = common.setup_project(
        s, slug(ds, config, seed), f"{ds} {config} seed {seed}", p.stage, template, p.mapping,
        description=p.description, ml_config=p.ml_config, graph_config=p.graph_config)
    log(f"project {slug(ds, config, seed)} = {pid}")
    out: dict[str, Any] = {"dataset": ds, "seed": seed, "config": config, "project_id": pid, "meta": p.meta}
    if config == "lit":
        from public_eval.rules_lit import ruleset_for

        rs = ruleset_for(ds)
        out["ruleset"] = common.load_ruleset(s, pid, rs)
        log(f"  LIT ruleset loaded: {out['ruleset']['rules']}")
    recs = p.records
    # chronological split by event count (time-share splits break on skewed timelines, e.g. AMLworld's tail)
    k = int(len(recs) * history_share)
    hist, test = recs[:k], recs[k:]
    cut = test[0]["ts"] if test else recs[-1]["ts"]
    out["split"] = {"history": len(hist), "scored": len(test), "cutoff": cut,
                    "history_fraud": sum(p.truth[r["ext_id"]]["is_fraud"] for r in hist),
                    "scored_fraud": sum(p.truth[r["ext_id"]]["is_fraud"] for r in test)}
    log(f"  split {out['split']}")
    out["ingest_history"] = common.ingest(s, key, hist, "load_only", ds)
    out["labels"] = common.post_labels(s, pid, hist, p.truth, seed, "ts", customer_key=p.customer_key)
    log(f"  labels {out['labels']}")
    out["models"] = common.train_and_approve(s, pid)
    out["ingest_scored"] = common.ingest(s, key, test, "score", ds)
    from public_eval.evaluate import metrics

    out["metrics"] = metrics(pid, p.truth)
    f = WORK_DIR / f"{ds}-s{seed}" / f"result-{config}.json"
    f.write_text(json.dumps(out, indent=2, default=str))
    log(f"  done → {f}")
    return out


def main() -> None:
    ap = argparse.ArgumentParser(prog="public_eval")
    ap.add_argument("cmd", choices=["prepare", "run", "evaluate"])
    ap.add_argument("dataset", choices=sorted(PREPARERS))
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--config", default="tpl", choices=["tpl", "lit", "ml"])
    ap.add_argument("--history-share", type=float, default=0.7)
    ap.add_argument("--refresh", action="store_true")
    a = ap.parse_args()
    if a.cmd == "prepare":
        p = prepared(a.dataset, a.seed, refresh=a.refresh)
        log(json.dumps(p.meta, default=str))
    elif a.cmd == "run":
        run(a.dataset, a.seed, a.config, a.history_share)
    else:
        from public_eval.evaluate import dumps, metrics

        p = prepared(a.dataset, a.seed)
        pid = common.query("SELECT id FROM core.projects WHERE slug = %s", (slug(a.dataset, a.config, a.seed),))[0][0]
        print(dumps(metrics(str(pid), p.truth)))


if __name__ == "__main__":
    main()
