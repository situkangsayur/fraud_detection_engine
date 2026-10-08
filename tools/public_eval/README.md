# public_eval — end-to-end evaluation on public fraud datasets

Runs published fraud datasets through the **real platform API** (ingest → features → graph → ML → rules →
noisy-OR decision) on a separate evaluation stack, then joins the stored decisions with a local truth file that is
never sent to the platform.

## Datasets

| Key | Dataset | Real / synthetic | Licence | Download (verified 2026-10-07) |
|---|---|---|---|---|
| `tabformer` | IBM TabFormer card transactions (Padhi et al., ICASSP 2021) | synthetic | Apache-2.0 | `https://github.com/IBM/TabFormer/raw/main/data/credit_card/transactions.tgz` |
| `ecom` | Fraud_Data e-commerce (120k-row public copy) | unknown origin, likely synthetic | **unknown — internal test only, do not redistribute** | `https://raw.githubusercontent.com/lu-16/FraudDetection/master/data/fraud.csv` (+ `IpAddress_to_Country.csv`) |
| `aml` | IBM AMLworld HI-Small (Altman et al., NeurIPS 2023) | synthetic | CDLA-Sharing-1.0 | `https://huggingface.co/datasets/OsamaMIT/IBM-AML-HI-Small/resolve/main/HI-Small_{Trans,accounts}.csv`, `HI-Small_Patterns.txt` |
| `rba` | RBA login dataset (Wiefling et al., ACM TOPS 2022) | synthesized from real SSO logs | CC BY 4.0 | `https://zenodo.org/records/6782156/files/rba-dataset.zip?download=1` |
| `ulb` | ULB credit card fraud (Dal Pozzolo et al. 2015) | real (PCA features) | DbCL v1.0 | `https://data.openml.org/datasets/0000/1597/dataset_1597.pq` |

`~/datasets/fraud-public/download.sh` fetches them. Raw files are treated as untrusted data (parsed, never executed).

## Sampling (per seed)

* **tabformer** — the 4-month window with the most fraud (2015-10 … 2016-01); users with fraud in the window up to half
  of ~80k events, then random other users; whole user histories inside the window.
* **ecom** — all 120,000 rows (one purchase per user; device and IP shared across accounts).
* **aml** — laundering senders (whole histories, ≤ 500 tx; hub accounts skipped) up to half of ~80k events, then random
  other senders; recipient account → `recipient_fingerprint` (bank-account graph entity). Pattern type per laundering
  transaction from `Patterns.txt`.
* **rba** — all users with an account-takeover login plus random users (≤ 2,000 logins each) up to ~110k logins; the
  window ends 3 days after the last ATO label (labels exist only in the first ~9 months of the source).
  `Is Attack IP` is label-like and is **not** mapped. Device = device type | OS | browser.
* **ulb** — ML-only benchmark (no entity ids): all 492 fraud + 60,000 random legit rows; V1–V28 passed to the models as
  `extra_source_fields`; template `none`, so the rules engine is idle.

Timestamps are shifted so the newest event is ~1 h before now (relative spacing kept), because training windows and
label maturity are relative to "now".

## Protocol (per dataset, seed, configuration)

1. Tenant `public-eval`; project `<dataset>-<config>-s<seed>-<tag>`; an **analyst** and a separate **approver** user.
2. First 70 % of events (chronological, by count) ingested `load_only`; delayed partial labels posted
   (60 % of fraud older than min(7 days, 20 % of the history span), 3 % of legit; customers of labelled fraud marked
   fraud with p = 0.7 to feed graph proximity).
   Supervised label maturity = age of the last history event + label delay, capped at 14 days (short datasets
   such as ULB and AMLworld would otherwise train on labelled rows only).
3. Supervised `mlp_backprop` and unsupervised `isolation_forest` + `hdbscan` trained by the analyst, approved by the
   approver (maker–checker).
4. Remaining 30 % ingested with `mode=score`. Eval projects get engine timeouts ML 3 s / graph 2 s / rules 5 s
   (`common.EVAL_TIMEOUTS`) because several projects and model training share one host; degraded decisions are
   counted (`degraded_rate`) and metrics on non-degraded events are reported separately.
5. Metrics from `core.decisions` ⨝ truth: recall / precision / FPR at default thresholds (review ≥ 50, decline ≥ 80),
   decline precision, AUC of the fused score and of each engine score, recall per typology / laundering pattern,
   the review threshold for FPR ≤ 5 %, and per-rule hit counts.

Configurations: `tpl` = the platform's stage-template rules; `lit` = template `none` + the literature-referenced
ruleset `RS-LIT` (`rules_lit.py`, codes `RL-LIT-Rxx` from the rule catalogue, source in each rule's description,
[S]/[P] thresholds, rules approved by the approver); `ml` = no rules (ULB).

## Commands

```bash
# evaluation stack (separate from the live one)
docker compose -p fraud-eval --env-file ~/fraud-eval/.env.eval \
  -f docker-compose.yml -f ~/fraud-eval/compose.eval.yml \
  up -d --no-build postgres migrate core-api rule-service graph-service ml-service ingest-service gateway

cd tools/public_eval
python -m public_eval prepare <dataset> --seed 1
python -m public_eval run <dataset> --seed 1 --config tpl|lit|ml
python -m public_eval evaluate <dataset> --seed 1 --config tpl
python -m public_eval.aggregate          # → results-public.json + evaluation-public-tables.md in $EVAL_WORK_DIR
```

Runs are resumable (existing events, labels, models and the LIT ruleset are reused). `~/fraud-eval/supervise.sh`
(not in the repo) restarted crashed runs with at most 6 in parallel. Results: `docs/technical/evaluation-public.md`.

Requirements: Python 3.12 with `pandas pyarrow httpx scikit-learn numpy psycopg2-binary pycountry`.
Environment: `EVAL_ENV_FILE` (default `~/fraud-eval/.env.eval`), `EVAL_WORK_DIR`, `EVAL_RAW_DIR`.
