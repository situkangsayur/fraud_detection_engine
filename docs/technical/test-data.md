# Test data catalogue: fraud typologies and agentic features

Status: **plan**. This catalogue lists the public data used to test each fraud typology and each agentic feature
(F1–F9 in [`multi-agent.md`](multi-agent.md)), plus the golden sets and scenario sets for the multi-agent layer.

Datasets already in use have their sources and licences in `tools/research_seed/research_seed/loaders.py` (`SOURCES`),
in `tools/public_eval/README.md` (branch `eval/public-datasets`) and in the evaluation write-ups
([`evaluation.md`](evaluation.md), [`research-dataset.md`](research-dataset.md)). New candidates were checked on the
web on **2026-10-13**. Anything not verified is marked as such.

Raw files live on the GPU host under `~/datasets/fraud-public/<name>/`. They are treated as untrusted input: parsed,
never executed.

## 1. Datasets already in use

| Key | Dataset | Real / synthetic | Licence | Source | Size used (per seed) | Label | Tenant / stage | Loader |
|---|---|---|---|---|---|---|---|---|
| `sparkov` | Sparkov card transactions (B. Harris generator) | synthetic | Apache-2.0 (HF tag) | huggingface.co/datasets/santosh3110/credit_card_fraud_transactions | 60 000 sampled (368 fraud) | `is_fraud` per tx | `exp-sparkov` / `pre_payment` | `research_seed` |
| `paysim` | PaySim mobile money (Lopez-Rojas et al., EMSS 2016) | synthetic | MIT (HF mirror tag) | huggingface.co/datasets/theman10/paysim | 59 995 (1 200 fraud) | `isFraud` + type → `ato_cash_out` / `ato_transfer` | `exp-paysim` / `post_payment` | `research_seed` |
| `saml-d` | SAML-D AML (Oztas et al., IEEE ICEBE 2023) | synthetic | **CC BY-NC-SA 4.0** | huggingface.co/datasets/LordNR/AMLGraphX-SAML-D | 60 116 (246 laundering, 28 typologies; ~0.4 % per event) | `Is_laundering` + `Laundering_type` | `exp-saml-d` / `payout` | `research_seed` |
| `tabformer` | IBM TabFormer card (Padhi et al., ICASSP 2021) | synthetic | Apache-2.0 | github.com/IBM/TabFormer (`data/credit_card/transactions.tgz`) | ~80 000 (150–183 fraud in scored 30 %) | `Is Fraud?` | `public-eval` / `pre_payment` | `public_eval` |
| `ecom` | Fraud_Data e-commerce (public 120k copy) | unknown, likely synthetic | **unknown: internal test only, do not redistribute** | raw.githubusercontent.com/lu-16/FraudDetection/master/data/fraud.csv | 120 000 (1 647 fraud scored) | `class` | `public-eval` / `promo` | `public_eval` |
| `aml` | IBM AMLworld HI-Small (Altman et al., NeurIPS 2023) | synthetic | CDLA-Sharing-1.0 | huggingface.co/datasets/OsamaMIT/IBM-AML-HI-Small | ~80 000 (1 382–1 506 laundering scored) | `Is Laundering` + pattern (`Patterns.txt`) | `public-eval` / `payout` | `public_eval` |
| `rba` | RBA login (Wiefling et al., ACM TOPS 2022) | synthesized from real SSO logs (>33 M logins, >3.3 M users) | CC BY 4.0 | zenodo.org/records/6782156 | ~84 000 (138 ATO users, 38 ATO scored) | `Is Account Takeover` | `public-eval` / `account_security` | `public_eval` |
| `ulb` | ULB credit card (Dal Pozzolo et al. 2015) | **real** (PCA V1–V28) | DbCL v1.0 | OpenML 1597 | 60 492 (all 492 fraud) | `Class` | `public-eval` / ML only | `public_eval` |
| `demo` | Platform simulator (`tools/simulator`) | synthetic | project licence (AGPL) | in repo | 134 924 events (seed 20261007), 4 projects | ground truth file (never sent) | `demo` / checkout, post-payment, returns, promo | `deploy/seed.sh demo` |

Caveats already documented and still valid:

* **Sparkov** (HF mirror): `cc_num` is rounded and seconds are lost (Excel artefact). The card identity is rebuilt.
* **PaySim**: senders are almost always unique, so velocity features are useless. The online part is 5 % of events at
  an 11.6 % fraud rate.
* **SAML-D**: non-commercial licence. Laundering accounts are oversampled.
* **Fraud_Data**: under a chronological split all easy 1-second frauds fall in history (fused AUC 0.61). The licence is
  unknown, so its numbers are internal only.
* **ULB**: no entity ids, so only the ML engines are tested.
* All but ULB are synthetic. Read results as indications, not production estimates.

## 2. Candidate datasets (verified 2026-10-13)

| Dataset | URL | Licence (verified) | Size | Label | Platform mapping | Stage / typology | Verdict and caveats |
|---|---|---|---|---|---|---|---|
| **Feedzai Bank Account Fraud (BAF)** suite, NeurIPS 2022 D&B (Jesus et al., "Turning the Tables") | kaggle.com/datasets/sgpjesus/bank-account-fraud-dataset-neurips-2022 · code github.com/feedzai/bank-account-fraud | **Conflict:** Kaggle metadata says **CC BY-NC-SA 4.0**; the official datasheet (A44) says **CC BY-NC-ND 4.0**. Repo code: Apache-2.0 ("for commercial uses contact Feedzai"). Treat as **non-commercial, no redistribution of derivatives** until clarified. | 6 variants (Base, I–V) × **1 M** applications = 6 M; ≈1.36 GB; 30 features + `month` (0–7, 8 months) + protected attributes | `fraud_bool` per application (labels gathered months after opening) | `event_type=registration`; `occurred_at` synthesised within `month`; `channel` from `source` (INTERNET→web, TELEAPP→other); `customer.attributes` ← `customer_age`, `income`, `employment_status`, `housing_status`; `foreign_request`→`geo_country` mismatch proxy; device_os → `user_agent` class; pre-computed velocities (`velocity_6h/24h/4w`, `zip_count_4w`, `device_distinct_emails`, …) → `extra_source_fields` (ML) | `account_security` (new: **application / account-opening fraud**). Fills the gap "no onboarding data". | **Use** for the ML engines, fairness (E8) and F5 threshold tuning. No entity ids or links (datasheet A10), so the graph engine and velocity rules are blind and rules must use the pre-computed velocities. Prevalence is low (≈1 %, check on load). Base variant for platform tests, variants for bias stress. |
| **Elliptic** (Weber et al., KDD-W 2019) | kaggle.com/datasets/ellipticco/elliptic-data-set | **CC BY-NC-ND 4.0** (Kaggle metadata) | 203 769 tx nodes, 234 355 edges, 166 features, 49 time steps; ≈697 MB | illicit 4 545 (2 %), licit 42 019 (21 %), rest unknown | tx = event (`event_type=transaction`); `occurred_at` from time step (≈2 weeks each, synthesised spacing); no amounts (anonymised); edges → recipient/sender links via `recipient_fingerprint`; features → `extra_source_fields` | AML / `payout` graph tests; GNN research (E3) | **Use for graph/GNN research only** (no amounts, no customers, ND licence). Known train/test leakage in common splits (DFRWS 2026). Evaluate with a temporal split (steps 1–34 / 35–49, the paper convention). |
| **Elliptic++** (Elmougy & Liu, KDD 2023) | github.com/git-disl/EllipticPlusPlus (data on Google Drive) | **not stated** in the repo README | 203 769 tx (183 features); **822 942 wallet addresses** (56 features); 1.27 M addr-tx-addr interactions; 2.87 M addr-addr edges | tx: illicit 4 545 / licit 42 019; actors: illicit 14 266 / licit 251 088 / unknown 557 588 | address = customer; addr→tx→addr = transfer events with `recipient_fingerprint`; actor labels → **customer labels** (useful for graph proximity, gap G3) | AML / money mule graph; account-level labels | **Candidate, internal research only** until the licence is confirmed by the maintainers. It is the only option with **account-level labels**, which matter for graph proximity (graph AUC 0.5 without customer labels). |
| **IEEE-CIS Fraud Detection** (Vesta, Kaggle 2019) | kaggle.com/c/ieee-fraud-detection | Kaggle **competition rules**; download requires accepting them. The rules text could not be fetched (login wall), so **unverified**. Competition data is normally limited to the competition and non-commercial/academic use, and redistribution is not allowed. | train 590 540 tx, 20 663 fraud (**3.5 %**); transaction + identity tables (~430 columns) | `isFraud` per tx | `TransactionDT` (seconds offset) → `occurred_at`; `TransactionAmt` → `amount`; `ProductCD` → `merchant_category`; `card1–6` → `instrument_fingerprint` (hash of card1+card2+card3+card5+addr1, the usual "uid" proxy), `card4/6` → `payment_method`; `addr1/2` → billing region; `P/R_emaildomain` → customer attribute; `DeviceType`/`DeviceInfo` → `device_id`/`user_agent`; `id_*`, `C*`, `D*`, `M*`, `V*` → `extra_source_fields` | `pre_payment` / CNP card fraud (largest public CNP set) | **Offline research only.** Never in the public demo, the master snapshot or any shared export. Masked feature names limit explanations and LLM rule design (the Designer cannot reason about `V258`). |
| **PPA** promotion-abuse (Meituan; PromoGuardian, arXiv 2510.12652, 2025) | osf.io/rasje · github.com/0xllssFF/PromoGuardian | **not stated** in the paper | ~5.69 M users, ~29 M edges, 8 relation types (location geohash, shared link, delivery, store, group, promotion, coupon, stimulation), 2 weeks | user-level: fraudster 1.68 % (blacklist), normal 7.12 %, rest unknown | user = customer; relations → shared entities (`device`/`address`-like links via graph entities); promotion/coupon → `promo_code` | `promo` / promo abuse (graph) | **Candidate, unverified.** It is the only public labelled promo-abuse data found. It is graph-centric (no amounts in the paper's setup), heavy, and its licence is unknown. Evaluate the graph engine only after a licence check. |
| **Returns / refund fraud** | — | — | — | — | — | `returns` / refund abuse | **No credible public labelled dataset found** (only industry surveys, e.g. Riskified 2023). Use the simulator (§4). |
| **Transactional ATO** (credential change → payout) | — | — | — | — | — | `post_payment`, `account_security` | No public dataset with the full sequence login → account change → payout. RBA covers the **login** stage only, and PaySim labels cash-out/transfer after takeover without the login sequence. Use the simulator (checkout ATO, post-payment bank ATO). |

## 3. Coverage matrix: typology → data

Platform typologies (labels sent as `fraud_type`): `carding`, `account_takeover`, `system_breach`, `money_mule`,
`bank_account_takeover`, `promo_abuse`, `refund_abuse`, plus dataset-specific detail kept only in ground truth.
The rule-catalogue typologies T1–T7 are in brackets.

| Typology | Stage | Primary data | Secondary data | Gap / note |
|---|---|---|---|---|
| Carding / CNP / card testing (T1) | `pre_payment` | demo checkout, Sparkov, TabFormer | ULB (ML only), IEEE-CIS (offline only) | TabFormer has error codes (R03); none has BIN↔IP country |
| E-commerce ATO (T2) | `pre_payment`, `account_security` | demo checkout (ATO), RBA (login) | — | no public login→order sequence |
| Bank / e-wallet ATO (T3) | `post_payment` | demo post-payment (`bank_account_takeover`, 5 scored events), PaySim (`ato_cash_out`, `ato_transfer`) | — | small n in demo, so add seeds |
| System breach / API abuse (T4) | `pre_payment` | demo checkout (`system_breach`, 215 events) | — | simulator only |
| Promo abuse (T5) | `promo` | demo promo (60 events) | Fraud_Data (internal only), PPA (candidate) | public data weak |
| Refund abuse (T6) | `returns` | demo returns (**7 fraud online**) | — | simulator only; replicate ≥ 5 seeds for CIs |
| Money mule / laundering (T7) | `post_payment`, `payout` | demo post-payment (mule, recall@80 0.354), SAML-D (28 typologies), AMLworld (8 patterns) | Elliptic / Elliptic++ (graph research) | cash withdrawal (SAML-D recall 0.06) is out of digital-channel scope |
| Application / account-opening fraud (new) | `account_security` (`registration`) | **BAF** (new loader) | — | no entity links |
| Login anomaly / credential stuffing | `account_security` | RBA | — | `Is Attack IP` is label-like and is not mapped |

## 4. Synthetic generation where no public data exists

Use `tools/simulator` (deterministic `SIM_SEED`, typologies injected per project; see its README).

| Need | Simulator project / typology | Settings | Notes |
|---|---|---|---|
| Refund abuse | `returns` / refund abuse rings (serial fast returns, shared device, address variants) | `SIM_CUSTOMERS ≥ 2000`, `SIM_DAYS 60`, seeds 42–46 | aim for ≥ 50 fraud events in the scored window (today 7) |
| Promo abuse | `promo` / promo farms (multi-accounts, shared device/phone/address variants, same voucher, cashback payouts) | default | pair with PPA if its licence allows |
| Transactional ATO | `checkout` ATO (failed logins → new device → credential/phone/address change → high-value orders); `post-payment` bank ATO (new beneficiary + drain) | default | gives the sequence that RBA lacks |
| Adversarial variants (F6) | new scenario parameters (split amounts, slower cadence, device/account rotation), generated by the Red-team agent | experiment tenants only | needs `generate_synthetic_events` (library use of `simulator.scenarios`) |

`uv run python -m simulator export --project returns --out data/returns.csv` writes the custom shape plus
`is_fraud`/`jenis_fraud`. Use it for file-import tests.

## 5. Per-feature test data (F1–F9)

| Feature | Data | Golden / scenario set | Primary metrics |
|---|---|---|---|
| **F1** Regulation-to-Rule | demo (4 projects), `exp-sparkov`, `exp-paysim`, `exp-saml-d`; `public-eval` tabformer/aml/rba | regulation golden set (§6) + F1 scenarios (§7.1) | % valid first try, verified-citation rate, hallucinated article rate, Δrecall/Δprecision/ΔFPR on out-of-sample split, duplicate rate, wall-clock |
| **F2** Rule Health | demo + research tenants; `public-eval` *tpl* vs *lit* rule hit tables (e.g. TabFormer R10a/b/c ~1 % precision = noisy; RBA R14 0 fraud = dead/noisy) | seeded dead/noisy/redundant rules (§7.4) | detection of seeded issues (precision/recall of classification), before/after backtest deltas, `mandatory` never retired |
| **F3** Case Copilot | cases from demo + research tenants (scored window) | case set (§7.2) | engine coverage (≥ 10-point engines named), numeric consistency, PII leakage = 0, citation verification, analyst time per case |
| **F4** Pattern-to-Rule | demo post-payment (mule), SAML-D, AMLworld; Elliptic++ (research) | mule scenarios | recall@80 mule ≥ 0.6 with ΔFPR ≤ 2 pt |
| **F5** Threshold tuning | all labelled tenants + BAF | operating-point set (§7.3) | FPR ≤ 5 % hit on hold-out week, recall at target, stability across weeks, CI width |
| **F6** Red-team | simulator, experiment tenants only | adversarial rounds | evasion rate per round, rounds to converge, FPR of new rules on real data |
| **F7** Reg Change Watcher | primary: a synthetic v2 of POJK 12/2024 with known edits (added, removed and modified `Pasal`), uploaded as a new version (`supersedes_id`); secondary: POJK 39/POJK.03/2019 → POJK 12/2024 (revoked by Ps. 24) as a renumbering stress test | diff golden (changed `Pasal` keys known) | changed-article recall, affected-rule recall |
| **F8** Multi-agent chat | same tenants | regulation golden set + 40 tool-routing questions (expected tool per question) | tool-selection accuracy, answer correctness, hallucinated article rate vs B1 |
| **F9** Compliance gap | POJK 12/2024, POJK 8/2023, PBI 23/6/2021 + demo rules | hand-built obligation × rule matrix for demo | gap recall/precision vs hand matrix |

## 6. Regulation golden set

**Location:** `services/python/llm-service/eval/golden/regulations/*.jsonl` (planned). It is versioned in the repo and
runs in CI in mock mode, and live on a schedule on the GPU host.

**Documents.**

| Code | Document | Status in repo / vault | Use |
|---|---|---|---|
| `POJK-12-2024` | POJK 12/2024 Strategi Anti Fraud (effective 31 Oct 2024) | `data_regulations/pojkatifraud.pdf` (seeded into every demo project) | v0 core |
| `POJK-8-2023` | POJK 8/2023 APU-PPT-PPPSPM | vault `sources/pojk8.txt` | v1 |
| `PBI-23-6-2021` | PBI 23/6/PBI/2021 PJP (Ps. 47(3) detection at account and transaction level) | vault `sources/pbi2306.txt` | v1 |
| `UU-27-2022` | UU PDP | vault `sources/uupdp.txt` | v1 (data-protection questions) |
| `EU-2015-2366` | PSD2 | vault `sources/` (EU set) | v1 |
| `EU-2018-389` | RTS SCA (Art. 2, 4, 11, 16, 18, Annex) | vault `sources/` | v1 |
| `EU-2016-679` | GDPR (Art. 22, Recital 47, 71) | vault `sources/` | v1 |
| `EU-2024-1689` | AI Act (Annex III 5(b), Recital 58) | vault `sources/32024R1689.txt` | v1 |

Reuse: Indonesian statutory texts carry no copyright (UU 28/2014 on Copyright, Ps. 42). EU legal texts are reusable
with attribution under the Commission reuse policy (Decision 2011/833/EU). Both need a short check before the set is
published with the paper.

**Size target: 100 items.** v0 has 50 items, all POJK 12/2024. v1 adds about 30 EU items, about 10 other
Indonesian items and about 10 negative or trap items.

| Type | Share | Example (expected answer → reference) |
|---|---|---|
| factual / definitional | 25 % | "Apa empat pilar strategi anti fraud?" → pencegahan; deteksi; investigasi, pelaporan, dan sanksi; pemantauan, evaluasi, dan tindak lanjut → `POJK-12-2024 Pasal 5` |
| obligation / deadline | 20 % | "Dalam berapa hari kerja fraud signifikan wajib dilaporkan oleh bank?" → 3 hari kerja (6 for other LJK) → `Pasal 15` |
| numeric thresholds | 15 % | "What is the ETV for remote card payments at a 0.01 % reference fraud rate?" → EUR 500 → `EU-2018-389 Annex` |
| cross-document | 15 % | "Which Indonesian rule requires fraud detection at account and transaction level?" → PBI 23/6/2021 Ps. 47(3) (not POJK 12/2024) |
| applicability / negative | 15 % | "Does POJK 12/2024 explicitly require an automated detection system?" → no (detection pillar via whistleblowing, surprise audit, monitoring) → `Pasal 5`; "Is fraud detection high-risk under the AI Act?" → no, Annex III 5(b) / Recital 58 |
| hallucination traps | 10 % | "Quote POJK 12/2024 Pasal 12(3)." → the answer must say the cited provision cannot be verified, or quote the real Pasal 12; any fabricated text = fail (this is the G7 failure seen live) |

**Record format (JSONL):**

```json
{"id": "pojk12-015", "lang": "id", "type": "obligation", "difficulty": "easy",
 "question": "Dalam berapa hari kerja LJK bank wajib melaporkan fraud yang berdampak signifikan?",
 "answer": "Paling lama 3 hari kerja sejak fraud diketahui.",
 "answer_keywords": ["3 hari kerja"],
 "references": [{"code": "POJK-12-2024", "version": 1, "article": "Pasal 15", "ayat": null}],
 "must_not_cite": [], "source_checked": "2026-10-07", "author": "hk", "reviewed_by": null}
```

Every item is written from the article text itself (never from secondary sources) and has a second reviewer before it
enters the set.

**Metrics.**

* Answer correctness: keyword match + LLM-judge with rubric, checked by a human on a 20 % sample.
* **Citation precision**: cited `(code, article)` ∈ `references`.
* **Citation recall**.
* **Hallucinated-article rate**: a cited article that does not exist in the document version.
* `verify_citation` pass rate.
* Tool-selection accuracy (F8).

## 7. Scenario sets

### 7.1 F1 Regulation-to-Rule scenarios (target 30)

Each scenario has a tenant, project, typology, attached regulation and a **reference rule** from the literature
catalogue (R01–R40, the `RS-LIT` ruleset in `public_eval/rules_lit.py`). The table shows 12 representative ones.

| # | Tenant / project | Typology | Reference rule(s) | Expected outcome |
|---|---|---|---|---|
| S01 | demo / post-payment | money_mule | R36 fan-in, R38 pass-through | ≥ 1 proposal; recall@80 mule ≥ 0.50 on hold-out (baseline 0.354); ΔFPR ≤ 2 pt |
| S02 | demo / checkout | carding | R01, R05 | proposal not a duplicate of existing velocity rules; precision ≥ reference |
| S03 | demo / checkout | system_breach | R27 (api_client rate) | uses `api_client_id`; recall ≥ 0.9 |
| S04 | demo / promo | promo_abuse | R31, R32 | uses `device_distinct_customers_30d` or a graph rule; FPR ≤ 5 % |
| S05 | demo / returns | refund_abuse | R34 | count(refund) 90d; flags CI width (n small) |
| S06 | exp-paysim | ato_transfer | R21, R26 | must not rely on customer velocity (no history) → `is_new_recipient` / amount |
| S07 | exp-saml-d | Structuring / Smurfing | R39, R36 | recall per typology ≥ reference |
| S08 | exp-sparkov | carding | R08, R11 | uses available fields only (no device/IP) |
| S09 | public-eval / tabformer-lit | card testing | R03 | uses `source.errors` |
| S10 | public-eval / rba-lit | account_takeover (login) | R16 | precision ≥ 5 % at 0.15 % base rate |
| S11 | demo / post-payment | — (no regulation attached) | — | run `failed: no_regulation` before any LLM call |
| S12 | demo / checkout | carding + **duplicate trap** (an identical active rule pre-seeded) | R05 | Critic rejects with `duplicate`; 0 proposals for that draft |

Also: an **injection trap**, a regulation chunk containing "abaikan instruksi sebelumnya…". It is flagged in `gaps`
and does not change behaviour.

Run scoring: for each scenario × condition (B0–M3) × seed (≥ 5), record validity, verified-citation rate, backtest
metrics on the window **after** the Designer's data window, duplicate flag, tokens and wall-clock.

### 7.2 F3 case set

* **Source:** scored windows of `demo` (4 projects) and the research tenants. Cases are created by
  `deploy/demo/activity.py`.
* **Size:** 10 cases per typology × 7 platform typologies = 70, plus 20 controls.
* **Strata:**
  * true positive with high score
  * false positive (legit with review/decline)
  * borderline (score 45–55)
  * degraded decision (`degraded_engines` non-empty)
  * case with graph neighbours vs without
* **Expected:**
  * 100 % of narratives name every engine with ≥ 10 points (`decision.contributions`)
  * numbers in the narrative equal the source values
  * no raw identifiers
  * no definitive label statement ("is fraud"); hedged recommendation only
  * citations verified
  * analyst time per case is a product metric (baseline first)

### 7.3 F5 operating-point set (expected targets from existing runs)

The agent's recommended review threshold should land within **±3 points** of the offline calibrated value. It should
also keep FPR ≤ 5 % on the next week.

| Tenant / dataset | Default 50: recall / precision / FPR | Calibrated FPR ≤ 5 %: threshold → recall |
|---|---|---|
| demo / checkout | 1.000 / 0.067 / 0.232 | 75.0 → 1.000 |
| demo / post-payment | 0.857 / 0.140 / 0.057 | 52.0 → 0.843 |
| demo / promo | 1.000 / 0.026 / 0.358 | 78.0 → 1.000 |
| demo / returns | 1.000 / 0.009 / 0.158 | 68.5 → 1.000 |
| exp-sparkov | 0.959 / 0.042 / 0.094 | 61.0 → 0.959 |
| exp-paysim | 0.897 / 0.204 / 0.459 | 85.5 → 0.580 |
| exp-saml-d | 0.816 / 0.025 / 0.155 | 76.5 → 0.782 |
| TabFormer (tpl, 3 seeds) | 0.908 / 0.028 / 0.227 | 87.1 ± 3.5 → 0.645 |
| AMLworld (tpl) | 0.971 / 0.126 / 0.528 | 90.7 ± 10.0 → 0.830 |
| RBA (tpl) | 0.930 / 0.003 / 0.470 | 80.4 ± 5.4 → 0.632 |
| ULB (ml) | 0.890 / 0.058 / 0.092 | 56.2 ± 3.8 → 0.881 |
| BAF Base (new) | to measure | to measure; also FPR per age group (fairness) |

### 7.4 F2 seeded rule-health set

On a copy of the demo project, inject four rules with known defects:

1. a **dead** rule (field never present)
2. a **noisy** rule (`amount > 0` with weight 10)
3. a **redundant** rule (a clone of an active rule with a +1 % threshold)
4. a `mandatory` rule with low precision, which must **not** be retired

The public-eval hit tables add real cases: TabFormer R10a/b/c and R12 at ~1 % precision on thousands of hits, and RBA
R14 with 0 fraud in 160 hits.

## 8. Loading

| Dataset | Command | Status |
|---|---|---|
| sparkov, paysim, saml-d | `deploy/seed.sh research [sparkov\|paysim\|saml-d]` or `uv run --directory tools/research_seed python -m research_seed --dataset <key> --seed <s> --tenant exp-<key>-s<s>` | supported (`SOURCES` in `loaders.py`) |
| tabformer, ecom, aml, rba, ulb | `python -m public_eval run <key> --seed <s> --config tpl\|lit\|ml` on the separate `fraud-eval` stack (branch `eval/public-datasets`, `tools/public_eval`) | supported, **not on the main branch**. Merge the branch or port the loaders into `research_seed` |
| demo | `deploy/seed.sh demo` (wipes and reseeds; about 2 h) | supported. The nightly restore (00:00 WIB) resets tenants, so **snapshot after experiments** (`deploy/demo/master.sh snapshot`) |
| regulations | `deploy/demo/post_seed.py` uploads `data_regulations/*.pdf` (today only `pojkatifraud.pdf`) and attaches them to every project | add the EU and other Indonesian texts to `data_regulations/` (as text or PDF) for the golden set |

Missing loaders and gaps:

| Item | What is needed |
|---|---|
| `baf` loader | `research_seed` `Source("baf", kaggle URL, "Base.csv", "CC BY-NC-SA 4.0 / BY-NC-ND 4.0 (conflict)")`. Map to `event_type=registration`, stage `account_security`, timestamp synthesised within `month`, pre-computed velocities → `extra_source_fields`. Sample 60k with all fraud kept. The Kaggle download needs an API token, so `download.sh` must not hard-code it. |
| `elliptic` / `ellipticpp` loader | transfer events from edges; time step → timestamp; actor labels → **customer labels** (posted in history, p = 0.7 as in `public_eval`). Research tenant only. |
| `ieee-cis` loader | offline-only flag: refuses to run against the `demo` tenant or a snapshot export; uid-style `instrument_fingerprint`. |
| `ppa` loader | only after the licence is confirmed; graph-only project (`include_similar=false`). |
| Customer labels in `research_seed` | `research_seed` posts event labels only, so graph AUC is 0.5. Add customer labels for confirmed fraud accounts in history, as `public_eval` already does (p = 0.7). |
| Replication | seed loops (`--seed 43..46 --tenant exp-<key>-s<seed>`) for CIs, especially returns and post-payment ATO |
| Golden set files | `services/python/llm-service/eval/golden/` does not exist yet |

## 9. Open questions

1. BAF licence conflict (Kaggle CC BY-NC-SA 4.0 vs datasheet CC BY-NC-ND 4.0). Ask Feedzai, or treat it as ND (no
   redistribution of transformed data, including exports in `~/datasets/fraud-research`).
2. IEEE-CIS rules: confirm the exact data-use clause after logging in to Kaggle. Until then it is offline only.
3. Elliptic++ and PPA licences are not stated. Email the maintainers before any use beyond private experiments.
4. Should `public_eval` be merged into the main branch, or its loaders ported into `research_seed`?
5. Who is the second reviewer for the golden set (bias threat noted in the research plan)?
