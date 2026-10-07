#!/usr/bin/env python3
"""Simulates a working day of the fraud team on the demo, so every screen has realistic content.

Per project:
  * case work: the analyst takes open cases, adds notes and resolves those whose outcome is already known
    (labelled event) as fraud/legit; the rest stays open as a realistic backlog;
  * unsupervised clusters: the analyst names the cluster with the highest fraud rate;
  * graph: Louvain communities are recomputed;
  * reference lists: devices of confirmed fraud go to a device blacklist (if the project has one).
Once per run (first project, LLM is slow on CPU):
  * the assistant writes a fraud-situation report and rule recommendations; the approver approves the first
    valid proposal into shadow mode.

Stdlib only; run after post_seed.py: python3 deploy/demo/activity.py
"""

from __future__ import annotations

import json
import os
import sys
import time
import zlib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from post_seed import ANALYST, APPROVER, call, log, login  # noqa: E402

MAX_CASES = int(os.environ.get("DEMO_ACTIVITY_MAX_CASES", "40"))
TYPOLOGIES = {"carding", "account_takeover", "bank_account_takeover", "system_breach", "promo_abuse", "refund_abuse",
              "money_mule", "other"}


def load_truth() -> dict[tuple[str, str], str | None]:
    """Optional investigation outcome per (project slug, external_id) from a ground-truth JSONL (DEMO_TRUTH_FILE)."""
    path = os.environ.get("DEMO_TRUTH_FILE")
    if not path or not Path(path).exists():
        return {}
    out: dict[tuple[str, str], str | None] = {}
    with open(path) as f:
        for line in f:
            r = json.loads(line)
            out[(r["project"], r["external_id"])] = r.get("platform_typology") or r.get("fraud_type")
    return out


TRUTH = load_truth()
LLM_WAIT_S = int(os.environ.get("DEMO_ACTIVITY_LLM_WAIT_S", "1500"))


def items(page: object) -> list[dict]:
    if isinstance(page, dict):
        return list(page.get("items", []))
    return list(page or [])  # type: ignore[call-overload]


def work_cases(analyst: str, me_id: str, pid: str, slug: str) -> None:
    """Known outcomes (label feed) are resolved. With a truth file the analyst also "investigates": ~70 % of the
    remaining cases get the true outcome, the rest stays in review as a realistic backlog."""
    labels = items(call("GET", f"/projects/{pid}/labels?page_size=5000", analyst))
    known = {lab["subject_id"]: lab for lab in labels if lab.get("subject_type") == "event"}
    cases = items(call("GET", f"/projects/{pid}/cases?status=open&page_size={MAX_CASES}", analyst))
    resolved = assigned = 0
    for case in cases:
        cid, eid = case["id"], case.get("event_id")
        lab = known.get(eid) if eid else None
        outcome: tuple[str, str | None] | None = (lab["label"], lab.get("fraud_type")) if lab else None
        if outcome is None and TRUTH and eid and zlib.crc32(cid.encode()) % 10 < 7:
            ext = call("GET", f"/projects/{pid}/events/{eid}", analyst).get("event", {}).get("external_id")
            if (slug, ext) in TRUTH:
                t = TRUTH[(slug, ext)]
                outcome = ("fraud", t if t in TYPOLOGIES else "other") if t else ("legit", None)
        if outcome:
            label, ftype = outcome
            body: dict = {"label": label, "apply_to_customer": label == "fraud",
                          "notes": "Investigated: device, payment history and linked accounts reviewed."}
            if ftype:
                body["fraud_type"] = ftype
            call("POST", f"/projects/{pid}/cases/{cid}/resolve", analyst, body=body)
            resolved += 1
        else:
            call("PATCH", f"/projects/{pid}/cases/{cid}", analyst,
                 body={"status": "in_review", "assigned_to": me_id,
                       "note": "Taken for review: checking device, payment history and linked accounts."})
            assigned += 1
    log(f"{slug}: cases resolved {resolved}, in review {assigned}")


def name_clusters(analyst: str, pid: str, slug: str) -> None:
    clusters = items(call("GET", f"/projects/{pid}/ml/unsupervised/clusters", analyst))
    ranked = sorted((c for c in clusters if c.get("cluster_id", -1) >= 0 and c.get("fraud_rate") is not None),
                    key=lambda c: c["fraud_rate"], reverse=True)
    if ranked:
        top = ranked[0]
        call("PATCH", f"/projects/{pid}/ml/unsupervised/clusters/{top['model_id']}/{top['cluster_id']}", analyst,
             body={"label": "high-risk pattern", "notes": f"Fraud rate {top['fraud_rate']:.1%}; watch closely."})
        log(f"{slug}: cluster {top['cluster_id']} named (fraud rate {top['fraud_rate']:.1%})")


def blacklist_devices(analyst: str, pid: str, slug: str) -> None:
    lists = items(call("GET", f"/projects/{pid}/reference-lists", analyst))
    target = next((x for x in lists if x.get("list_type") == "blacklist" and x.get("key_kind") == "device_id"
                   and x.get("scope") != "tenant"), None)
    if not target:
        return
    events = items(call("GET", f"/projects/{pid}/events?decision=decline&page_size=50", analyst))
    devices = sorted({e["device_id"] for e in events if e.get("device_id")})[:25]
    if devices:
        call("POST", f"/projects/{pid}/reference-lists/{target['id']}/entries", analyst,
             body={"entries": [{"key": d, "reason": "device used in declined fraud attempt"} for d in devices]})
        log(f"{slug}: {len(devices)} devices blacklisted in {target.get('name')}")


def llm_reports(analyst: str, approver: str, pid: str, slug: str) -> None:
    # one report at a time: on a CPU-only host two concurrent generations both run into the request timeout
    for kind in ("fraud-situation", "recommend-rules"):
        rid = call("POST", f"/projects/{pid}/llm/analysis/{kind}", analyst, body={})["report_id"]
        deadline, status = time.time() + LLM_WAIT_S, "running"
        while status in ("pending", "running", "queued", "processing") and time.time() < deadline:
            time.sleep(20)
            status = call("GET", f"/projects/{pid}/llm/reports/{rid}", analyst).get("status", "?")
        log(f"{slug}: LLM report {kind} → {status}")
    proposals = items(call("GET", f"/projects/{pid}/proposals?status=pending", approver))
    valid = [p for p in proposals if (p.get("validation") or {}).get("valid", True)]
    if valid:
        call("POST", f"/projects/{pid}/proposals/{valid[0]['id']}/approve", approver,
             body={"comment": "Approved into shadow for observation."})
        log(f"{slug}: proposal {valid[0]['id']} approved into shadow ({len(proposals)} pending in total)")


def main() -> int:
    analyst = login(ANALYST)
    me = call("GET", "/me", analyst)
    projects = [(p["id"], p.get("slug", p["id"])) for p in me["projects"]]
    failures = 0
    for i, (pid, slug) in enumerate(projects):
        analyst, approver = login(ANALYST), login(APPROVER)  # fresh tokens per project
        steps = [
            lambda: work_cases(analyst, me["user"]["id"], pid, slug),
            lambda: name_clusters(analyst, pid, slug),
            lambda: call("POST", f"/projects/{pid}/ml/graph-communities/recompute", analyst, body={}),
            lambda: blacklist_devices(analyst, pid, slug),
        ]
        if i == 0 and not os.environ.get("DEMO_ACTIVITY_SKIP_LLM"):
            steps.append(lambda: llm_reports(analyst, approver, pid, slug))
        for step in steps:
            try:
                step()
            except Exception as exc:  # one failing feature must not stop the rest of the simulation
                failures += 1
                log(f"{slug}: activity step failed: {exc}")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
