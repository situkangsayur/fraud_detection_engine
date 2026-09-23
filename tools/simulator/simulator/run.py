"""Orchestration via the public gateway API: tenancy → projects → source + mapping → ingest → labels."""

from __future__ import annotations

import os
from collections import Counter
from dataclasses import dataclass, field
from typing import Any

from simulator.api import ApiError, Gateway
from simulator.labels import plan_labels
from simulator.scenarios import Dataset
from simulator.shape import mapping

SOURCE_SLUG = "sim-webhook"
BATCH = 500


@dataclass
class ProjectReport:
    slug: str
    events: int = 0
    fraud_events: Counter[str] = field(default_factory=Counter)
    sent: dict[str, int] = field(default_factory=lambda: {"load_only": 0, "score": 0})
    accepted: int = 0
    rejected: int = 0
    decisions: Counter[str] = field(default_factory=Counter)
    event_labels: int = 0
    customer_labels: int = 0
    skipped: list[str] = field(default_factory=list)


@dataclass
class Settings:
    gateway_url: str
    admin_email: str
    admin_password: str
    tenant: str
    tenant_admin_email: str
    tenant_admin_password: str
    seed: int
    load_only_share: float = 0.7
    force: bool = False

    @classmethod
    def from_env(cls) -> Settings:
        tenant = os.environ.get("SIM_TENANT", "demo")
        admin_pw = os.environ.get("ADMIN_PASSWORD", "")
        return cls(
            gateway_url=os.environ.get("GATEWAY_URL", "http://localhost:8080"),
            admin_email=os.environ.get("ADMIN_EMAIL", "admin@fraud.local"),
            admin_password=admin_pw,
            tenant=tenant,
            tenant_admin_email=os.environ.get("SIM_TENANT_ADMIN_EMAIL", f"sim-admin@{tenant}.local"),
            tenant_admin_password=os.environ.get("SIM_TENANT_ADMIN_PASSWORD", admin_pw),
            seed=int(os.environ.get("SIM_SEED", "42")),
            # Share of the time window sent as load_only (history); the rest is scored. 0 → score everything.
            load_only_share=float(os.environ.get("SIM_LOAD_ONLY_SHARE", "0.7")),
        )


def setup_tenant(gw: Gateway, s: Settings) -> None:
    """Get a tenant-admin session for SIM_TENANT.

    1. If SIM_TENANT_ADMIN_EMAIL/PASSWORD already work → use them (e.g. an existing seeded tenant).
    2. Otherwise the platform admin creates the tenant (with that tenant admin) or, if the tenant exists,
       (re)provisions the tenant-admin user. Platform admins never touch project data themselves
       (multi-tenancy.md §5) — everything after this runs as the tenant admin.
    """
    try:
        gw.login(s.tenant_admin_email, s.tenant_admin_password)
        return
    except ApiError as e:
        if e.status not in (400, 401, 403, 404):
            raise
    gw.login(s.admin_email, s.admin_password)
    admin = {
        "email": s.tenant_admin_email,
        "full_name": "Simulator Tenant Admin",
        "password": s.tenant_admin_password,
    }
    t = gw.find_tenant(s.tenant)
    if t is None:
        gw.create_tenant(s.tenant, s.tenant.title(), admin)
    else:
        gw.ensure_tenant_admin(str(t["id"]), s.tenant_admin_email, s.tenant_admin_password)
    gw.login(s.tenant_admin_email, s.tenant_admin_password)


def setup_project(gw: Gateway, ds: Dataset) -> tuple[str, str, str]:
    """Returns (project_id, source_slug, api_key). Idempotent."""
    spec = ds.project
    p = gw.find_project(spec.slug) or gw.create_project(
        {
            "slug": spec.slug,
            "name": spec.name,
            "description": spec.description,
            "stage": spec.stage,
            "business_context": spec.business_context,
            "timezone": "Asia/Jakarta",
            "currency": "IDR",
            "template": spec.stage,
        }
    )
    pid = str(p["id"])
    src = gw.find_source(pid, SOURCE_SLUG)
    if src is None:
        src = gw.create_source(
            pid,
            {
                "slug": SOURCE_SLUG,
                "name": "Simulator webhook (custom shape)",
                "kind": "webhook",
                "default_event_type": "transaction",
                "mode": "score",
                "description": "Indonesian nested record shape produced by tools/simulator",
            },
        )
        api_key = str(src["api_key"])
    else:
        api_key = gw.rotate_key(pid, str(src["id"]))  # keys are shown once; rotate to reuse the source
    sid = str(src["id"])
    wanted = mapping()
    active = [m for m in gw.list_mappings(pid, sid) if m.get("status") == "active"]
    if not active or active[0].get("mapping") != wanted:
        created = gw.create_mapping(pid, sid, wanted)
        gw.activate_mapping(pid, sid, int(created["version"]))
    return pid, SOURCE_SLUG, api_key


def ingest(
    gw: Gateway,
    ds: Dataset,
    pid: str,
    slug: str,
    api_key: str,
    s: Settings,
    rep: ProjectReport,
    source_id: str | None = None,
) -> dict[str, str]:
    """Send events chronologically. First `load_only_share` of the time window as load_only (history for
    features/training), the rest scored. Returns external_id → event_id when core-api reports it."""
    ids: dict[str, str] = {}
    if not ds.events:
        return ids
    t0, t1 = ds.events[0].ts, ds.events[-1].ts
    cutoff = t0 + (t1 - t0) * s.load_only_share

    def flush(batch: list[dict[str, Any]], mode: str) -> None:
        if not batch:
            return
        res = gw.ingest_batch(slug, api_key, batch, mode)
        rep.sent[mode] += len(batch)
        rep.accepted += int(res.get("accepted", 0))
        rep.rejected += int(res.get("rejected", 0))
        for d in res.get("decisions", []) or []:
            if d.get("decision"):
                rep.decisions[str(d["decision"])] += 1
            if d.get("event_id") and d.get("external_id"):
                ids[str(d["external_id"])] = str(d["event_id"])

    batch: list[dict[str, Any]] = []
    mode = "load_only"
    for e in ds.events:
        m = "load_only" if e.ts <= cutoff else "score"
        if m != mode or len(batch) >= BATCH:
            flush(batch, mode)
            batch = []
            mode = m
        batch.append(e.record)
    flush(batch, mode)
    return ids


def post_labels(
    gw: Gateway, ds: Dataset, pid: str, ids: dict[str, str], s: Settings, rep: ProjectReport
) -> None:
    ev_labels, cust_labels = plan_labels(ds, s.seed)
    for lab in ev_labels:
        eid = ids.get(lab.external_id) or gw.find_event_id(pid, lab.external_id)
        if not eid:
            continue
        body: dict[str, Any] = {
            "subject_type": "event",
            "subject_id": eid,
            "label": lab.label,
            "source": lab.source,
            "notes": "tools/simulator ground truth",
        }
        if lab.fraud_type:
            body["fraud_type"] = lab.fraud_type
        gw.post_label(pid, body)
        rep.event_labels += 1
    for cl in cust_labels:
        cid = gw.find_customer_id(pid, cl.external_id)
        if not cid:
            continue
        gw.post_label(
            pid,
            {
                "subject_type": "customer",
                "subject_id": cid,
                "label": "fraud",
                "fraud_type": cl.fraud_type,
                "source": "analyst",
                "notes": "tools/simulator: confirmed fraud ring member",
            },
        )
        rep.customer_labels += 1


def run(gw: Gateway, datasets: list[Dataset], s: Settings) -> list[ProjectReport]:
    setup_tenant(gw, s)
    reports = []
    for ds in datasets:
        rep = ProjectReport(ds.project.slug, len(ds.events), ds.typology_counts())
        pid, slug, key = setup_project(gw, ds)
        src = gw.find_source(pid, slug)
        existing = gw.count_events(pid, str(src["id"])) if src else 0
        ids: dict[str, str] = {}
        if existing and not s.force:
            rep.skipped.append(f"ingest ({existing} events already present)")
        else:
            try:
                ids = ingest(gw, ds, pid, slug, key, s, rep)
            except ApiError as e:
                rep.skipped.append(f"ingest failed: {e}")
        if gw.count_labels(pid) and not s.force:
            rep.skipped.append("labels (already present)")
        else:
            post_labels(gw, ds, pid, ids, s, rep)
        reports.append(rep)
    return reports


def summary(reports: list[ProjectReport]) -> str:
    lines = ["", "Simulation summary", "=" * 60]
    for r in reports:
        fraud = sum(r.fraud_events.values())
        lines += [
            f"[{r.slug}] events={r.events} fraud_events={fraud} ({fraud / max(1, r.events):.1%})",
            f"   typologies: {dict(r.fraud_events)}",
            f"   sent: {r.sent}  accepted={r.accepted} rejected={r.rejected}",
            f"   decisions (scored part): {dict(r.decisions)}",
            f"   labels: events={r.event_labels} customers={r.customer_labels}",
        ]
        if r.skipped:
            lines.append(f"   skipped: {'; '.join(r.skipped)}")
    return "\n".join(lines)
