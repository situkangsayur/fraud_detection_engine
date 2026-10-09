# Gap analysis — legacy v1 → platform v2

This answers "check whether anything was missed". It covers defects found in the legacy code (branch
`legacy/python-fastapi-mongo`) and capabilities that were not in the original feature list but are needed for a
production fraud platform.

## 1. Defects found in the legacy code

| Area | Finding | Impact | v2 resolution |
|---|---|---|---|
| Rule evaluation | only the `>` operator was implemented; `!=` rules in the seed data never fired | most rules were silently inactive | full operator set + tri-state outcome (rule-dsl §4) |
| Velocity rules | "mocked": **always matched** and always added risk | every transaction got inflated scores | real windowed aggregates over indexed history, plus statistics |
| Policies | `policies` were fetched but never used in evaluation | the policy concept had no effect | rulesets with aggregation, weights and actions |
| Transactions | no timestamp field | velocity was impossible | `occurred_at` is required on every event |
| Stats | `policies-performance` matched `rules._id` inside policies, but embedded rules had no `_id` | always 0 | `rules.rule_hits` + counters |
| LLM validator | the function was named `jalidate_rule_structure`, but the UI imported `validate_rule_structure` | the LLM UI crashed on import | validation lives in the Rust rule-service (single source of truth) |
| LLM schema | the validator expected `type: StandardRule`, the API expected `rule_type: standard` | extracted rules could never be posted | one DSL (rule-dsl.md) with JSON-schema structured output |
| Embeddings | stored with HuggingFace MiniLM, retrieved with OpenAIEmbeddings | retrieval returned garbage or failed | one configured embed model per index (bge-m3 via Ollama) |
| Vector store | `MongoDBAtlasVectorSearch` against local Mongo 5 (Atlas-only feature) | RAG could not work outside Atlas | OpenSearch k-NN, hybrid BM25 + vector |
| Extractor | `RetrievalQA` built without a retriever in `extract_structured_policy` | runtime error | explicit retrieval with a tool loop |
| Agent tool | `post_new_rule` always posted to `/rule/standard` | velocity proposals broke | proposals via rule-service; nothing auto-posted |
| Security | no auth, no tenant isolation, card `number` stored raw | PCI/PII exposure | JWT + RBAC + RLS, hashed PAN/account (tenant pepper) |
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

## 3. Open questions to confirm with the product owner

1. **Cross-project signals.** Should a customer confirmed as fraud in `returns` automatically raise risk in
   `pre_payment` of the same tenant? This is currently backlog "linked projects" (opt-in).
2. **Decision callbacks.** Do client systems need an asynchronous webhook when a `review` case is resolved?
   (backlog E8)
3. **LLM provider.** Is local Ollama mandatory for all tenants (data residency), or may some tenants opt into hosted
   models?
4. **Retention.** How long must events and decisions be kept (regulatory minimum vs UU PDP minimisation)?
5. **Plugin trust.** Who may install external ML plugins in production (platform team only is the current
   assumption)?
