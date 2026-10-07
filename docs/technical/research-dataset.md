# Research dataset (live demo export)

A daily, self-describing dataset of the platform **running with every feature on**: rules, supervised and
unsupervised ML, graph, cases, labels and the LLM assistant. It is meant for analysis and for the paper
(*Five Engines, One Decision*), next to the synthetic evaluation in [evaluation.md](evaluation.md) and the
public-dataset evaluation (`tools/public_eval`, separate instance).

## Two seeding modes, one install

* **demo** — tenant `demo`, synthetic simulator (below).
* **research** — one tenant per public dataset (`exp-sparkov`, `exp-paysim`, `exp-saml-d`), loaded by
  `tools/research_seed` with the same protocol; exports go to `~/datasets/fraud-research/<tenant>/` with the dataset's
  own labels as ground truth.

All tenants are archived together as the **master snapshot** (`deploy/demo/master.sh`), restored at 00:00 with
timestamps shifted to the present. Commands and the reproduction tutorial: [experiments.md](../guides/experiments.md).

## How it is produced

`deploy/demo/reset-demo.sh` (cron 00:00 WIB on the demo host) runs:

| Step | What happens | Why it matters for analysis |
|---|---|---|
| 1. wipe + start | data volumes of compose project `fraud-platform` removed | every day starts from the same empty state |
| 2. history seed | `simulator --phase history`: first 70 % of a 60-day window as `load_only` events, plus delayed labels and confirmed fraud rings | history for velocity features, model training and graph "known fraudsters" |
| 3. post-seed | regulation PDFs uploaded/attached; per project `mlp_backprop` (supervised) and `isolation_forest` + `hdbscan` (unsupervised) trained, submitted (analyst) and approved (approver) | maker–checker exactly as in production |
| 4. online seed | `simulator --phase online`: last 30 % scored online, **all engines active** | engine scores are comparable on the same events |
| 5. activity | `deploy/demo/activity.py`: cases worked/resolved, cluster named, communities recomputed, device blacklist, LLM reports + one proposal approved into shadow | governance and assistant artefacts |
| 6. export | `tools/research_export` → `~/datasets/fraud-live-demo/<YYYY-MM-DD>/` | the dataset |

The simulator is deterministic: **seed + window end** fully define the data and the ground truth. The reset uses
seed = date (`YYYYMMDD`) and records both in `~/.local/state/fraud-demo/last-run.env` and in the dataset's
`manifest.json`. Each day is therefore an independent replication of the same generator, which gives
repeated-run confidence intervals.

## Contents

See the generated `README.md` inside each dataset folder for row counts, checksums and a metric snapshot.

| File | Grain | Key columns |
|---|---|---|
| `events.parquet` / `.csv.gz` | event | per-engine scores, final score, decision, ML/graph outputs, labels, case status, **`truth_fraud_type` / `is_fraud_truth`** |
| `rule_hits.parquet` | rule evaluation that matched/trapped | rule code/kind/version, ruleset, outcome, contribution, shadow |
| `features.parquet` | event | `f_*` feature vector used by the engines |
| `anomaly.parquet` | event × unsupervised model | anomaly score, cluster id, PCA x/y |
| `rules`, `rulesets`, `models`, `clusters`, `cases`, `labels`, `proposals`, `llm_reports`, `graph_communities`, `project_settings` | catalogue/artefact tables | definitions, metrics JSON, review status |
| `metrics/*.csv` | project × engine / threshold / typology | AUC per engine, operating points, typology recall, threshold sweep |

`event_label` is what the platform knew (delayed, partial). **Always evaluate against `is_fraud_truth`.**

## Running an export manually

```bash
cd tools/research_export
uv run python -m research_export --out ~/datasets/fraud-live-demo/manual --sim-end <SIM_END> --sim-seed <SIM_SEED>
# values from ~/.local/state/fraud-demo/last-run.env; or --truth file.jsonl from `simulator --truth-out`
```

The exporter connects read-only as the Postgres superuser on `127.0.0.1:5433` (credentials from `.env`); it does not
touch the evaluation instance (`fraud-eval`, port 5437).

## Suggested analyses

1. **Engine complementarity:** ROC/PR-AUC per engine vs fused score, and recall of events caught by exactly one engine.
2. **Typology coverage:** recall per `truth_fraud_type` at review/decline thresholds; which engine contributes (`reasons_json`).
3. **Threshold calibration:** default 50/80 vs the FPR ≤ 5 % operating point per project (`metrics/operating_points.csv`).
4. **Replication:** the same metrics over several daily datasets → mean ± CI.
5. **Latency and degradation:** `latency_ms`, `degraded_engines` distribution.
6. **Governance/LLM:** proposals with citations, validation and backtest results; reports content.

## Limitations

Synthetic generator; base rates and typologies follow its assumptions. LLM output quality depends on the model
available on the demo host (CPU inference without GPU). The demo is public, so artefacts created by visitors may
appear between reset and export (export runs right after the reset to minimise this).
