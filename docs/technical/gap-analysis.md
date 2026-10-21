# Gap analysis — legacy v1 → platform v2 (legacy context)

> Document status: verified against commit `07ee765` on 2026-10-13.

> **Legacy context.** This is a historical record written when platform v2 replaced the v1 prototype
> (Python/FastAPI + MongoDB, kept in the `legacy/` directory and on branch `legacy/python-fastapi-mongo`). The
> "Defects" table describes **v1 code that no longer runs**; nothing in it is a defect of v2. The "v2 resolution"
> column and §2 are kept current. For the current system read [`architecture.md`](architecture.md) and
> [`technical-overview.md`](technical-overview.md); open work is tracked in [`../backlog.md`](../backlog.md).

This answers "check whether anything was missed". It covers defects found in the legacy code and capabilities that
were not in the original feature list but are needed for a production fraud platform.

## 1. Defects found in the legacy v1 code

| Area | Finding | Impact | v2 resolution |
|---|---|---|---|
| Rule evaluation | only the `>` operator was implemented; `!=` rules in the seed data never fired | most rules were silently inactive | full operator set + tri-state outcome (rule-dsl §4) |
| Velocity rules | "mocked": **always matched** and always added risk | every transaction got inflated scores | real windowed aggregates over indexed history, plus statistics |
| Policies | `policies` were fetched but never used in evaluation | the policy concept had no effect | rulesets with aggregation, weights and actions |
| Transactions | no timestamp field | velocity was impossible | `occurred_at` is required on every event |
| Stats | `policies-performance` matched `rules._id` inside policies, but embedded rules had no `_id` | always 0 | `rules.rule_hits` + counters |
| LLM validator | the function was named `jalidate_rule_structure`, but the UI imported `validate_rule_structure` | the LLM UI crashed on import | validation lives in the Rust rule-service (single source of truth) |
| LLM schema | the validator expected `type: StandardRule`, the API expected `rule_type: standard` | extracted rules could never be posted | one DSL (rule-dsl.md) with JSON-schema structured output |
| Embeddings | stored with HuggingFace MiniLM, retrieved with OpenAIEmbeddings | retrieval returned garbage or failed | one configured embed model for every index (`OLLAMA_EMBED_MODEL`, default bge-m3 via Ollama), independent of the chat provider |
| Vector store | `MongoDBAtlasVectorSearch` against local Mongo 5 (Atlas-only feature) | RAG could not work outside Atlas | OpenSearch 2.19 k-NN, hybrid BM25 + vector with RRF, one index per tenant |
| Extractor | `RetrievalQA` built without a retriever in `extract_structured_policy` | runtime error | explicit retrieval with a tool loop |
| Agent tool | `post_new_rule` always posted to `/rule/standard` | velocity proposals broke | proposals via rule-service; nothing auto-posted |
| Security | no auth, no tenant isolation, card `number` stored raw | PCI/PII exposure | JWT + RBAC + RLS, hashed PAN/account (tenant pepper), gateway IP allow-list, tokens kept out of the browser (BFF) |
| Governance | the LLM could create rules directly | unreviewed rules could go live | maker–checker, shadow mode, and the LLM only proposes |

## 2. Capabilities added beyond the requested list

| Capability | Why it is needed |
|---|---|
| **Non-transaction events** (login, account change, promo redemption, payout, registration) | ATO and promo abuse cannot be detected from payments alone |
| **Labels + feedback loop + case management** | supervised ML needs labels; analysts need a review queue for `review` decisions |
| **Shadow mode + backtest** | safe rollout of new or LLM-proposed rules |
| **Versioning + maker–checker + audit** | regulatory accountability (who changed what and when) and reproducible decisions |
| **Reason codes / explainability** | disputes, customer communication, regulator requests |
| **Degraded mode** | an ML or LLM outage must never block payments |
| **Supernode cap** in the graph | shared public IPs or office Wi-Fi would otherwise link everyone |
| **Similarity links** (phone ±1 digit, address trigram, email local-part) | fraud rings deliberately vary identifiers ("mirip") |
| **Tenant-peppered PII hashing** | the same card links within a tenant, but it is never linkable across tenants |
| **Feature catalog + drift monitoring (PSI)** | the LLM needs data evidence to judge rule relevance; it also detects new patterns when data changes |
| **Stage templates** (pre-payment, post-payment, returns, promo, …) | fast onboarding of a new project with sensible default rules |
| **Refund abuse & money mule typologies** | common in marketplaces and wallets; the user's typology list implied them |
| **Data simulator** | demos and tests without production data |
| **Research datasets as tenants** (Sparkov, PaySim, SAML-D) | reproducible experiments on public data next to the synthetic demo |

## 3. Open questions to confirm with the product owner

Status as of 2026-10-13 in brackets.

1. **Cross-project signals.** Should a customer confirmed as fraud in `returns` automatically raise risk in
   `pre_payment` of the same tenant? [open — backlog P2 "linked projects" (opt-in)]
2. **Decision callbacks.** Do client systems need an asynchronous webhook when a `review` case is resolved?
   [open — backlog E8 P1 "decision callback/webhook"]
3. **LLM provider.** Is local Ollama mandatory for all tenants (data residency), or may some tenants opt into hosted
   models? [partly answered — Gemini can be enabled for chat **deployment-wide** (`LLM_PROVIDER=gemini`); embeddings
   always stay on Ollama; there is no per-tenant provider choice yet]
4. **Retention.** How long must events and decisions be kept (regulatory minimum vs UU PDP minimisation)? [open —
   no retention job exists]
5. **Plugin trust.** Who may install external ML plugins in production? [current implementation: files in the
   read-only `./plugins` mount, reload by platform admin or internal call only]
