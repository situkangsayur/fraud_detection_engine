"""Export a research dataset from a running Fraud Detection Platform install.

    uv run python -m research_export --out ~/datasets/fraud-live-demo/2026-10-07 \\
        --sim-end 2026-10-07T09:19:52            # regenerate ground truth from the simulator (seed 42 default)
    # or: --truth path/to/truth.jsonl            # truth written by `simulator --truth-out`

The database is read with the Postgres superuser (`POSTGRES_SUPERUSER[_PASSWORD]` from .env, host port 5433) and never written.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import sys
from datetime import UTC, datetime
from pathlib import Path
from urllib.parse import quote

import pandas as pd
import psycopg

from research_export import metrics, queries
from research_export.readme import write_readme

ROOT = Path(__file__).resolve().parents[3]


def _env_file(path: Path) -> dict[str, str]:
    out: dict[str, str] = {}
    if path.exists():
        for line in path.read_text().splitlines():
            if "=" in line and not line.lstrip().startswith("#"):
                k, v = line.split("=", 1)
                out[k.strip()] = v.strip().strip("'\"")
    return out


def _parser() -> argparse.ArgumentParser:
    env = _env_file(ROOT / ".env")
    user = os.environ.get("POSTGRES_SUPERUSER") or env.get("POSTGRES_SUPERUSER", "postgres")
    pw = os.environ.get("POSTGRES_SUPERUSER_PASSWORD") or env.get("POSTGRES_SUPERUSER_PASSWORD", "")
    port = os.environ.get("POSTGRES_HOST_PORT") or env.get("POSTGRES_HOST_PORT", "5433")
    db = env.get("POSTGRES_DB", "fraud")
    p = argparse.ArgumentParser(prog="research-export", description=__doc__)
    p.add_argument("--dsn", default=f"postgresql://{user}:{quote(pw, safe='')}@127.0.0.1:{port}/{db}")
    p.add_argument("--tenant", default="demo", help="tenant slug to export")
    p.add_argument("--out", type=Path, required=True)
    g = p.add_mutually_exclusive_group(required=True)
    g.add_argument("--truth", type=Path, help="ground truth JSONL (project, external_id, fraud_type)")
    g.add_argument("--sim-end", help="simulator window end (UTC, ISO) used for the seed; truth is regenerated")
    p.add_argument("--sim-seed", type=int, default=42)
    p.add_argument("--sim-customers", type=int, default=2000)
    p.add_argument("--sim-days", type=int, default=60)
    p.add_argument("--no-features", action="store_true", help="skip the (large) wide feature table")
    return p


def regenerate_truth(a: argparse.Namespace, dest: Path) -> Path:
    cmd = [
        "uv", "run", "--directory", str(ROOT / "tools" / "simulator"), "python", "-m", "simulator", "stats",
        "--end", a.sim_end, "--seed", str(a.sim_seed), "--customers", str(a.sim_customers), "--days", str(a.sim_days),
        "--truth-out", str(dest),
    ]  # fmt: skip
    subprocess.run(cmd, check=True, stdout=subprocess.DEVNULL)
    return dest


def load_truth(path: Path) -> pd.DataFrame:
    t = pd.read_json(path, lines=True)
    t = t.rename(columns={"fraud_type": "truth_fraud_type"})
    return t[["project", "external_id", "truth_fraud_type"]]


def read_sql(conn: psycopg.Connection, sql: str, tenant: str) -> pd.DataFrame:
    with conn.cursor() as cur:
        cur.execute(sql, {"tenant": tenant})
        cols = [c.name for c in cur.description or []]
        return pd.DataFrame(cur.fetchall(), columns=cols)


def write_table(df: pd.DataFrame, out: Path, name: str, csv: bool = False) -> dict[str, object]:
    path = out / f"{name}.parquet"
    df.to_parquet(path, index=False)
    files = [path]
    if csv:
        cpath = out / f"{name}.csv.gz"
        df.to_csv(cpath, index=False)
        files.append(cpath)
    return {
        "table": name,
        "rows": len(df),
        "columns": list(df.columns),
        "files": [{"file": f.name, "bytes": f.stat().st_size, "sha256": _sha256(f)} for f in files],
    }


def _sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def main(argv: list[str] | None = None) -> int:
    a = _parser().parse_args(argv)
    out: Path = a.out.expanduser()
    (out / "metrics").mkdir(parents=True, exist_ok=True)

    truth_path = a.truth or regenerate_truth(a, out / "ground_truth.jsonl")
    truth = load_truth(truth_path)
    manifest: dict[str, object] = {
        "created_at": datetime.now(UTC).isoformat(timespec="seconds"),
        "tenant": a.tenant,
        "simulator": {"end": a.sim_end, "seed": a.sim_seed, "customers": a.sim_customers, "days": a.sim_days}
        if a.sim_end
        else {"truth_file": str(a.truth)},
        "tables": [],
    }
    tables: list[dict[str, object]] = []

    with psycopg.connect(a.dsn) as conn:
        conn.read_only = True
        events = read_sql(conn, queries.EVENTS, a.tenant)
        if events.empty:
            print(f"no events for tenant {a.tenant!r}", file=sys.stderr)
            return 1
        events = events.merge(truth, on=["project", "external_id"], how="left", validate="one_to_one")
        events["is_fraud_truth"] = events["truth_fraud_type"].notna()
        tables.append(write_table(events, out, "events", csv=True))
        print(
            f"events: {len(events)} rows, truth matched {events['external_id'].isin(truth['external_id']).mean():.4f}"
        )

        for name, sql in queries.TABLES.items():
            if name == "features" and a.no_features:
                continue
            df = read_sql(conn, sql, a.tenant)
            if name == "features" and not df.empty:
                wide = pd.json_normalize(df["features_json"].map(json.loads).tolist())
                wide.columns = [f"f_{c}" for c in wide.columns]
                df = pd.concat([df.drop(columns=["features_json"]), wide], axis=1)
            tables.append(write_table(df, out, name, csv=name in {"rule_hits", "cases", "rules", "models"}))
            print(f"{name}: {len(df)} rows")

    results = {
        "dataset_summary": metrics.dataset_summary(events),
        "engine_auc": metrics.engine_auc(events),
        "operating_points": metrics.operating_points(events),
        "typology_recall": metrics.typology_recall(events),
        "threshold_sweep": metrics.threshold_sweep(events),
    }
    for name, df in results.items():
        df.to_csv(out / "metrics" / f"{name}.csv", index=False)
    manifest["tables"] = tables
    (out / "manifest.json").write_text(json.dumps(manifest, indent=2, default=str))
    write_readme(out, manifest, results)
    print(f"written to {out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
