#!/usr/bin/env python3
"""After the simulator seed: bring every demo project to a "fully set up" state.

* uploads the PDFs in data_regulations/ to the tenant library and attaches them to every project;
* trains a supervised and an unsupervised model per project (as the analyst), submits them, and approves them
  (as the approver), so every engine is live.

Stdlib only, so it runs on the host: python3 deploy/demo/post_seed.py
Env: GATEWAY_URL (default http://localhost:8080), ADMIN_PASSWORD, DEMO_USER_PASSWORD, SIM_TENANT (default demo).
"""

from __future__ import annotations

import json
import os
import sys
import time
import urllib.error
import urllib.request
import uuid
from pathlib import Path

GW = os.environ.get("GATEWAY_URL", "http://localhost:8080").rstrip("/") + "/api/v1"
ROOT = Path(__file__).resolve().parents[2]
TENANT = os.environ.get("SIM_TENANT", "demo")
TENANT_ADMIN = (
    os.environ.get("SIM_TENANT_ADMIN_EMAIL", f"sim-admin@{TENANT}.local"),
    os.environ.get("SIM_TENANT_ADMIN_PASSWORD") or os.environ["ADMIN_PASSWORD"],
)
ANALYST = (f"analyst@{TENANT}.local", os.environ["DEMO_USER_PASSWORD"])
APPROVER = (f"approver@{TENANT}.local", os.environ["DEMO_USER_PASSWORD"])
TRAIN_TIMEOUT_S = 1800
# One supervised and one unsupervised setup per demo project (pre-payment checkout, post-payment, promo, returns).
TRAINING = {
    "supervised": {"algorithm": "mlp_backprop"},
    "unsupervised": {"anomaly_algorithm": "isolation_forest", "clustering_algorithm": "hdbscan"},
}


def log(msg: str) -> None:
    print(f"[post-seed] {msg}", flush=True)


def call(method: str, path: str, token: str | None = None, body: object = None, raw: bytes | None = None,
         content_type: str = "application/json") -> dict:
    data = raw if raw is not None else (json.dumps(body).encode() if body is not None else None)
    req = urllib.request.Request(GW + path, data=data, method=method)
    if data is not None:
        req.add_header("content-type", content_type)
    if token:
        req.add_header("authorization", f"Bearer {token}")
    try:
        with urllib.request.urlopen(req, timeout=120) as resp:
            text = resp.read().decode()
            return json.loads(text) if text else {}
    except urllib.error.HTTPError as exc:
        raise RuntimeError(f"{method} {path} → {exc.code}: {exc.read().decode()[:300]}") from exc


def login(user: tuple[str, str]) -> str:
    return call("POST", "/auth/login", body={"email": user[0], "password": user[1]})["access_token"]


def multipart(fields: dict[str, str], file_field: str, path: Path) -> tuple[bytes, str]:
    boundary = uuid.uuid4().hex
    parts = [
        f'--{boundary}\r\nContent-Disposition: form-data; name="{k}"\r\n\r\n{v}\r\n'.encode()
        for k, v in fields.items()
    ]
    parts.append(
        f'--{boundary}\r\nContent-Disposition: form-data; name="{file_field}"; filename="{path.name}"\r\n'
        f"Content-Type: application/pdf\r\n\r\n".encode() + path.read_bytes() + b"\r\n"
    )
    parts.append(f"--{boundary}--\r\n".encode())
    return b"".join(parts), f"multipart/form-data; boundary={boundary}"


def regulations(admin: str, tenant_id: str, project_ids: list[str]) -> None:
    pdfs = sorted((ROOT / "data_regulations").glob("*.pdf"))
    ids = []
    for pdf in pdfs:
        code = pdf.stem.upper()
        body, ctype = multipart(
            {"code": code, "title": pdf.stem, "doc_type": "regulation", "issuer": "OJK"}, "file", pdf
        )
        rid = call("POST", f"/tenants/{tenant_id}/regulations", admin, raw=body, content_type=ctype)["regulation_id"]
        for _ in range(120):
            status = call("GET", f"/tenants/{tenant_id}/regulations/{rid}", admin).get("status")
            if status != "processing":
                break
            time.sleep(5)
        log(f"regulation {pdf.name}: {status}")
        if status == "indexed":
            ids.append(rid)
    for pid in project_ids:
        call("PUT", f"/projects/{pid}/llm/regulations", admin, body={"regulation_ids": ids})
    log(f"attached {len(ids)} regulation(s) to {len(project_ids)} project(s)")


def train_and_activate(analyst: str, approver: str, pid: str, slug: str, kind: str) -> None:
    model_id = call("POST", f"/projects/{pid}/ml/{kind}/train", analyst, body=TRAINING[kind])["model_id"]
    deadline = time.time() + TRAIN_TIMEOUT_S
    status = "training"
    while status == "training" and time.time() < deadline:
        time.sleep(10)
        status = call("GET", f"/projects/{pid}/ml/models/{model_id}", analyst).get("status", "?")
    if status != "ready":
        log(f"{slug} {kind}: training ended as {status!r}, skipped")
        return
    call("POST", f"/projects/{pid}/ml/models/{model_id}/submit", analyst, body={})
    call("POST", f"/projects/{pid}/ml/models/{model_id}/approve", approver, body={})
    log(f"{slug} {kind}: model {model_id} ({TRAINING[kind]}) active")


def ensure_team(admin: str, tenant_id: str, project_ids: list[str]) -> None:
    """Analyst and approver accounts with project membership. The demo tenant gets them from core-api's bootstrap;
    research tenants created by the simulator only have a tenant admin."""
    for role, (email, password) in (("analyst", ANALYST), ("approver", APPROVER)):
        try:
            login((email, password))
            continue
        except RuntimeError:
            pass
        user = call("POST", f"/tenants/{tenant_id}/users", admin, body={
            "email": email, "full_name": f"{TENANT} {role}".title(), "password": password, "tenant_role": "member"})
        for pid in project_ids:
            call("POST", f"/projects/{pid}/members", admin, body={"user_id": user["id"], "role": role})
        log(f"created {email} ({role}) in {len(project_ids)} project(s)")


def main() -> int:
    admin = login(TENANT_ADMIN)
    me = call("GET", "/me", admin)
    tenant_id = me["user"]["tenant_id"]
    projects = [(p["id"], p.get("slug", p["id"])) for p in me["projects"]]
    log(f"tenant {tenant_id}: {len(projects)} project(s)")
    ensure_team(admin, tenant_id, [pid for pid, _ in projects])
    analyst, approver = login(ANALYST), login(APPROVER)
    failures = 0
    try:
        regulations(admin, tenant_id, [pid for pid, _ in projects])
    except Exception as exc:  # LLM/OpenSearch problems must not block the ML setup
        failures += 1
        log(f"regulations failed: {exc}")
    for pid, slug in projects:
        analyst, approver = login(ANALYST), login(APPROVER)  # fresh tokens: all trainings outlast one token TTL
        for kind in TRAINING:
            try:
                train_and_activate(analyst, approver, pid, slug, kind)
            except Exception as exc:
                failures += 1
                log(f"{slug} {kind} failed: {exc}")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
