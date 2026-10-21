# Research dataset (live demo export)

> Document status: verified against commit 07ee765 on 2026-10-13

A self-describing dataset of the platform **running with every feature on**: rules, supervised and unsupervised ML,
graph, cases, labels and the LLM assistant. It is meant for analysis and for the paper (*Five Engines, One
Decision*), next to the synthetic evaluation and the result summaries in [evaluation.md](evaluation.md). The
public-dataset evaluation (`tools/public_eval`, separate `fraud-eval` instance) is **not** on this branch; it lives on
branch `eval/public-datasets` (see evaluation.md §6).

## Two seeding modes, one install

Entry point: `deploy/seed.sh` (loads `.env`).

| Command | What it does |
|---|---|
| `deploy/seed.sh demo` | runs `deploy/demo/reset-demo.sh`: wipes everything and seeds tenant `demo` (synthetic simulator, 4 projects) |
| `deploy/seed.sh research [dataset…]` | adds/refreshes one tenant per public dataset with `tools/research_seed` (no wipe): `sparkov` → `exp-sparkov`, `paysim` → `exp-paysim`, `saml-d` → `exp-saml-d` |
| `deploy/seed.sh master` | `demo` + all research tenants, then `deploy/demo/master.sh snapshot` |
| `deploy/seed.sh restore` | `deploy/demo/master.sh restore`: puts the master snapshot back and shifts every timestamp so the data ends "now" |

On the demo host, cron runs **`deploy/seed.sh restore` at 00:00**: all tenants come back from the master snapshot
(volumes `pg_data os_data ml_models regulations uploads`). A restore does **not** re-seed or export; a new dataset is
produced only when `demo`, `research` or `master` runs. Commands and the reproduction tutorial:
[experiments.md](../guides/experiments.md).

## How the demo dataset is produced

`deploy/demo/reset-demo.sh` runs:

| Step | What happens | Why it matters for analysis |
|---|---|---|
| 1. wipe + start | data volumes of compose project `fraud-platform` removed | every run starts from the same empty state |
| 2. history seed | `simulator --phase history`: first 70 % of a 60-day window (`SIM_DAYS`, 2 000 customers by default) as `load_only` events, plus delayed labels (60 % of fraud events older than 7 days, ~3 % legit) and 70 % of fraud-ring customers labelled `fraud` | history for velocity features, model training and graph "known fraudsters" |
| 3. post-seed | `deploy/demo/post_seed.py`: regulation PDFs uploaded/attached; per project `mlp_backprop` (supervised) and `isolation_forest` + `hdbscan` (unsupervised) trained, submitted (analyst) and approved (approver) | maker–checker exactly as in production |
| 4. online seed | `simulator --phase online`: last 30 % scored online, **all engines active** | engine scores are comparable on the same events |
| 5. activity | `deploy/demo/activity.py`: cases worked/resolved against the ground truth, cluster named, communities recomputed, device blacklist, LLM reports + one proposal approved into shadow | governance and assistant artefacts |
| 6. export | `tools/research_export` → `~/datasets/fraud-live-demo/<YYYY-MM-DD>/` (`DEMO_DATASET_DIR`) | the dataset |

The simulator is deterministic: **seed + window end** fully define the data and the ground truth. The reset uses
`SIM_SEED` when set (it is loaded from `.env`), otherwise the date (`YYYYMMDD`), and records seed, window end,
customers and days in `~/.local/state/fraud-demo/last-run.env` and in the dataset's `manifest.json`. Runs with
different seeds are independent replications of the same generator, which gives repeated-run confidence intervals.

## Research mode (`tools/research_seed`)

`uv run python -m research_seed --dataset <sparkov|paysim|saml-d> [--tenant] [--target 60000] [--seed 42]
[--data-dir ~/datasets/fraud-public] [--out ~/datasets/fraud-research] [--skip-export] [--truth-only]`

Same protocol as the demo: the dataset is sampled deterministically, its timeline shifted to end now (relative spacing
kept), mapped into the simulator's custom record shape (unused columns kept under `sumber.*` → `source.sumber.*`),
then history (70 %, `load_only`, delayed labels 60 % fraud / 3 % legit / 7 days) → `post_seed.py` → online (30 %) →
`activity.py` with `DEMO_ACTIVITY_SKIP_LLM=1` → `research_export --tenant exp-<dataset>`. Only **event** labels are
sent; research tenants get no customer labels.

| Dataset | Source (licence) | Sampling (default `--target` 60 000) | Platform typology sent as label | Dataset label kept as truth |
|---|---|---|---|---|
| `sparkov` | HF `santosh3110/credit_card_fraud_transactions`, Kaggle "fraudTrain" mirror (Apache-2.0 HF tag) | last `target` transactions of the timeline (keeps per-card history); the mirror is truncated to 1,048,575 rows and went through Excel: `unix_time` is used as timestamp and card identity is rebuilt from the rounded `cc_num` + holder + birth date | `carding` | `card_fraud` |
| `paysim` | HF `theman10/paysim` (MIT) | fraud sampled to ~2 % of `target` (seeded) + random legit; `step` = hour, random second inside it | `bank_account_takeover` | `ato_<type>` (e.g. `ato_cash_out`, `ato_transfer`) |
| `saml-d` | HF `LordNR/AMLGraphX-SAML-D` (CC BY-NC-SA 4.0) | whole histories of laundering senders up to ~30 % of `target`, then non-laundering senders | `money_mule` | SAML-D `Laundering_type` |

Output in `<out>/<tenant>/`: `ground_truth.jsonl` (`project`, `external_id`, `fraud_type` = the dataset's own label,
`platform_typology` = what was sent) and `source.json` (dataset, URL, licence, sample target, seed, events, window
end, tenant), next to the export files below. In research exports `truth_fraud_type` is therefore the dataset's
fine-grained label, not a platform typology.

## Contents

See the generated `README.md` inside each dataset folder for row counts, checksums and a metric snapshot.
`manifest.json` lists every table with rows, columns, file size and SHA-256.

| File | Grain | Key columns |
|---|---|---|
| `events.parquet` / `.csv.gz` | event | per-engine scores, final score, decision, ML/graph outputs, labels, case status, latency, degraded engines, **`truth_fraud_type` / `is_fraud_truth`** |
| `rule_hits.parquet` (+ `.csv.gz`) | rule evaluation that matched/trapped | rule code/kind/version, ruleset, outcome, contribution, shadow |
| `features.parquet` | event | `f_*` feature vector used by the engines, `feature_set_version` (skipped with `--no-features`) |
| `anomaly.parquet` | event × unsupervised model | anomaly score, cluster id, PCA x/y |
| `rules`, `rulesets`, `models`, `clusters`, `cases`, `labels`, `proposals`, `llm_reports`, `graph_communities`, `project_settings` (parquet; `rules`, `models`, `cases` also `.csv.gz`) | catalogue/artefact tables | definitions, metrics JSON, review status |
| `metrics/*.csv` | project × engine / threshold / typology | see below |
| `ground_truth.jsonl` | event | truth used for the join (regenerated, or the `--truth` file) |

Metrics (`research_export/metrics.py`; only scored events — `load_only = false` with a decision):

| File | Content |
|---|---|
| `dataset_summary.csv` | per project: stage, events, load-only/scored events, truth fraud events and rate, labelled events, time range, decision counts |
| `engine_auc.csv` | ROC-AUC and PR-AUC (average precision) per engine (`final`, `rules`, `supervised`, `unsupervised`, `graph`) on events where that engine produced a score, with `coverage` and fraud count |
| `operating_points.csv` | tp/fp/fn/tn, recall, precision, FPR at default review 50, decline 80, and the lowest threshold (0.5 step) with FPR ≤ 5 % |
| `typology_recall.csv` | per project × truth typology: events, recall at 50 and 80, mean final score |
| `threshold_sweep.csv` | confusion, recall, precision, FPR at thresholds 0–100 in steps of 2.5 |

`event_label` is what the platform knew (delayed, partial). **Always evaluate against `is_fraud_truth`.**

## Running an export manually

```bash
cd tools/research_export
uv run python -m research_export --out ~/datasets/fraud-live-demo/manual --sim-end <SIM_END> --sim-seed <SIM_SEED>
# values from ~/.local/state/fraud-demo/last-run.env (also --sim-customers / --sim-days, defaults 2000 / 60);
# or --truth file.jsonl from `simulator --truth-out`; --tenant <slug> (default demo); --no-features
```

With `--sim-end` the exporter regenerates the truth by running `simulator stats --truth-out`. It connects as the
Postgres superuser on `127.0.0.1:POSTGRES_HOST_PORT` (default 5433; credentials from `.env`) with a read-only
session; it does not touch the evaluation instance (`fraud-eval`, port 5437).

## Suggested analyses

1. **Engine complementarity:** ROC/PR-AUC per engine vs fused score, and recall of events caught by exactly one engine.
2. **Typology coverage:** recall per `truth_fraud_type` at review/decline thresholds; which engine contributes (`reasons_json`).
3. **Threshold calibration:** default 50/80 vs the FPR ≤ 5 % operating point per project (`metrics/operating_points.csv`).
4. **Replication:** the same metrics over several datasets with different seeds → mean ± CI.
5. **Latency and degradation:** `latency_ms`, `degraded_engines` distribution.
6. **Governance/LLM:** proposals with citations, validation and backtest results; reports content.

## Limitations

Synthetic generator (demo) and mostly synthetic public datasets; base rates and typologies follow their assumptions.
Events scored before model approval have no ML component (see `coverage` in `engine_auc.csv`). LLM output quality
depends on the model available on the demo host; research tenants skip the LLM activity. The demo is public, so
artefacts created by visitors may appear between a seed and its export (the export runs right after seeding).
