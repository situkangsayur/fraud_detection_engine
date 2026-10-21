# Bank fraud datasets and reference cases

> Document status: research of 2026-10-21. Each dataset was checked against its source page (Kaggle metadata,
> Zenodo, figshare/PMC, Mendeley, GitHub, FCA, J.P. Morgan, arXiv). Anything that could not be verified is marked
> **unverified**. Datasets already wired into the platform are described in [`test-data.md`](test-data.md) §1–2.

Purpose: choose public, **referenced** datasets for bank / e-wallet fraud cases that the platform can demonstrate
(public demo) or evaluate (papers), and record each dataset's characteristics, licence and caveats in one place.

## 1. Summary: what to use for what

| Goal | Recommended | Why |
|---|---|---|
| Public demo for an Indonesian audience (redistributable) | **AMLSim / AMLworld** (mule graph) + **SynthAML** (alert triage) + **RBA** (login ATO) + **MoMTSim** (e-wallet ATO) | permissive or share-alike licences; covers graph, ML, rules and LLM |
| Academic evaluation, non-commercial | add BankSim, SAML-D, BAF, J.P. Morgan sets | peer-reviewed or datasheet-backed, NC licences |
| Research only (gated / unknown terms) | FCA APP, DGraph-Fin, Xente, IEEE-CIS, T-Finance, Elliptic2, KorCCVi | never in the public demo, master snapshot or shared exports |
| Avoid | Kaggle "Credit Card Fraud 2023", Indonesian SMS corpus (unverified) | unknown provenance or licence |

No public **check-fraud** dataset with a proper reference exists, no public dataset released by a Brazilian, Dutch or
Indian bank was found, and there is **no public transaction-level fraud dataset for Indonesia / Southeast Asia**
(only OJK IASC aggregate statistics). The closest "real bank" sources are SynthAML (synthesised from a Danish bank)
and the FCA APP dataset (regulator-sponsored, synthetic).

## 2. Dataset catalogue (new candidates)

| # | Dataset | Reference / URL | Licence (as listed) | Real / synthetic | Size & prevalence | Label grain | Key fields | Stage / typology | Engines | Caveats |
|---|---|---|---|---|---|---|---|---|---|---|
| 1 | **BankSim** | Lopez-Rojas & Axelsson, EMSS 2014 · kaggle.com/datasets/ealaxi/banksim1 | CC BY-NC-SA 4.0 | synthetic, calibrated on aggregated data of a Spanish bank | 594 643 tx, 7 200 fraud (1.21 %), 180 steps ≈ 6 months | transaction | customer, merchant, age, gender, category, amount, step | `pre_payment` card fraud | rules, ML, graph (customer–merchant) | non-commercial; coarse ages/categories; step = day |
| 2 | **SynthAML** (Spar Nord, DK) | Jensen et al., *Scientific Data* 10:661 (2023), doi:10.1038/s41597-023-02569-2 · figshare 10.6084/m9.figshare.c.6504421.v1 | CC BY 4.0 | synthetic (SDV), fitted on a real Danish bank's AML alerts 2020–21 | 20 000 alerts, > 16 M tx; ≈ 17 % alerts reported (inflated vs real banks) | alert (case) | alert id, date (quarter-accurate), entry, type (card/cash/intl/wire), standardised size | `payout` / AML alert triage | ML, LLM (triage narrative), rules | no client/counterparty ids → no graph; split on quarter boundaries; standardised amounts |
| 3 | **RBA login** (Telenor SSO, NO) | Wiefling et al., *ACM TOPS* 2022, doi:10.1145/3546069 · zenodo.org/record/6782156 | CC BY 4.0 | synthesised from real logs (≈ 33 M attempts, 3.3 M users, Feb 2020–Feb 2021) | ATO flag very rare (exact count not checked) | login attempt | user id, timestamp, IP, ASN, country, UA/OS/browser, device, RTT, success, is attack IP, is account takeover | `account_security` / login ATO | rules (geo-velocity, new device), unsupervised ML, ML | authors warn against production use; some values deliberately unrealistic |
| 4 | **FCA APP synthetic data** | FCA Digital Sandbox (Aizle / Smart Data Foundry, City of London Corp.), 2023 · fca.org.uk/firms/digital-sandbox/authorised-push-payment-synthetic-data | **unknown** (application via FCA Innovation platform) | synthetic (agent-based) | 37 tables, 15 M tx, 61 k attempted frauds, 20 k individuals, 4 banks + 2 telcos, 2 years | transaction / fraud event | identities, accounts, narratives, destination accounts, call & SMS metadata | `post_payment` **APP scams** (romance, investment, purchase, impersonation…) | rules, graph, ML, **LLM** | gated; likely not redistributable; no peer-reviewed paper (FCA evaluation report exists) |
| 5 | **MoMTSim** (Uganda mobile money) | Azamuke, Katarahweire & Bainomugisha, *Data in Brief* 2025, doi:10.1016/j.dib.2025.111534 · Mendeley 10.17632/zhj366m53p.2 | CC BY 4.0 | synthetic (agent-based, modelled on real processes) | v1 1.72 M tx (10.2 % fraud); v2 4.23 M (52.8 %) | transaction | step (hour), type, amount, initiator/recipient ids, balances before/after, isFraud | `post_payment` e-wallet: SIM-swap ATO, refund fraud, fake accounts | rules, graph, ML | v2 prevalence unrealistic; balance fields leak the label (drop them) |
| 6 | **DGraph-Fin** (Finvolution, CN) | Huang et al., NeurIPS 2022 D&B, arXiv:2207.03579 · dgraph.xinye.com | non-commercial research licence with registration (text **unverified**) | real (lending platform) | 3.7 M nodes, 4.3 M edges; 15 509 fraudsters / 1.23 M labelled (1.3 %) | account (node) | 17 masked features, emergency-contact edges with relative time | `account_security` / credit fraud | graph, GNN, unsupervised ML | "fraud" = long-overdue borrower; masked features; NC |
| 7 | **T-Finance** | Tang et al., ICML 2022 · github.com/squareRoot3/Rethinking-Anomaly-Detection | **unverified** | real tx network (source undisclosed) | 39 357 accounts, 4.58 % anomalous | account | 10 features, tx edges | mule / gambling accounts | graph | weak provenance |
| 8 | **Xente** (Uganda) | Zindi 2019 · zindi.africa/competitions/xente-fraud-detection-challenge | Zindi competition terms (**unverified**) | real e-commerce / financial app tx | ≈ 140 k tx, Nov 2018–Mar 2019; ≈ 0.2 % fraud (**unverified**) | transaction | ids, provider, product, channel, amount, time, FraudResult | `pre_payment` / wallet payments | rules, ML | no paper; redistribution restricted |
| 9 | **J.P. Morgan AI Research synthetic data** | Assefa et al., ICAIF 2020; Borrajo, Veloso & Shah, ICAIF 2020 (arXiv:2011.01826) · jpmorganchase.com/about/technology/research/ai/synthetic-data | non-commercial, per dataset, by email request | synthetic (planning-execution simulator) | not stated; configurable fraud probability | customer trace (AML), transaction (payments) | state-action traces; subject-centric payments; customer journeys (login, payment, ATM) | AML, payments fraud, journey-level ATO | ML, sequence models, LLM | gated; AML set lacks channel/account features |
| 10 | **IBM AMLSim** (generator) | Weber et al. arXiv:1812.00076 · github.com/IBM/AMLSim | Apache-2.0 (code) | synthetic generator | generate from 1 k accounts up | transaction + alert subgraph | accounts as nodes, tx as edges, patterns (fan-in/out, cycle, scatter-gather…) | AML / mule rings | graph, rules, ML | dated stack (Java 8 / Python 3.7); generated data is yours |
| 11 | **AMLGentex** | Östman et al., arXiv:2506.13989 (2025) | paper CC BY 4.0; code licence **unverified** | synthetic generator | not checked | tx / account | not checked | AML, partial observability | graph, ML | preprint |
| 12 | **Elliptic2** | Bellei et al., arXiv:2404.19109 (2024) · elliptic.co/elliptic2 | code Apache-2.0; data **unverified** | real Bitcoin | 122 k labelled subgraphs, 49 M clusters, 196 M edges | subgraph | cluster graph | AML / crypto off-ramp | graph | crypto, not bank; registration |
| 13 | **Berka / PKDD'99** (Czech bank) | PKDD'99 Discovery Challenge · relational.fel.cvut.cz/dataset/Financial | none stated | real (anonymised, 1993–98) | 8 tables, 1.09 M rows; 682 loans (76 bad) | loan | client, account, card, tx, district | **no fraud labels**: legitimate background or injected-fraud experiments | rules, unsupervised ML, graph | internal use only (licence unclear) |
| 14 | Kaggle "Credit Card Fraud Detection 2023" | kaggle.com/datasets/nelgiriyewithana/credit-card-fraud-detection-dataset-2023 | "Other", description gives none | claims "European cardholders 2023", no institution | > 550 k, ≈ 50/50 | transaction | V1–V28, Amount, Class | — | ML only | **avoid**: provenance unverifiable, likely resampled |
| 15 | Indonesian SMS (normal / penipuan / promo) | Rahmi & Wibisono (secondary literature) | **unverified** | real SMS | ≈ 1 143 messages, 3 classes | message | text, label | smishing (ID) | LLM / NLP | source not found; **avoid** until verified |
| 16 | **KorCCVi** (Korean vishing) | IEEE DataPort | CC BY-NC-SA 4.0 (snippet) | real call transcripts | 1 417 calls (706 vishing) | call | text, label | vishing / bank impersonation | LLM | Korean; NC |
| 17 | **Amazon FDB** (benchmark wrapper) | Grover et al., arXiv:2208.14417 · github.com/amazon-science/fraud-dataset-benchmark | MIT-0 (repo); each dataset keeps its own | mixed | 9 datasets | varies | standard splits | benchmarking harness | ML | inherited licences |

Already in the catalogue ([`test-data.md`](test-data.md)): PaySim (CC BY-SA 4.0), SAML-D (CC BY-NC-SA 4.0),
IBM AMLworld (CDLA-Sharing-1.0), IBM TabFormer (Apache-2.0), ULB (DbCL), Feedzai BAF (CC BY-NC-SA 4.0 per Kaggle;
datasheet says ND), Elliptic v1 (CC BY-NC-ND 4.0), IEEE-CIS (competition rules), Sparkov (CC0).

## 3. Characteristics that matter for this platform

| Characteristic | Why it matters | Datasets that have it |
|---|---|---|
| Customer / account ids | velocity features, case per customer, customer labels | BankSim, MoMTSim, RBA (user id), AMLSim, AMLworld, SAML-D, PaySim |
| Counterparty / shared entities (graph links) | graph engine (shared device, recipient, contact) | AMLSim, AMLworld, SAML-D, MoMTSim (recipient), DGraph-Fin, FCA APP |
| Real timestamps | time-based splits, daily replay in the demo | RBA, AMLworld, SAML-D, Xente; BankSim/PaySim/MoMTSim use steps |
| Text / narratives | LLM triage and explanation | FCA APP (narratives, call/SMS metadata), KorCCVi |
| Account-level labels | graph proximity (`distance_to_fraud`) | DGraph-Fin, T-Finance, Elliptic++, AMLSim alert accounts |
| Label leakage risk | inflated metrics | PaySim / MoMTSim balance fields; Elliptic common splits |
| Realistic prevalence (< 2 %) | thresholds, FPR targets | BankSim, RBA, AMLworld, SAML-D, DGraph-Fin |

## 4. Reference cases (demo and research scenarios)

| # | Case | Dataset | Typology | What the platform shows | Metric (reference) |
|---|---|---|---|---|---|
| C1 | Mule ring via fan-in / fan-out | AMLSim (generated) or AMLworld | money mule / layering (FATF–Egmont–INTERPOL 2023) | graph explorer reveals cycles and scatter-gather; LLM writes the case narrative | subgraph recall; F1 on illicit tx vs Altman et al. GNN baselines |
| C2 | Structuring / smurfing rule | SAML-D | structuring, smurfing | LLM drafts a threshold-avoidance rule citing POJK 12/2024, validated and backtested, then a proposal | precision/recall per typology vs Oztas et al. |
| C3 | AML alert triage | SynthAML | AML alerts | ML ranks alerts; LLM explains "report vs dismiss" | AUC / PR-AUC with quarter splits (Jensen et al.) |
| C4 | Login account takeover | RBA | ATO (login) | rules: new country/ASN + new device; unsupervised risk score | ATO recall at a fixed re-auth rate (Wiefling et al.) |
| C5 | Account opening / synthetic identity | BAF (catalogue) | application fraud | ML + fairness check per age group | recall at 5 % FPR + predictive equality (Jesus et al.) |
| C6 | APP scam / bank impersonation | FCA APP (research only) + KorCCVi | APP scams, vishing | call metadata → first-time payee → high-value transfer rule; LLM classifies narratives (IASC scenario in Indonesia) | detection rate per scam type |
| C7 | Mobile-money SIM-swap ATO | MoMTSim v1 | e-wallet ATO | rules "drain after new device/SIM"; drop balance fields | vs XGBoost MCC 0.82 / AUC 0.97 (Azamuke et al., AFRICOMM) |
| C8 | Card-theft bursts | BankSim | card fraud | velocity rules + customer–merchant graph | vs Lopez-Rojas results |
| C9 | Fraud-graph GNN benchmark (research) | DGraph-Fin, T-Finance | graph anomaly | unsupervised / GNN anomaly detection | AUC vs BWGNN / DGraph baselines |

Loading follows [`test-data.md`](test-data.md) §8 (`research_seed` / `public_eval`): one experiment tenant per
dataset, a mapping from the dataset's own columns (see [`data-sources.md`](data-sources.md) §3), and ground-truth
labels written before seeding.

## 5. Licence guidance for this project

* **Safe to host and redistribute in the public demo:** SynthAML, RBA, MoMTSim (CC BY 4.0); PaySim (CC BY-SA, derived
  data share-alike); Sparkov (CC0); ULB (DbCL, attribution); AMLworld (CDLA-Sharing, share-alike); data generated
  with AMLSim.
* **Non-commercial (academic / free demo, not commercial):** BankSim, SAML-D, BAF, J.P. Morgan sets. Elliptic v1 is ND,
  so derived datasets cannot be published.
* **Research only, never redistributed:** FCA APP, DGraph-Fin, Xente, IEEE-CIS, T-Finance, Elliptic2, KorCCVi.
* **Avoid:** Kaggle 2023 card set, Indonesian SMS corpus; Berka for internal background only.

## 6. References

- Altman, E., et al. (2023). Realistic Synthetic Financial Transactions for Anti-Money Laundering Models. *NeurIPS 2023 D&B*. arXiv:2306.16424.
- Assefa, S., et al. (2020). Generating Synthetic Data in Finance: Opportunities, Challenges and Pitfalls. *ICAIF 2020*.
- Azamuke, D., Katarahweire, M., & Bainomugisha, E. (2025). A labeled synthetic mobile money transaction dataset. *Data in Brief*. doi:10.1016/j.dib.2025.111534.
- Azamuke, D., et al. (2025). Financial Fraud Detection Using Rich Mobile Money Transaction Datasets. *AFRICOMM*. doi:10.1007/978-3-031-81573-7_16.
- Bellei, C., et al. (2024). The Shape of Money Laundering: Subgraph Representation Learning on the Blockchain with the Elliptic2 Dataset. arXiv:2404.19109.
- Borrajo, D., Veloso, M., & Shah, S. (2020). Simulating and classifying behavior in adversarial environments based on action-state traces: an application to money laundering. *ICAIF 2020*. arXiv:2011.01826.
- Dal Pozzolo, A., et al. (2015). Calibrating Probability with Undersampling for Unbalanced Classification. *IEEE SSCI*.
- FATF, Egmont Group & INTERPOL (2023). *Illicit Financial Flows from Cyber-Enabled Fraud*.
- Financial Conduct Authority (2023–25). Authorised Push Payment synthetic data; APP fraud dataset evaluation report.
- Grover, P., et al. (2023). Fraud Dataset Benchmark and Applications. arXiv:2208.14417.
- Huang, X., et al. (2022). DGraph: A Large-Scale Financial Dataset for Graph Anomaly Detection. *NeurIPS 2022 D&B*. arXiv:2207.03579.
- Jensen, R. I. T., et al. (2023). A synthetic data set to benchmark anti-money laundering methods. *Scientific Data* 10, 661.
- Jesus, S., et al. (2022). Turning the Tables: Biased, Imbalanced, Dynamic Tabular Datasets for ML Evaluation. *NeurIPS 2022 D&B*.
- Lopez-Rojas, E. A., & Axelsson, S. (2014). BankSim: A bank payments simulator for fraud detection research. *26th EMSS*, 144–152.
- Lopez-Rojas, E. A., Elmir, A., & Axelsson, S. (2016). PaySim: A financial mobile money simulator for fraud detection. *28th EMSS*.
- Östman, J., et al. (2025). AMLGentex: Mobilizing Data-Driven Research to Combat Money Laundering. arXiv:2506.13989.
- Otoritas Jasa Keuangan (2024). POJK No. 12 Tahun 2024 tentang Penerapan Strategi Anti Fraud bagi Lembaga Jasa Keuangan.
- Oztas, B., et al. (2023). Enhancing Anti-Money Laundering: Development of a Synthetic Transaction Monitoring Dataset. *IEEE ICEBE*, 47–54. doi:10.1109/ICEBE59045.2023.00028.
- Padhi, I., et al. (2021). Tabular Transformers for Modeling Multivariate Time Series. *ICASSP 2021*.
- PKDD'99 Discovery Challenge, Financial dataset.
- Tang, J., Li, J., Gao, Z., & Li, J. (2022). Rethinking Graph Neural Networks for Anomaly Detection. *ICML 2022*.
- UK Finance (2025). *Annual Fraud Report 2025*.
- Weber, M., et al. (2018). Scalable Graph Learning for Anti-Money Laundering: A First Look. arXiv:1812.00076.
- Weber, M., et al. (2019). Anti-Money Laundering in Bitcoin. *KDD Workshop on Anomaly Detection in Finance*.
- Wiefling, S., Jørgensen, P. R., Thunem, S., & Lo Iacono, L. (2022). Pump Up Password Security! Evaluating and Enhancing Risk-Based Authentication on a Real-World Large-Scale Online Service. *ACM TOPS*. doi:10.1145/3546069.
