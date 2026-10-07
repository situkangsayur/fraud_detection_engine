"""CLI: `python -m simulator [run|export|stats]` (env vars: see README / docker-compose `simulator`)."""

from __future__ import annotations

import argparse
import json
import os
import sys
from datetime import UTC, datetime
from pathlib import Path

from simulator.api import Gateway
from simulator.export import export
from simulator.run import Settings, run, summary
from simulator.scenarios import PROJECTS, Dataset, generate_all


def _parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="fraud-simulator", description=__doc__)
    p.add_argument("command", nargs="?", default="run", choices=["run", "export", "stats"])
    p.add_argument("--customers", type=int, default=int(os.environ.get("SIM_CUSTOMERS", "2000")))
    p.add_argument("--days", type=int, default=int(os.environ.get("SIM_DAYS", "60")))
    p.add_argument("--seed", type=int, default=int(os.environ.get("SIM_SEED", "42")))
    p.add_argument(
        "--end",
        default=os.environ.get("SIM_END"),
        help="window end (ISO date, default: now) — fix it for reproducible exports",
    )
    p.add_argument(
        "--project",
        action="append",
        choices=[s.slug for s in PROJECTS],
        help="limit to project(s); repeatable",
    )
    p.add_argument("--force", action="store_true", help="re-send even if data/labels already exist")
    p.add_argument(
        "--run-tag",
        default=os.environ.get("SIM_RUN_TAG", ""),
        help="namespace external ids (new customers/events), e.g. for an out-of-sample evaluation run",
    )
    p.add_argument("--out", type=Path, default=Path("sim_dataset.csv"), help="export: output file")
    p.add_argument("--format", choices=["csv", "jsonl"], default="csv", help="export: file format")
    p.add_argument("--delimiter", default=",", help="export: CSV delimiter (e.g. ';')")
    p.add_argument(
        "--truth-out",
        type=Path,
        default=Path(os.environ["SIM_TRUTH_OUT"]) if os.environ.get("SIM_TRUTH_OUT") else None,
        help="write full ground truth (JSONL: project, external_id, fraud_type) for offline evaluation; "
        "it is never sent to the platform",
    )
    return p


def write_truth(datasets: list[Dataset], path: Path) -> None:
    """Ground truth for every generated event (used to measure detection quality, never ingested)."""
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as f:
        for ds in datasets:
            for e in ds.events:
                f.write(
                    json.dumps(
                        {
                            "project": ds.project.slug,
                            "external_id": e.record["no_ref"],
                            "fraud_type": e.fraud_type,
                        }
                    )
                    + "\n"
                )


def main(argv: list[str] | None = None) -> int:
    a = _parser().parse_args(argv)
    end = (
        datetime.fromisoformat(a.end).replace(tzinfo=UTC)
        if a.end
        else datetime.now(UTC).replace(microsecond=0)
    )
    datasets = generate_all(a.customers, a.days, a.seed, end, a.project, a.run_tag)
    if a.truth_out:
        write_truth(datasets, a.truth_out)
    if a.command == "stats":
        for ds in datasets:
            fraud = sum(ds.typology_counts().values())
            print(
                f"{ds.project.slug}: events={len(ds.events)} customers={len(ds.customers)} "
                f"fraud={fraud} {dict(ds.typology_counts())}"
            )
        return 0
    if a.command == "export":
        for ds in datasets:
            out = (
                a.out
                if len(datasets) == 1
                else a.out.with_name(f"{a.out.stem}_{ds.project.slug}{a.out.suffix}")
            )
            n = export(ds, out, a.format, a.delimiter)
            print(f"wrote {n} records → {out}")
        return 0
    s = Settings.from_env()
    s.seed, s.force = a.seed, a.force
    if not s.admin_password:
        print("ADMIN_PASSWORD is required", file=sys.stderr)
        return 2
    reports = run(Gateway(s.gateway_url), datasets, s)
    print(summary(reports))
    # a project whose ingest broke off is only partly seeded: fail so callers (the demo reset) can start over
    failed = [r for r in reports if any(x.startswith("ingest failed") for x in r.skipped)]
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
