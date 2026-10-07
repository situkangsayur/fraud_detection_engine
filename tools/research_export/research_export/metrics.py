"""Detection metrics computed against the simulator's ground truth (`is_fraud_truth`, `truth_fraud_type`).

Only events that were actually scored (`load_only = false` and a decision exists) are evaluated, mirroring
docs/technical/evaluation.md. Engines without a score on an event (no active model, degraded) are evaluated on the
events where they produced one, and the coverage is reported next to the AUC.
"""

from __future__ import annotations

import numpy as np
import pandas as pd
from sklearn.metrics import average_precision_score, roc_auc_score

ENGINES = {
    "final": "final_score",
    "rules": "score_rules",
    "supervised": "score_supervised",
    "unsupervised": "score_unsupervised",
    "graph": "score_graph",
}


def scored(events: pd.DataFrame) -> pd.DataFrame:
    return events[(~events["load_only"]) & events["decision"].notna()]


def _auc(y: pd.Series, s: pd.Series) -> tuple[float | None, float | None]:
    if y.nunique() < 2:
        return None, None
    return float(roc_auc_score(y, s)), float(average_precision_score(y, s))


def engine_auc(events: pd.DataFrame) -> pd.DataFrame:
    rows = []
    for project, g in scored(events).groupby("project"):
        for engine, col in ENGINES.items():
            have = g[g[col].notna()]
            roc, pr = _auc(have["is_fraud_truth"], have[col])
            rows.append(
                {
                    "project": project,
                    "engine": engine,
                    "events": len(have),
                    "coverage": round(len(have) / len(g), 4) if len(g) else None,
                    "fraud_events": int(have["is_fraud_truth"].sum()),
                    "roc_auc": roc,
                    "pr_auc": pr,
                }
            )
    return pd.DataFrame(rows)


def confusion_at(g: pd.DataFrame, threshold: float) -> dict[str, float | int | None]:
    y, flagged = g["is_fraud_truth"], g["final_score"] >= threshold
    tp, fp = int((y & flagged).sum()), int((~y & flagged).sum())
    fn, tn = int((y & ~flagged).sum()), int((~y & ~flagged).sum())
    return {
        "threshold": threshold,
        "tp": tp,
        "fp": fp,
        "fn": fn,
        "tn": tn,
        "recall": tp / (tp + fn) if tp + fn else None,
        "precision": tp / (tp + fp) if tp + fp else None,
        "fpr": fp / (fp + tn) if fp + tn else None,
    }


def threshold_sweep(events: pd.DataFrame, step: float = 2.5) -> pd.DataFrame:
    rows = []
    for project, g in scored(events).groupby("project"):
        for t in np.arange(0, 100 + step, step):
            rows.append({"project": project, **confusion_at(g, float(t))})
    return pd.DataFrame(rows)


def operating_points(events: pd.DataFrame, max_fpr: float = 0.05) -> pd.DataFrame:
    """Default thresholds (review 50, decline 80) and the lowest threshold whose FPR ≤ max_fpr."""
    sweep = threshold_sweep(events, step=0.5)
    rows = []
    for project, g in scored(events).groupby("project"):
        rows.append({"project": project, "point": "default_review_50", **confusion_at(g, 50.0)})
        rows.append({"project": project, "point": "default_decline_80", **confusion_at(g, 80.0)})
        ok = sweep[(sweep["project"] == project) & (sweep["fpr"].notna()) & (sweep["fpr"] <= max_fpr)]
        if not ok.empty:
            t = float(ok["threshold"].min())
            rows.append({"project": project, "point": f"calibrated_fpr_le_{max_fpr:g}", **confusion_at(g, t)})
    return pd.DataFrame(rows)


def typology_recall(events: pd.DataFrame) -> pd.DataFrame:
    """Recall per fraud typology at review threshold 50, per project."""
    rows = []
    s = scored(events)
    for (project, typ), g in s[s["is_fraud_truth"]].groupby(["project", "truth_fraud_type"]):
        rows.append(
            {
                "project": project,
                "fraud_type": typ,
                "events": len(g),
                "recall_review_50": float((g["final_score"] >= 50).mean()),
                "recall_decline_80": float((g["final_score"] >= 80).mean()),
                "mean_final_score": float(g["final_score"].mean()),
            }
        )
    return pd.DataFrame(rows)


def dataset_summary(events: pd.DataFrame) -> pd.DataFrame:
    rows = []
    for project, g in events.groupby("project"):
        s = scored(g)
        rows.append(
            {
                "project": project,
                "stage": g["project_stage"].iloc[0],
                "events": len(g),
                "load_only_events": int(g["load_only"].sum()),
                "scored_events": len(s),
                "fraud_events_truth": int(g["is_fraud_truth"].sum()),
                "fraud_rate_truth": float(g["is_fraud_truth"].mean()),
                "labelled_events": int(g["event_label"].notna().sum()),
                "from": g["occurred_at"].min(),
                "to": g["occurred_at"].max(),
                "decisions": s["decision"].value_counts().to_dict(),
            }
        )
    return pd.DataFrame(rows)
