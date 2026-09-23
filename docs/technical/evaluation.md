# Detection quality — end-to-end evaluation

Last run: 2026-09-24, full docker-compose stack, all engines active (rules from stage templates, supervised
`mlp_backprop`, unsupervised `isolation_forest` + `hdbscan`, graph). This document explains how to reproduce the
numbers and how to read them.

## 1. Method

1. **History (training) run:** simulator seed 42, 300 customers, 30 days, 4 projects (checkout, post-payment,
   returns, promo). 70% of the window was ingested `load_only` and the rest scored. Partial, delayed labels were
   posted (60% of fraud events, 3% of legit).
2. **Training:** per project, the supervised model uses label maturity of 14 days (older unlabelled events count as
   legit) and a time-based validation split. The unsupervised model is trained on the last 90 days. Both models were
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

```bash
docker compose --profile seed run --rm -e SIM_CUSTOMERS=300 -e SIM_DAYS=30 simulator            # history
# train + approve models per project (UI: ML pages, or API: /ml/{supervised,unsupervised}/train → submit → approve)
docker compose --profile seed run --rm -v "$PWD/eval:/out" \
  -e SIM_CUSTOMERS=300 -e SIM_DAYS=10 -e SIM_SEED=7 -e SIM_RUN_TAG=E1 -e SIM_LOAD_ONLY_SHARE=0 \
  -e SIM_TRUTH_OUT=/out/truth_E1.jsonl simulator run --force                                   # unseen traffic
# join eval/truth_E1.jsonl with core.decisions (events whose external_id contains "-E1-")
```
