# Detection quality — end-to-end evaluation

> Document status: verified against commit 07ee765 on 2026-10-13

This document collects three evaluations, all measured on decisions the platform stored after ingesting data through
its API, joined afterwards with ground truth that is never sent to the platform:

| § | Run | Data | Tooling |
|---|---|---|---|
| 1–4 | 2026-09-24 | synthetic simulator, out-of-sample run tag `E1` | `tools/simulator` + manual join with `core.decisions` |
| 5 | 2026-10-07 | research datasets: demo tenant (simulator) + Sparkov, PaySim, SAML-D tenants | `tools/research_seed`, `tools/research_export` (this branch) |
| 6 | 2026-10-07/08 | public datasets: TabFormer, Fraud_Data, AMLworld HI-Small, RBA, ULB | `tools/public_eval` — **branch `eval/public-datasets` only** |

Where the metrics are computed in code:

* **Platform-side model metrics** (validation split, shown on the ML pages): `ml_service/training/metrics.py` —
  ROC-AUC, PR-AUC (average precision), precision/recall/F1/flagged rate at 0.3/0.5/0.7, confusion matrix at 0.5,
  10-bin calibration, permutation importance (PR-AUC drop); unsupervised: anomaly ROC-AUC vs labels, silhouette,
  noise ratio, score quantiles (ml-plugins.md §4).
* **End-to-end metrics against ground truth** (§5): `tools/research_export/research_export/metrics.py` — per-engine
  ROC-AUC and PR-AUC with coverage, recall/precision/FPR at review 50 / decline 80, lowest threshold with FPR ≤ 5 %,
  recall per typology at 50/80, threshold sweep (research-dataset.md).
* §6 uses its own evaluator on the eval branch (`public_eval/evaluate.py`: ROC-AUC per engine, recall/precision/FPR at
  default thresholds, FPR ≤ 5 % threshold, rule hit counts, recall per typology).

The sections 1–4 below describe the 2026-09-24 run: full docker-compose stack, all engines active (rules from stage
templates, supervised `mlp_backprop`, unsupervised `isolation_forest` + `hdbscan`, graph).

## 1. Method

1. **History (training) run:** simulator seed 42, 300 customers, 30 days, 4 projects (checkout, post-payment,
   returns, promo). 70% of the window was ingested `load_only` and the rest scored. Partial, delayed labels were
   posted (60% of fraud events, 3% of legit).
2. **Training:** per project, the supervised model uses label maturity of 14 days (older unlabelled events count as
   legit; the API default) and a time-based validation split. The unsupervised model is trained on the last 90 days
   (the API default). Both models were
   submitted by an analyst and approved by an approver (maker–checker; self-approval returned 403 as expected).
3. **Out-of-sample run:** simulator with a **new run tag (`E1`) and seed 7**, so new customers, events and fraud rings
   the models never saw. 10 days, 300 customers, **100% scored** (`SIM_LOAD_ONLY_SHARE=0`).
4. **Ground truth:** the simulator writes the true label of every event to a local file (`--truth-out`). This file
   is never sent to the platform. Metrics join it with `core.decisions`.

"Flagged" means decision `review` or `decline`. Thresholds were the defaults (review ≥ 50, decline ≥ 80), and
`engine_combination = noisy_or`.

## 2. Results (out-of-sample, default thresholds)

| Project | Events | Fraud | Recall | Precision | FPR | Decline precision | AUC final | AUC rules | AUC supervised | AUC anomaly | AUC graph |
|---|---|---|---|---|---|---|---|---|---|---|---|
| checkout | 2129 | 105 | 95.2% | 40.0% | 7.4% | 93.5% | 0.986 | 0.783 | 0.993 | 0.975 | 0.524 |
| post-payment | 580 | 45 | 82.2% | 17.7% | 32.1% | 91.3% | 0.862 | 0.516 | 0.791 | 0.823 | 0.500 |
| returns | 423 | 22 | 100% | 13.7% | 34.7% | 57.9% | 0.998 | 0.773 | 0.944 | 0.988 | 0.500 |
| promo | 518 | 45 | 100% | 63.4% | 5.5% | 78.9% | 0.999 | 0.722 | 0.998 | 0.990 | 0.496 |

Recall per typology: carding 100% · system breach 100% · promo abuse 100% · refund abuse 100% ·
bank account takeover 100% · account takeover 84% · money mule 78%.

### Per-project threshold calibration (same scores, FPR ≤ 5%)

| Project | Review threshold | Recall | Precision |
|---|---|---|---|
| checkout | 55.0 | 94.3% | 49.5% |
| post-payment | 65.5 | 64.4% | 52.7% |
| returns | 75.9 | 100% | 52.4% |
| promo | 52.5 | 100% | 66.2% |

## 3. Reading the results

* **Ranking quality is high** (AUC of the final score 0.86–0.999). The combination of rules, supervised and
  unsupervised engines separates fraud from legit well on unseen customers.
* **Thresholds must be calibrated per project.** The same default of 50 gives 5–7% FPR in checkout/promo but 32–35%
  in post-payment/returns. This is expected: each stage has a different base rate and behaviour, which is why
  thresholds are a project setting. Backlog E3 now has *threshold recommendation from labelled data* at P1.
* **Graph AUC ≈ 0.5 in this run** is expected. The `E1` rings are new and share no entities with labelled fraud
  customers of the first run, and their own members were not labelled before scoring. The graph engine contributes
  once fraud is confirmed on connected customers (see the integration test and graph metrics). A follow-up run
  after labelling part of `E1` would show it.
* **Why noisy-OR:** the first end-to-end run used the weighted average, and with no ML model a strong rule hit was
  diluted below the review threshold (only 4 reviews for 111 fraud events). See architecture.md §3.1.
* **Post-payment (money mules)** is the weakest typology. Mule behaviour shows up in fan-in/fan-out patterns,
  so dedicated velocity rules on `recipient_fingerprint` and graph community features are the next improvement.
* This is **synthetic data**. Absolute numbers will differ in production; the method is what transfers.

## 4. Reproduce

The commands below seed a running stack; do not run them against a live/demo installation you want to keep.

```bash
docker compose --profile seed run --rm -e SIM_CUSTOMERS=300 -e SIM_DAYS=30 simulator            # history
# train + approve models per project (UI: ML pages, or API: /ml/{supervised,unsupervised}/train → submit → approve)
docker compose --profile seed run --rm -v "$PWD/eval:/out" \
  -e SIM_CUSTOMERS=300 -e SIM_DAYS=10 -e SIM_SEED=7 -e SIM_RUN_TAG=E1 -e SIM_LOAD_ONLY_SHARE=0 \
  -e SIM_TRUTH_OUT=/out/truth_E1.jsonl simulator run --force                                   # unseen traffic
# join eval/truth_E1.jsonl with core.decisions (events whose external_id contains "-E1-")
```

## 5. Results on the research datasets (run 2026-10-07)

Source: the research datasets of the 2026-10-07 multi-tenant run (research-dataset.md), metrics from
`metrics/*.csv` of each export. All numbers: events scored online (last 30 % of the timeline), against the
**ground truth** (not the platform's labels). Protocol: history 70 % → train and approve models → online with all
engines active. Default thresholds review ≥ 50, decline ≥ 80.

| Tenant | Folder | Events | Fraud (truth) |
|---|---|---|---|
| `demo` (synthetic, 4 projects, seed 20261007) | `~/datasets/fraud-live-demo/2026-10-07/` | 134 924 | see tables |
| `exp-sparkov` (Sparkov, card fraud) | `~/datasets/fraud-research/exp-sparkov/` | 60 000 | 368 |
| `exp-paysim` (PaySim, mobile money) | `~/datasets/fraud-research/exp-paysim/` | 59 995 | 1 200 (348 in the online part) |
| `exp-saml-d` (SAML-D, AML) | `~/datasets/fraud-research/exp-saml-d/` | 60 116 | 246 (87 in the online part), 15 typologies |

### 5.1 Ranking quality per engine (ROC-AUC / PR-AUC)

| Tenant / project | Fused (final) | Rules | Supervised | Unsupervised | Graph |
|---|---|---|---|---|---|
| demo / checkout | **0.999** / 0.977 | 0.903 / 0.311 | 0.995 / 0.943 | 0.988 / 0.844 | 0.666 / 0.083 |
| demo / post-payment | **0.957** / 0.678 | 0.796 / 0.104 | 0.934 / 0.716 | 0.910 / 0.278 | n/a (coverage 0.2 %) |
| demo / promo | **1.000** / 0.985 | 0.619 / 0.308 | 1.000 / 0.991 | 0.988 / 0.615 | 0.578 / 0.032 |
| demo / returns (7 fraud) | **0.992** / 0.577 | 0.784 / 0.223 | 0.994 / 0.502 | 0.948 / 0.018 | 0.641 / 0.039 |
| exp-sparkov | **0.976** / 0.557 | 0.891 / 0.176 | 0.985 / 0.504 | 0.959 / 0.291 | 0.500 / 0.004 |
| exp-paysim | **0.860** / 0.640 | 0.585 / 0.141 | 0.890 / 0.669 | 0.783 / 0.442 | 0.500 / 0.116 |
| exp-saml-d | **0.942** / 0.177 | 0.500 / 0.005 | 0.973 / 0.238 | 0.791 / 0.012 | 0.500 / 0.005 |

### 5.2 Operating points

| Tenant / project | Default review 50 (recall / precision / FPR) | Calibrated FPR ≤ 5 % (threshold → recall / precision) |
|---|---|---|
| demo / checkout | 1.000 / 0.067 / **0.232** | 75.0 → 1.000 / 0.258 |
| demo / post-payment | 0.857 / 0.140 / 0.057 | 52.0 → 0.843 / 0.160 |
| demo / promo | 1.000 / 0.026 / **0.358** | 78.0 → 1.000 / 0.170 |
| demo / returns | 1.000 / 0.009 / 0.158 | 68.5 → 1.000 / 0.030 |
| exp-sparkov | 0.959 / 0.042 / 0.094 | 61.0 → 0.959 / 0.076 |
| exp-paysim | 0.897 / 0.204 / **0.459** | 85.5 → 0.580 / 0.612 |
| exp-saml-d | 0.816 / 0.025 / 0.155 | 76.5 → 0.782 / 0.072 |

### 5.3 Recall per typology

Demo (threshold 50 / 80):

| Project | Typology | Events | Recall@50 | Recall@80 |
|---|---|---|---|---|
| checkout | system_breach | 215 | 1.000 | 1.000 |
| checkout | carding | 118 | 1.000 | 1.000 |
| checkout | account_takeover | 54 | 1.000 | 0.981 |
| post-payment | money_mule | 65 | 0.846 | **0.354** |
| post-payment | bank_account_takeover | 5 | 1.000 | 1.000 |
| promo | promo_abuse | 60 | 1.000 | 1.000 |
| returns | refund_abuse | 7 | 1.000 | 0.857 |

PaySim (dataset's own fine-grained label):

| Typology | Online events | Recall@50 | Recall@80 |
|---|---|---|---|
| ato_cash_out | 182 | 1.000 | 0.758 |
| ato_transfer | 166 | 0.783 | 0.536 |

SAML-D (small online n — read as indicative):

| Typology | n | Recall@50 | Recall@80 |
|---|---|---|---|
| Cash_Withdrawal | 17 | **0.06** | 0.06 |
| Behavioural_Change_1 | 15 | 1.00 | 1.00 |
| Structuring | 14 | 1.00 | 0.93 |
| Deposit-Send | 8 | 1.00 | 1.00 |
| Smurfing | 7 | 1.00 | 1.00 |
| Scatter-Gather | 6 | 1.00 | 1.00 |
| Stacked Bipartite | 5 | 1.00 | 0.60 |
| Behavioural_Change_2 | 5 | 1.00 | 1.00 |
| Cycle / Fan_In / Gather-Scatter / Layered_Fan_In / Single_large | 1–3 | 1.00 | 0.50–1.00 |

### 5.4 Reading the results

1. **The fused score is always at or very close to the best single engine**; the ML engines active at scoring time
   contribute most. Checkout fused AUC rose from 0.986 in the 2026-09-24 run (§2) to 0.999; the source notes
   attribute this to the ML models being active at scoring time in the new protocol.
2. **The default threshold of 50 does not fit every project:** FPR 23 % on checkout and 36 % on promo. Per-project
   calibration (FPR ≤ 5 %) keeps recall at ~100 % (checkout, promo, returns) — support for per-project threshold
   recommendation (backlog P1).
3. **The graph only helps with shared entities and already known fraudsters:** Sparkov (no device/IP/recipient
   account) → graph AUC 0.5; post-payment graph coverage 0.2 %.
4. **Money mules are the hardest demo typology** (recall@80 0.35): layered transfer patterns need richer graph and
   temporal features.
5. **PaySim is the hardest dataset** (AUC 0.86): origin accounts have no history, so velocity features do not work;
   supervised ML is strongest, rules weak (0.585). The online part holds only 5 % of the events (activity is
   concentrated at the start of the timeline) with an 11.6 % fraud rate, versus 2 % overall.
6. **The graph needs confirmed fraudsters.** Research tenants receive event labels only (no customer labels), so the
   graph has no anchor → AUC 0.5 on all three public datasets, even SAML-D (AML), which should benefit most.
   Follow-up: send customer labels for confirmed perpetrators in the history phase.
7. **Stage-template rules do not fit public data** (SAML-D rules AUC 0.5, PaySim 0.585): the templates target
   Indonesian e-commerce events (compare the literature-based rules in §6).
8. **SAML-D Cash_Withdrawal is almost undetected** (recall 0.06): a cash typology outside the reach of digital-channel
   features.
9. **Returns has only 7 online fraud events** → wide confidence intervals; combine several replications (seeds).

Caveats: synthetic data (simulator and synthetic public datasets; the Sparkov HF mirror is degraded — it went through
Excel, timestamps lost their seconds and card numbers were rounded, see research-dataset.md); ML models are retrained on every run (small
variation) and there is one run per dataset so far → more seed replications are needed; LLM reports/proposals on the
demo used `qwen3:8b` on CPU, so the LLM evaluation is qualitative.

Reproduce: `deploy/seed.sh demo` / `deploy/seed.sh research <dataset>` (destructive for `demo`; see
research-dataset.md and [experiments.md](../guides/experiments.md)), then read `metrics/*.csv` in each export.

## 6. Results on public datasets (runs 2026-10-07/08, branch `eval/public-datasets`)

> The tooling for this section (`tools/public_eval`) and its full write-up (`docs/technical/evaluation-public.md`)
> exist **only on branch `eval/public-datasets`**, not on `master` or this docs branch. Numbers below are copied from
> that write-up.

Separate evaluation stack (compose project `fraud-eval`, same images as the live stack; Postgres on host port
5437), tenant `public-eval`, one project per dataset × configuration × seed, separate analyst and approver users.

**Datasets and sampling**

| Dataset | Real / synthetic | Licence | Events (per seed) | Fraud in scored 30 % | Platform fields used |
|---|---|---|---|---|---|
| IBM TabFormer card transactions (Padhi et al. 2021) | synthetic | Apache-2.0 | ~80,000 (4-month window with most fraud, 2015-10…2016-01) | 150–183 | customer, card (hashed), merchant, MCC, city, chip/online, error codes (`source.errors`) |
| Fraud_Data e-commerce (public 120k copy) | unknown, likely synthetic | **unknown — internal test only** | 120,000 (all) | 1,647 | customer, sign-up time, device, IP (+ IP country) |
| IBM AMLworld HI-Small (Altman et al. 2023) | synthetic | CDLA-Sharing-1.0 | ~80,000 (laundering senders ≤ 500 tx up to half, then random senders) | 1,382–1,506 | sender = customer, recipient account → bank-account entity, amount, payment format |
| RBA login (Wiefling et al. 2022) | synthesized from real SSO logs | CC BY 4.0 | ~84,000 (all 138 ATO users + random users; window ends 3 days after last ATO label) | 38 | user, IP, country, city, device (type, OS, browser), user agent, login success |
| ULB credit card (Dal Pozzolo et al. 2015) | real (PCA) | DbCL v1.0 | 60,492 (all 492 fraud + 60,000 legit) | 109 | ML-only: V1–V28 + amount as `extra_source_fields`; no entity ids |

Three seeds per dataset vary the user/account sample (TabFormer, AMLworld, RBA) and the label plan (all).

**Protocol:** first 70 % of events (chronological, by count) `load_only`; delayed partial labels (60 % of fraud older
than min(7 days, 20 % of the history span), 3 % of legit; customers of labelled fraud marked fraud with p = 0.7);
supervised label maturity = age of the last history event + label delay, capped at 14 days; `mlp_backprop` +
`isolation_forest` + `hdbscan` trained by the analyst, approved by the approver; remaining 30 % scored with default
thresholds (review ≥ 50, decline ≥ 80) and noisy-OR fusion. **Evaluation-only engine timeouts** (ML 3 s, graph 2 s,
rules 5 s instead of 100/200/300 ms) because several projects and trainings shared one host; degraded share in the
final runs 0.1–0.3 % of decisions, metrics on non-degraded events within 0.006 AUC of the full set.

Configurations: **tpl** = stage-template rules (card → `pre_payment`, e-commerce → `promo`, AMLworld → `payout`,
RBA → `account_security`); **lit** = template `none` + ruleset `RS-LIT` from the literature rule catalogue (R01–R40,
codes `RL-LIT-Rxx`, not tuned on the test window); **ml** = no rules (ULB).

### 6.1 Results (mean ± std over 3 seeds, default thresholds; AUC = ROC-AUC)

| Dataset | Config | Seeds | Scored events | Fraud | Recall % | Precision % | FPR % | AUC fused | AUC rules | AUC superv. | AUC anomaly | AUC graph |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| TabFormer (card) | tpl | 3 | 24024 ± 8 | 172 ± 19 | 90.8 ± 4.5 | 2.8 ± 0.1 | 22.7 ± 1.8 | 0.919 ± 0.028 | 0.660 ± 0.023 | 0.903 ± 0.030 | 0.885 ± 0.018 | 0.500 ± 0.000 |
| TabFormer (card) | lit | 3 | 24024 ± 9 | 172 ± 19 | 95.9 ± 2.4 | 1.6 ± 0.2 | 43.7 ± 6.4 | 0.914 ± 0.030 | 0.703 ± 0.014 | 0.909 ± 0.030 | 0.886 ± 0.020 | 0.500 ± 0.000 |
| E-commerce (Fraud_Data) | tpl | 3 | 35997 ± 4 | 1647 ± 0 | 31.2 ± 1.9 | 12.8 ± 2.4 | 10.6 ± 2.6 | 0.611 ± 0.001 | 0.500 ± 0.000 | 0.587 ± 0.046 | 0.608 ± 0.001 | 0.506 ± 0.005 |
| E-commerce (Fraud_Data) | lit | 3 | 35997 ± 3 | 1647 ± 0 | 31.2 ± 2.6 | 13.2 ± 3.8 | 10.6 ± 3.7 | 0.611 ± 0.001 | 0.610 ± 0.000 | 0.589 ± 0.049 | 0.607 ± 0.001 | 0.506 ± 0.005 |
| AMLworld HI-Small | tpl | 3 | 24000 ± 7 | 1436 ± 64 | 97.1 ± 4.5 | 12.6 ± 7.4 | 52.8 ± 26.3 | 0.958 ± 0.016 | 0.500 ± 0.000 | 0.971 ± 0.003 | 0.820 ± 0.026 | 0.526 ± 0.015 |
| AMLworld HI-Small | lit | 3 | 24001 ± 4 | 1436 ± 64 | 98.5 ± 2.0 | 12.4 ± 4.9 | 49.1 ± 18.9 | 0.957 ± 0.007 | 0.650 ± 0.023 | 0.974 ± 0.002 | 0.824 ± 0.011 | 0.526 ± 0.016 |
| RBA login | tpl | 3 | 25179 ± 96 | 38 ± 0 | 93.0 ± 3.0 | 0.3 ± 0.1 | 47.0 ± 8.4 | 0.884 ± 0.031 | 0.498 ± 0.000 | 0.831 ± 0.017 | 0.691 ± 0.011 | 0.738 ± 0.035 |
| RBA login | lit | 3 | 25178 ± 99 | 38 ± 0 | 97.4 ± 0.0 | 0.2 ± 0.1 | 61.7 ± 12.9 | 0.877 ± 0.041 | 0.725 ± 0.000 | 0.814 ± 0.036 | 0.695 ± 0.013 | 0.706 ± 0.051 |
| ULB credit card | ml | 3 | 18146 ± 1 | 109 ± 0 | 89.0 ± 2.4 | 5.8 ± 1.3 | 9.2 ± 2.9 | 0.956 ± 0.015 | 0.500 ± 0.000 | 0.951 ± 0.009 | 0.933 ± 0.009 | 0.500 ± 0.000 |

### 6.2 Per-project review threshold calibrated to FPR ≤ 5 % (same scores)

| Dataset | Config | Threshold | Recall % | Precision % |
|---|---|---|---|---|
| TabFormer (card) | tpl | 87.1 ± 3.5 | 64.5 ± 7.8 | 8.5 ± 1.1 |
| TabFormer (card) | lit | 93.3 ± 2.0 | 64.1 ± 9.1 | 8.4 ± 1.1 |
| E-commerce (Fraud_Data) | tpl | 58.5 ± 2.2 | 25.8 ± 1.4 | 19.9 ± 0.8 |
| E-commerce (Fraud_Data) | lit | 58.4 ± 3.8 | 26.8 ± 0.3 | 20.4 ± 0.2 |
| AMLworld HI-Small | tpl | 90.7 ± 10.0 | 83.0 ± 3.8 | 51.4 ± 1.5 |
| AMLworld HI-Small | lit | 95.7 ± 2.1 | 82.2 ± 3.4 | 51.1 ± 0.8 |
| RBA login | tpl | 80.4 ± 5.4 | 63.2 ± 4.6 | 1.9 ± 0.1 |
| RBA login | lit | 80.6 ± 1.4 | 59.6 ± 4.0 | 1.8 ± 0.1 |
| ULB credit card | ml | 56.2 ± 3.8 | 88.1 ± 1.8 | 9.6 ± 0.2 |

Recall by typology / laundering pattern (seed 1, default threshold): AMLworld / lit: BIPARTITE 100 % (n=67), CYCLE
100 % (n=100), FAN-IN 100 % (n=108), FAN-OUT 100 % (n=141), GATHER-SCATTER 100 % (n=298), RANDOM 98 % (n=66),
SCATTER-GATHER 99 % (n=192), STACK 99 % (n=142), UNLISTED 99 % (n=306); AMLworld / tpl: all patterns 100 % except
STACK 99 % and UNLISTED 99 %; e-commerce other 28 % (lit) / 29 % (tpl) (n=1647); RBA account_takeover 97 % (lit) /
89 % (tpl) (n=38); TabFormer carding 95 % (lit) / 86 % (tpl) (n=183); ULB carding 92 % (n=109). Per-rule hit
counts are in the eval-branch write-up.

### 6.3 Reading the results

* **The fused score ranks well where the data carries behavioural signal:** AUC 0.914–0.919 on TabFormer,
  0.957–0.958 on AMLworld, 0.877–0.884 on RBA and 0.956 on ULB. On every dataset the fused AUC is within 0.02 of the
  best single engine or above it; on RBA it is 0.05 above the best single engine (supervised 0.81–0.83).
* **The graph engine contributes where identities are shared across customers:** RBA graph AUC 0.71–0.74 from a cold
  start with only partial customer labels; AMLworld 0.53; TabFormer and ULB have no cross-customer identifiers, so
  graph is 0.50 by construction.
* **Literature rules > stage templates as a ranking signal:** rules-engine AUC 0.66 → 0.70 (TabFormer), 0.50 → 0.61
  (e-commerce), 0.50 → 0.65 (AMLworld), 0.50 → 0.73 (RBA). The fused AUC barely changes between configurations
  (≤ 0.007) because supervised and anomaly models dominate; LIT rules raise recall at the default threshold
  (TabFormer 91 → 96 %, RBA 93 → 97 %) and add citable explanations.
* **Generic regulatory gates are poor detectors on their own:** PSD2 low-value limits (R10a/b/c) and the night-time
  proxy (R12) match thousands of TabFormer transactions with ~1 % precision and push the default-threshold FPR from
  23 % to 44 %; they belong in shadow mode or at low weight. Behavioural rules are precise (e.g. R11 amount z-score
  9 %, R03 card-testing error codes 6 %, R09 > ETV 500 8 % on a 0.7 % base rate; AMLworld R39 16 %, R11 27 % on a 6 %
  base rate; RBA R16 new device + unusual country 7.8 % on a 0.15 % base rate).
* **Thresholds must be calibrated per project:** at the default 50 the FPR ranges from 9 % (ULB) to 53 % (AMLworld);
  at FPR ≤ 5 % recall is 64 % (TabFormer), 83 % (AMLworld), 60–63 % (RBA) and 88 % (ULB).
* **E-commerce (Fraud_Data) is hard under a chronological split:** all 6,021 frauds with a 1-second sign-up →
  purchase gap fall in the history window; the scored 30 % has one purchase per user and no velocity history, so
  fused AUC is 0.61. A random split would look far better and would be misleading.

**Caveats (from the source):** four of five datasets are synthetic (RBA synthesized from real logs; Fraud_Data of
unknown origin); only ULB is real and it has no entity identifiers, so it tests only the ML engines. Samples are
stratified (fraud users/accounts over-represented), so absolute precision is higher than at natural prevalence; AUC
is prevalence-independent. Engine timeouts were raised for evaluation and latency was not evaluated. Runs interrupted
during label posting were resumed without re-posting (e-commerce seed 1 has 3.4–4.6k labels instead of ~12k), and the
effective supervised training set of ULB differs between seeds (3.8k–25.6k rows). Fraud_Data's licence is unknown:
use the e-commerce numbers internally only.

**Reproduce (on branch `eval/public-datasets`, `tools/public_eval/README.md`):** download the datasets
(`~/datasets/fraud-public/download.sh`), start the separate evaluation stack (`docker compose -p fraud-eval
--env-file ~/fraud-eval/.env.eval -f docker-compose.yml -f ~/fraud-eval/compose.eval.yml up -d --no-build …`), then
in `tools/public_eval`: `python -m public_eval run <tabformer|ecom|aml|rba> --seed <1..3> --config <tpl|lit>`,
`python -m public_eval run ulb --seed <s> --config ml`, and `python -m public_eval.aggregate` (writes
`results-public.json` and `evaluation-public-tables.md` to `$EVAL_WORK_DIR`). Runs are resumable.
