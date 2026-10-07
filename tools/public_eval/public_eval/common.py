"""Shared plumbing for the public-dataset evaluation: environment, gateway sessions, tenant/project setup,
ingest, delayed partial labels, model training with maker–checker, rulesets, and DB access for evaluation.

Only the eval instance is used (gateway + Postgres published on 127.0.0.1 by ~/fraud-eval/.env.eval).
"""

from __future__ import annotations

import json
import os
import random
import secrets
import sys
import time
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any, Iterable

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "simulator"))
from simulator.api import ApiError, Gateway  # noqa: E402

ENV_FILE = Path(os.environ.get("EVAL_ENV_FILE", str(Path.home() / "fraud-eval" / ".env.eval")))
STATE_FILE = Path(os.environ.get("EVAL_STATE_FILE", str(Path.home() / "fraud-eval" / "state.json")))
WORK_DIR = Path(os.environ.get("EVAL_WORK_DIR", str(Path.home() / "fraud-eval" / "work")))
RAW_DIR = Path(os.environ.get("EVAL_RAW_DIR", str(Path.home() / "datasets" / "fraud-public")))
TENANT = "public-eval"
SOURCE_SLUG = "public-data"
BATCH = {"load_only": 100, "score": 25}


def log(msg: str) -> None:
    print(f"[{datetime.now().strftime('%H:%M:%S')}] {msg}", flush=True)


def env() -> dict[str, str]:
    out: dict[str, str] = {}
    for line in ENV_FILE.read_text().splitlines():
        line = line.strip()
        if line and not line.startswith("#") and "=" in line:
            k, v = line.split("=", 1)
            out[k.strip()] = v.strip().strip('"').strip("'")
    return out


def gateway_url() -> str:
    return f"http://127.0.0.1:{env().get('GATEWAY_HOST_PORT', '18090')}"


def load_state() -> dict[str, Any]:
    return json.loads(STATE_FILE.read_text()) if STATE_FILE.exists() else {}


def save_state(st: dict[str, Any]) -> None:
    STATE_FILE.write_text(json.dumps(st, indent=2))
    STATE_FILE.chmod(0o600)


# ---------------------------------------------------------------------------------------------- database
def db():
    import psycopg2

    e = env()
    return psycopg2.connect(
        host="127.0.0.1",
        port=int(e.get("POSTGRES_HOST_PORT", "5437")),
        user=e["POSTGRES_SUPERUSER"],
        password=e["POSTGRES_SUPERUSER_PASSWORD"],
        dbname=e["POSTGRES_DB"],
    )


def query(sql: str, params: Iterable[Any] = ()) -> list[tuple]:
    with db() as conn, conn.cursor() as cur:
        cur.execute(sql, tuple(params))
        return cur.fetchall() if cur.description else []


# ---------------------------------------------------------------------------------------------- sessions
@dataclass
class Sessions:
    admin: Gateway  # tenant admin
    analyst: Gateway
    approver: Gateway
    tenant_id: str
    user_ids: dict[str, str]


def _user(gw: Gateway, tid: str, email: str, name: str, password: str) -> str:
    body = {"email": email, "full_name": name, "password": password, "tenant_role": "member"}
    try:
        u = gw.request("POST", f"/api/v1/tenants/{tid}/users", json=body)
        return str(u["id"])
    except ApiError as e:
        if e.status != 409:
            raise
    for u in Gateway.items(gw.request("GET", f"/api/v1/tenants/{tid}/users", params={"page_size": 200})):
        if u.get("email") == email:
            gw.request("PATCH", f"/api/v1/tenants/{tid}/users/{u['id']}", json={"password": password, "is_active": True})
            return str(u["id"])
    raise RuntimeError(f"user {email} not found after 409")


def sessions() -> Sessions:
    e, st = env(), load_state()
    pw = st.setdefault("passwords", {})
    for k in ("tenant_admin", "analyst", "approver"):
        pw.setdefault(k, secrets.token_urlsafe(18) + "Aa1!")
    save_state(st)
    emails = {
        "tenant_admin": "tenant-admin@public-eval.local",
        "analyst": "analyst@public-eval.local",
        "approver": "approver@public-eval.local",
    }
    url = gateway_url()
    admin = Gateway(url)
    try:
        admin.login(emails["tenant_admin"], pw["tenant_admin"])
    except ApiError:
        plat = Gateway(url)
        plat.login(e["ADMIN_EMAIL"], e["ADMIN_PASSWORD"])
        t = plat.find_tenant(TENANT)
        if t is None:
            plat.create_tenant(TENANT, "Public dataset evaluation",
                               {"email": emails["tenant_admin"], "full_name": "Eval Tenant Admin", "password": pw["tenant_admin"]})
        else:
            plat.ensure_tenant_admin(str(t["id"]), emails["tenant_admin"], pw["tenant_admin"])
        admin.login(emails["tenant_admin"], pw["tenant_admin"])
    me = admin.request("GET", "/api/v1/me")
    tid = str(me.get("tenant_id") or me.get("tid") or (me.get("tenant") or {}).get("id"))
    ids = {
        "analyst": _user(admin, tid, emails["analyst"], "Eval Analyst", pw["analyst"]),
        "approver": _user(admin, tid, emails["approver"], "Eval Approver", pw["approver"]),
    }
    analyst, approver = Gateway(url), Gateway(url)
    analyst.login(emails["analyst"], pw["analyst"])
    approver.login(emails["approver"], pw["approver"])
    return Sessions(admin, analyst, approver, tid, ids)


def relogin(gw: Gateway) -> None:
    """Project roles live in the JWT claims: refresh tokens after membership changes."""
    if gw._credentials:
        gw.login(*gw._credentials)


# ---------------------------------------------------------------------------------------------- projects
def setup_project(s: Sessions, slug: str, name: str, stage: str, template: str, mapping: dict[str, Any],
                  description: str = "", ml_config: dict[str, Any] | None = None,
                  graph_config: dict[str, Any] | None = None) -> tuple[str, str]:
    """Create (idempotently) a project, members, data source and active mapping. Returns (pid, api_key)."""
    gw = s.admin
    p = gw.find_project(slug)
    if p is None:
        body: dict[str, Any] = {
            "slug": slug, "name": name, "description": description, "stage": stage,
            "business_context": description, "timezone": "UTC", "currency": "USD", "template": template,
        }
        if ml_config:
            body["ml_config"] = ml_config
        if graph_config:
            body["graph_config"] = graph_config
        p = gw.create_project(body)
    pid = str(p["id"])
    for role, uid in (("analyst", s.user_ids["analyst"]), ("approver", s.user_ids["approver"])):
        gw.request("POST", f"/api/v1/projects/{pid}/members", json={"user_id": uid, "role": role})
    relogin(s.analyst)
    relogin(s.approver)
    src = gw.find_source(pid, SOURCE_SLUG)
    if src is None:
        src = gw.create_source(pid, {"slug": SOURCE_SLUG, "name": "Public dataset loader", "kind": "webhook",
                                     "default_event_type": "transaction", "mode": "score",
                                     "description": "tools/public_eval"})
        key = str(src["api_key"])
    else:
        key = gw.rotate_key(pid, str(src["id"]))
    sid = str(src["id"])
    active = [m for m in gw.list_mappings(pid, sid) if m.get("status") == "active"]
    if not active or active[0].get("mapping") != mapping:
        created = gw.create_mapping(pid, sid, mapping)
        gw.activate_mapping(pid, sid, int(created["version"]))
    return pid, key


def set_setting(s: Sessions, pid: str, key: str, value: Any) -> None:
    s.admin.request("PUT", f"/api/v1/projects/{pid}/settings/{key}", json={"value": value})


# ---------------------------------------------------------------------------------------------- ingest
def ingest(s: Sessions, key: str, records: list[dict[str, Any]], mode: str, label: str = "") -> dict[str, int]:
    """Send records in order with the given mode. Returns counters."""
    gw = s.admin
    n, acc, rej, t0 = 0, 0, 0, time.time()
    size = BATCH[mode]
    for i in range(0, len(records), size):
        chunk = records[i:i + size]
        for attempt in range(8):
            try:
                res = gw.ingest_batch(SOURCE_SLUG, key, chunk, mode)
                break
            except ApiError as e:  # 408 under heavy parallel load: retry (re-sent duplicates are rejected)
                if e.status != 408 or attempt == 7:
                    raise
                time.sleep(5 * (attempt + 1))
        n += len(chunk)
        acc += int(res.get("accepted", 0))
        rej += int(res.get("rejected", 0))
        if rej and not getattr(ingest, "_shown", False):
            errs = [r for r in res.get("results", res.get("errors", [])) or [] if isinstance(r, dict) and r.get("error")]
            log(f"  rejected example: {json.dumps(errs[:2])[:600] if errs else json.dumps(res)[:600]}")
            ingest._shown = True  # type: ignore[attr-defined]
        if (i // size) % max(1, 2000 // size) == 0:
            rate = n / max(1e-6, time.time() - t0)
            log(f"  {label} {mode}: {n}/{len(records)} sent, {acc} accepted, {rej} rejected, {rate:.0f}/s")
    return {"sent": n, "accepted": acc, "rejected": rej, "seconds": int(time.time() - t0)}


def event_ids(pid: str) -> dict[str, str]:
    return {ext: str(eid) for ext, eid in query(
        "SELECT external_id, id FROM core.events WHERE project_id = %s", (pid,))}


def customer_ids(pid: str) -> dict[str, str]:
    return {ext: str(cid) for ext, cid in query(
        "SELECT external_id, id FROM core.customers WHERE project_id = %s", (pid,))}


def post_labels(s: Sessions, pid: str, history: list[dict[str, Any]], truth: dict[str, dict[str, Any]],
                seed: int, ts_key: str, delay_days: int = 7, fraud_fraction: float = 0.6,
                legit_fraction: float = 0.03, customer_key: str | None = None,
                customer_fraction: float = 0.7) -> dict[str, int]:
    """Delayed, partial labels as in tools/simulator/labels.py: fraud older than `delay_days` (relative to the
    newest history event) is labelled with probability 0.6, legit with 0.03; optionally customers of labelled
    fraud are marked fraud (feeds graph proximity)."""
    rng = random.Random(f"{seed}:{pid}:labels")
    if not history:
        return {"event_labels": 0, "customer_labels": 0}
    newest = datetime.fromisoformat(max(r[ts_key] for r in history).replace("Z", "+00:00"))
    oldest = datetime.fromisoformat(min(r[ts_key] for r in history).replace("Z", "+00:00"))
    delay = min(timedelta(days=delay_days), (newest - oldest) * 0.2)  # short datasets (AMLworld) keep labels
    cutoff = (newest - delay).strftime("%Y-%m-%dT%H:%M:%SZ")
    eids = event_ids(pid)
    n_ev, fraud_customers = 0, set()
    for r in history:
        ext = r["ext_id"]
        if r[ts_key] > cutoff or ext not in eids:
            continue
        t = truth[ext]
        if t["is_fraud"]:
            if rng.random() >= fraud_fraction:
                continue
            body = {"subject_type": "event", "subject_id": eids[ext], "label": "fraud",
                    "fraud_type": t["typology"], "source": "chargeback", "notes": "public_eval delayed label"}
            if customer_key:
                fraud_customers.add(str(r[customer_key]))
        else:
            if rng.random() >= legit_fraction:
                continue
            body = {"subject_type": "event", "subject_id": eids[ext], "label": "legit", "source": "analyst",
                    "notes": "public_eval sampled legit"}
        s.analyst.post_label(pid, body)
        n_ev += 1
    n_cu = 0
    if customer_key and fraud_customers:
        cids = customer_ids(pid)
        for c in sorted(fraud_customers):
            if c in cids and rng.random() < customer_fraction:
                s.analyst.post_label(pid, {"subject_type": "customer", "subject_id": cids[c], "label": "fraud",
                                           "fraud_type": "other", "source": "analyst",
                                           "notes": "public_eval confirmed fraud customer"})
                n_cu += 1
    return {"event_labels": n_ev, "customer_labels": n_cu}


# ---------------------------------------------------------------------------------------------- models
def _wait_model(gw: Gateway, pid: str, mid: str, timeout: int = 7200) -> dict[str, Any]:
    t0 = time.time()
    while time.time() - t0 < timeout:
        m = gw.request("GET", f"/api/v1/projects/{pid}/ml/models/{mid}")
        if m.get("status") in ("ready", "failed", "active"):
            return dict(m)
        time.sleep(10)
    raise TimeoutError(f"model {mid} still training after {timeout}s")


def train_and_approve(s: Sessions, pid: str, since_days: int = 730) -> dict[str, Any]:
    out: dict[str, Any] = {}
    for kind, body in (("supervised", {"since_days": since_days}),
                       ("unsupervised", {"since_days": since_days})):
        r = s.analyst.request("POST", f"/api/v1/projects/{pid}/ml/{kind}/train", json=body)
        mid = str(r["model_id"])
        m = _wait_model(s.analyst, pid, mid)
        if m.get("status") == "failed":
            out[kind] = {"model_id": mid, "status": "failed", "error": m.get("error")}
            log(f"  {kind} training failed: {str(m.get('error'))[:300]}")
            continue
        s.analyst.request("POST", f"/api/v1/projects/{pid}/ml/models/{mid}/submit")
        s.approver.request("POST", f"/api/v1/projects/{pid}/ml/models/{mid}/approve")
        out[kind] = {"model_id": mid, "status": "active", "metrics": m.get("metrics")}
        log(f"  {kind} model active: {json.dumps(m.get('metrics'))[:300]}")
    return out


# ---------------------------------------------------------------------------------------------- rules
def retire_template_rulesets(s: Sessions, pid: str) -> int:
    n = 0
    for rs in Gateway.items(s.admin.request("GET", f"/api/v1/projects/{pid}/rulesets", params={"page_size": 200})):
        try:
            s.approver.request("POST", f"/api/v1/projects/{pid}/rulesets/{rs['id']}/retire", json={})
            n += 1
        except ApiError as e:
            log(f"  retire {rs.get('code')}: {e}")
    return n


def load_ruleset(s: Sessions, pid: str, ruleset: dict[str, Any]) -> dict[str, Any]:
    """Create rules (analyst), submit, approve active (approver), create a ruleset with them, approve it.
    `ruleset` = {code, name, description, event_types, typologies, aggregation, max_score, rules:[envelope+weight]}"""
    created = []
    for env_ in ruleset["rules"]:
        weight = env_.pop("weight", 1.0)
        try:
            v = s.analyst.request("POST", f"/api/v1/projects/{pid}/rules/validate", json=env_)
        except ApiError as e:
            v = {"valid": False, "errors": str(e)[:500]}
        if not v.get("valid"):
            log(f"  rule {env_['code']} invalid: {v.get('errors')}")
            continue
        r = s.analyst.request("POST", f"/api/v1/projects/{pid}/rules", json=env_)
        rid = str(r["id"])
        s.analyst.request("POST", f"/api/v1/projects/{pid}/rules/{rid}/submit", json={})
        s.approver.request("POST", f"/api/v1/projects/{pid}/rules/{rid}/approve", json={"target_status": "active"})
        created.append({"rule_id": rid, "weight": weight, "code": env_["code"]})
    body = {k: ruleset[k] for k in ("code", "name", "description", "event_types", "typologies", "aggregation", "max_score")}
    rs = s.analyst.request("POST", f"/api/v1/projects/{pid}/rulesets", json=body)
    rsid = str(rs["id"])
    s.analyst.request("PUT", f"/api/v1/projects/{pid}/rulesets/{rsid}/rules",
                      json=[{"rule_id": c["rule_id"], "weight": c["weight"]} for c in created])
    s.analyst.request("POST", f"/api/v1/projects/{pid}/rulesets/{rsid}/submit", json={})
    s.approver.request("POST", f"/api/v1/projects/{pid}/rulesets/{rsid}/approve", json={"target_status": "active"})
    return {"ruleset_id": rsid, "rules": [c["code"] for c in created]}


# ---------------------------------------------------------------------------------------------- time
def shift_to_now(ts: list[datetime], end_offset_hours: float = 1.0) -> timedelta:
    """Offset that moves the newest timestamp to (now − end_offset_hours), keeping relative spacing."""
    newest = max(ts)
    target = datetime.now(timezone.utc) - timedelta(hours=end_offset_hours)
    return target - newest


def iso(dt: datetime) -> str:
    return dt.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
