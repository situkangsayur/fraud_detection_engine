"""Writes README.md (provenance, data dictionary, metric snapshot) next to the exported dataset."""

from __future__ import annotations

from pathlib import Path
from typing import Any

import pandas as pd

EVENT_COLUMNS: dict[str, str] = {
    "project / project_stage": "Demo project slug and protection point (pre_payment, post_payment, promo, returns)",
    "event_id / external_id": "Platform UUID / id sent by the (simulated) client; joins to ground truth",
    "event_type": "TRANSACTION, LOGIN, ACCOUNT_CHANGE, PROMO_REDEMPTION, REFUND, … (canonical event type)",
    "occurred_at / received_at": "Event time (simulated) / ingestion time (UTC)",
    "load_only": "true = historical backfill (features + training only, never scored); false = scored online",
    "customer_*": "Customer external id, KYC level, segment, registration time",
    "channel … has_device": "Canonical transaction attributes (amount IDR, payment method, MCC, geo, promo, …)",
    "decision": "approve / review / decline (latest decision); null for load_only events",
    "final_score": "Fused score 0–100 (weighted noisy-OR of the engine scores, see technical-overview.md §3.1)",
    "score_rules / score_supervised / score_unsupervised / score_graph": "Per-engine score 0–100; null when the "
    "engine had no active model or was degraded",
    "ml_*": "Raw ML outputs: fraud probability, anomaly score, cluster id and the cluster's fraud rate",
    "graph_*": "Graph features at decision time: degree, component size, BFS distance to a known fraudster, fraud "
    "neighbours at 1 and 2 hops, shared entities, community fraud rate",
    "degraded_engines": "Engines dropped from this decision (timeout/unavailable)",
    "latency_ms": "End-to-end decision latency inside core-api",
    "reasons_json": "Top attributed reasons (JSON array)",
    "event_label / event_label_fraud_type": "Label known to the platform (delayed feed / analyst), as an operator "
    "would see it — NOT the ground truth",
    "customer_risk_label / case_status": "Customer-level label and status of the case the event belongs to",
    "truth_fraud_type / is_fraud_truth": "Simulator ground truth (never sent to the platform); use for evaluation",
}

TABLES: dict[str, str] = {
    "events": "One row per event (see dictionary below). Also as CSV.",
    "rule_hits": "Every rule evaluation that matched or was trapped: rule code/kind/version, ruleset, outcome, "
    "contribution, shadow flag",
    "features": "Feature vector used by the engines (`f_*` columns, feature set version) per event",
    "anomaly": "Unsupervised model output per training/scored event: anomaly score, cluster id, 2-D PCA projection",
    "rules / rulesets": "Rule catalogue at export time (definition JSON, risk score, action) and rulesets",
    "models": "Trained models: algorithm, params, metrics JSON (ROC/PR-AUC, confusion, calibration), status",
    "clusters": "Cluster profiles of the unsupervised model (size, fraud rate, top features, analyst label)",
    "cases / labels": "Case queue and every label (source: dataset feed, analyst, case resolution)",
    "proposals / llm_reports": "LLM-assistant rule proposals (citations, validation, backtest, review status) and "
    "analysis reports",
    "graph_communities": "Louvain community statistics",
    "project_settings": "Engine weights, thresholds, timeouts per project",
}


def _md(df: pd.DataFrame, floatfmt: str = ".3f") -> str:
    if df.empty:
        return "_(empty)_"
    return str(df.to_markdown(index=False, floatfmt=floatfmt))


def write_readme(out: Path, manifest: dict[str, Any], results: dict[str, pd.DataFrame]) -> None:
    sim = manifest["simulator"]
    tables = manifest["tables"]
    lines = [
        "# Fraud Detection Platform — research dataset (live demo export)",
        "",
        f"Exported {manifest['created_at']} from tenant `{manifest['tenant']}` of the demo install "
        "(https://fds.hendrikarisma.my.id) by `tools/research_export` "
        "(repo `github.com/situkangsayur/fraud_detection_engine`).",
        "",
        "> **Ringkas (ID):** dataset ini berisi semua event demo beserta skor tiap engine, hit rule, fitur, output ML, "
        "label, case, proposal LLM, dan **ground truth simulator** (`truth_fraud_type`). Siap dipakai untuk analisis "
        "dan tabel paper. Data sepenuhnya sintetis.",
        "",
        "## Provenance",
        "",
        "* Data are **synthetic**, produced by `tools/simulator` and sent to the platform through its webhook ingest "
        "exactly like a client would (custom record shape → mapping → canonical event).",
        f"* Simulator parameters: `{sim}`. The ground truth is regenerated deterministically from these parameters.",
        "* First 70 % of each project's time window is ingested as `load_only` (history for features and training), "
        "the rest is scored online. Labels arrive with a 7-day delay, like a chargeback feed.",
        "* After seeding, one supervised (`mlp_backprop`) and one unsupervised (`isolation_forest` + `hdbscan`) model "
        "were trained, submitted and approved per project (maker–checker). Scores of events ingested **before** "
        "approval therefore have no ML component (`score_supervised` null) — see coverage in `metrics/engine_auc.csv`.",
        "* Names, e-mails, phone numbers and IPs are generated; no real person is represented. "
        "Card and bank-account values exist only as HMAC fingerprints inside the platform and are not exported.",
        "",
        "## Files",
        "",
        "| Table | Rows | Files | Description |",
        "|---|---|---|---|",
    ]
    desc = {k2.strip(): v for k, v in TABLES.items() for k2 in k.split("/")}
    for t in tables:
        files = ", ".join(f"`{f['file']}`" for f in t["files"])
        lines.append(f"| `{t['table']}` | {t['rows']:,} | {files} | {desc.get(t['table'], '')} |")
    lines += [
        "",
        "Also: `metrics/*.csv` (below), `manifest.json` (row counts, SHA-256 of every file), "
        "`ground_truth.jsonl` (when regenerated).",
        "",
        "## `events` data dictionary",
        "",
        "| Column(s) | Meaning |",
        "|---|---|",
        *[f"| `{k}` | {v} |" for k, v in EVENT_COLUMNS.items()],
        "",
        "## Metric snapshot (computed at export time, scored events only)",
        "",
        "### Dataset",
        "",
        _md(results["dataset_summary"].drop(columns=["decisions"])),
        "",
        "### Engine ranking quality (ROC-AUC / PR-AUC vs ground truth)",
        "",
        _md(results["engine_auc"]),
        "",
        "### Operating points (default thresholds and calibrated FPR ≤ 5 %)",
        "",
        _md(results["operating_points"]),
        "",
        "### Recall per fraud typology",
        "",
        _md(results["typology_recall"]),
        "",
        "`metrics/threshold_sweep.csv` has recall/precision/FPR for thresholds 0–100 (step 2.5) per project.",
        "",
        "## Loading",
        "",
        "```python",
        "import pandas as pd",
        "ev = pd.read_parquet('events.parquet')",
        "scored = ev[~ev.load_only & ev.decision.notna()]",
        "hits = pd.read_parquet('rule_hits.parquet')          # join on event_id",
        "feat = pd.read_parquet('features.parquet')           # join on event_id",
        "```",
        "",
        "## Caveats (threats to validity)",
        "",
        "* Synthetic data: typologies and base rates follow the simulator's assumptions, not a real portfolio.",
        "* ML engines only cover events scored after model approval; compare engines on the same subset "
        "(`score_supervised.notna()`).",
        "* `event_label` is delayed and incomplete by design; always evaluate against `is_fraud_truth`.",
        "* The demo is reset every night (00:00 WIB) with the same seed but a new window end; exports from different "
        "days are different samples of the same generator, which can be used for repeated-run confidence intervals.",
        "* Live traffic added after seeding (run tags `live-*`) has its own truth files only if exported with them; "
        "otherwise those events have `is_fraud_truth = false` by construction of the left join — filter them out "
        "with `external_id.str.contains('-live')` if needed.",
        "",
        "## License and citation",
        "",
        "Code: AGPL-3.0-only (author Hendri Karisma). Dataset license: to be decided by the author before publication "
        "(suggested CC BY 4.0). Cite the repository and the paper draft *Five Engines, One Decision* when used.",
        "",
    ]
    (out / "README.md").write_text("\n".join(lines))
