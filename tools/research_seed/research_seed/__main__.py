"""Research seeding mode: one public dataset → one tenant, with the demo's protocol.

    uv run python -m research_seed --dataset sparkov            # tenant exp-sparkov
    uv run python -m research_seed --dataset paysim --target 60000 --seed 42

Steps (same as deploy/demo/reset-demo.sh, so results are comparable with the synthetic demo):
  1. load + sample the dataset (deterministic), shift its timeline to end now;
  2. history phase: first 70 % as load_only + delayed labels (60 % of fraud, 3 % of legit, 7-day delay);
  3. deploy/demo/post_seed.py: analyst/approver accounts, regulations, train + approve mlp_backprop and
     isolation_forest + hdbscan;
  4. online phase: last 30 % scored with every engine;
  5. deploy/demo/activity.py (cases, clusters, communities, blacklist; no LLM);
  6. ground truth JSONL + tools/research_export → <out>/<tenant>/.

Env: GATEWAY_URL (default http://localhost:8080), ADMIN_EMAIL, ADMIN_PASSWORD (platform admin), DEMO_USER_PASSWORD
(password of every seeded account). Datasets are read from --data-dir/<dataset>/ (downloaded beforehand).
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from datetime import UTC, datetime
from pathlib import Path

from simulator.api import Gateway
from simulator.run import Settings, run, summary

from research_seed.loaders import SOURCES

ROOT = Path(__file__).resolve().parents[3]
TENANT_NAMES = {
    "sparkov": "Experiment — Sparkov (card fraud)",
    "paysim": "Experiment — PaySim (mobile money)",
    "saml-d": "Experiment — SAML-D (money laundering)",
}


def _parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog="research-seed", description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    p.add_argument("--dataset", required=True, choices=sorted(SOURCES))
    p.add_argument("--tenant", help="tenant slug (default exp-<dataset>)")
    p.add_argument("--target", type=int, default=60_000, help="approximate number of events to sample")
    p.add_argument("--seed", type=int, default=42)
    p.add_argument("--data-dir", type=Path, default=Path.home() / "datasets" / "fraud-public")
    p.add_argument("--out", type=Path, default=Path.home() / "datasets" / "fraud-research")
    p.add_argument("--skip-export", action="store_true")
    return p


def _env(tenant: str) -> dict[str, str]:
    pw = os.environ["DEMO_USER_PASSWORD"]
    return {
        **os.environ,
        "SIM_TENANT": tenant,
        "SIM_TENANT_ADMIN_EMAIL": f"sim-admin@{tenant}.local",
        "SIM_TENANT_ADMIN_PASSWORD": pw,
        "DEMO_ACTIVITY_SKIP_LLM": "1",
    }


def _step(name: str, cmd: list[str], env: dict[str, str], *, required: bool = True) -> None:
    print(f"[research-seed] {name}", flush=True)
    rc = subprocess.run(cmd, check=required, env=env, cwd=ROOT).returncode
    if rc:
        print(f"[research-seed] {name} had errors (continuing)", flush=True)


def main(argv: list[str] | None = None) -> int:
    a = _parser().parse_args(argv)
    src = SOURCES[a.dataset]
    tenant = a.tenant or f"exp-{a.dataset}"
    end = datetime.now(UTC).replace(microsecond=0)
    ds = src.loader(a.data_dir / src.name, a.target, a.seed, end)
    print(
        f"[research-seed] {a.dataset}: {len(ds.events)} events, fraud {len(ds.truth_detail)} "
        f"({len(set(ds.truth_detail.values()))} dataset typologies)",
        flush=True,
    )

    env = _env(tenant)
    s = Settings(
        gateway_url=os.environ.get("GATEWAY_URL", "http://localhost:8080"),
        admin_email=os.environ.get("ADMIN_EMAIL", "admin@fraud.local"),
        admin_password=os.environ["ADMIN_PASSWORD"],
        tenant=tenant,
        tenant_admin_email=env["SIM_TENANT_ADMIN_EMAIL"],
        tenant_admin_password=env["SIM_TENANT_ADMIN_PASSWORD"],
        seed=a.seed,
    )
    gw = Gateway(s.gateway_url)
    s.phase = "history"
    print(summary(run(gw, [ds], s)), flush=True)
    _name_tenant(s, tenant, TENANT_NAMES.get(a.dataset))
    _step("models + regulations", [sys.executable, "deploy/demo/post_seed.py"], env)
    s.phase = "online"
    reports = run(gw, [ds], s)
    print(summary(reports), flush=True)
    if any(x.startswith("ingest failed") for r in reports for x in r.skipped):
        return 1
    _step("analyst activity", [sys.executable, "deploy/demo/activity.py"], env, required=False)

    out = (a.out / tenant).expanduser()
    out.mkdir(parents=True, exist_ok=True)
    truth = out / "ground_truth.jsonl"
    with truth.open("w") as f:
        for e in ds.events:
            ext = e.record["no_ref"]
            # fraud_type = the dataset's own fine-grained label (evaluation only); platform_typology = what was sent
            row = {
                "project": ds.project.slug,
                "external_id": ext,
                "fraud_type": ds.truth_detail.get(ext),
                "platform_typology": e.fraud_type,
            }
            f.write(json.dumps(row) + "\n")
    (out / "source.json").write_text(
        json.dumps(
            {
                "dataset": a.dataset,
                "url": src.url,
                "license": src.license,
                "sample_target": a.target,
                "seed": a.seed,
                "events": len(ds.events),
                "window_end": end.isoformat(),
                "tenant": tenant,
            },
            indent=2,
        )
    )
    if not a.skip_export:
        _step(
            "research export",
            [
                "uv",
                "run",
                "--directory",
                "tools/research_export",
                "python",
                "-m",
                "research_export",
                "--tenant",
                tenant,
                "--truth",
                str(truth),
                "--out",
                str(out),
            ],
            env,
        )
    return 0


def _name_tenant(s: Settings, slug: str, name: str | None) -> None:
    if not name:
        return
    gw = Gateway(s.gateway_url)
    gw.login(s.admin_email, s.admin_password)
    t = gw.find_tenant(slug)
    if t and t.get("name") != name:
        gw.request("PATCH", f"/api/v1/tenants/{t['id']}", json={"name": name})


if __name__ == "__main__":
    raise SystemExit(main())
