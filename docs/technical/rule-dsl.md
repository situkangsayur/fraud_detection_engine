# Rule DSL — specification v1

Implemented by the pure Rust crate `services/rust/crates/rule-engine`, served by `rule-service`. Rules are stored as JSON (`rules.rule_versions.definition`
plus envelope columns). This document is the contract for the Rust engine, the Nuxt rule builder and the LLM rule
recommender (which must emit exactly this JSON).

## 1. Rule kinds

| Kind | Purpose |
|---|---|
| `simple` | Compare one or more fields (or formulas of fields) against constants **or other fields** |
| `velocity` | Aggregate a field over history, grouped by one or more fields, within a window, optionally transformed by a **statistical function** (z-score, gaussian tail, linear trend, percentile rank, poisson tail), then compare against a value or a field |
| `composite` | Velocity whose history is **filtered by simple-rule conditions** (plus an optional gate on the current event) |
| `reference` | White/black/watch-list lookup in a named **reference list** (created at runtime) by a join key; hit on membership, non-membership, or a condition on the entry's attributes |
| `graph` | Condition on a graph metric of the customer: distance to a known fraudster, fraud neighbours within k hops, shared-entity count, component size, community fraud rate |

Every rule evaluates to exactly one of three **outcomes**:

* `match` (true)
* `no_match` (false)
* `trapped`: the rule could not be evaluated. Causes include a null/missing operand, a type mismatch, division by zero or
  a non-finite formula result, too few samples for a statistic, an unknown reference list, a regex compile error, or a
  data-provider error. A trapped result always carries a `reason`.

## 2. Rule envelope

```json
{
  "code": "RL-CARD-003",
  "name": "Card shared by many customers",
  "description": "Same card fingerprint used by >= 3 distinct customers in 30 days",
  "kind": "velocity",
  "typologies": ["carding"],
  "event_types": ["transaction"],
  "risk_score": 45,
  "trapped_score": 0,
  "action": "score",
  "on_trapped": "ignore",
  "missing_as_no_match": false,
  "definition": { "...kind-specific body..." }
}
```

| Field | Type | Notes |
|---|---|---|
| `code` | string `^[A-Z0-9-]{3,40}$` | unique, stable across versions |
| `kind` | enum | must equal the definition's kind |
| `typologies` | string[] | from the typology list in architecture.md §1, used for grouping and analytics |
| `event_types` | string[] | the rule only runs for these event types; empty means all |
| `risk_score` | number 0–100 | contribution when matched (before ruleset weight) |
| `trapped_score` | number 0–100 | contribution when trapped **and** `on_trapped = "score"` |
| `action` | `score` \| `force_review` \| `force_decline` \| `force_approve` | effect when matched |
| `on_trapped` | `ignore` \| `score` \| `review` | `ignore`: logged only; `score`: add `trapped_score`; `review`: force review |
| `missing_as_no_match` | bool | if true, a missing/null **field** operand yields `no_match` instead of `trapped` (other trap causes still trap) |

## 3. Operands

All operands are tagged objects (`type` discriminator).

```jsonc
{ "type": "const",   "value": 1000000 }                 // number | string | bool | null | array
{ "type": "field",   "path": "event.amount" }            // path into the evaluation context (§5)
{ "type": "formula", "expr": "F(x,y,z) = 2x + 2^y / z^2",
  "args": { "x": {"type":"field","path":"event.amount"},
            "y": {"type":"const","value":3},
            "z": {"type":"field","path":"features.cust_cnt_24h"} } }
{ "type": "ref",     "path": "max_amount" }              // only inside reference.attribute_condition: entry attribute
{ "type": "hist",    "path": "amount" }                  // only inside composite.history_filter: historical row field
```

`args` values are themselves operands, so formulas can nest (a formula arg may be another formula or a field).

### 3.1 Formula language

```
formula    := [ header "=" ] expr
header     := IDENT "(" IDENT { "," IDENT } ")"           e.g. F(x,y,z)
expr       := term { ("+" | "-") term }
term       := unary { ("*" | "/" | "%") unary | implicit }
unary      := "-" unary | power
power      := primary [ "^" unary ]                        right-associative; -x^2 = -(x^2)
primary    := NUMBER | IDENT | IDENT "(" args ")" | "(" expr ")"
implicit   := NUMBER followed directly by IDENT or "(" ; ")" followed by "(" or IDENT
```

* Numbers: `12`, `1.5`, `2e-3`. Constants: `pi`, `e`.
* Implicit multiplication: `2x` → `2*x`, `3(x+1)` → `3*(x+1)`, `(a+b)(a-b)`. Adjacent identifiers are **not**
  split (`xy` is the identifier `xy`); write `x*y`.
* Functions: `abs, sqrt, ln, log10, log(x, base), exp, pow(x,y), min(…), max(…), floor, ceil, round(x[, digits]),
  clamp(x, lo, hi), sigmoid(x), gauss(x, mu, sigma)` (normal pdf), `normcdf(x, mu, sigma)`, `if(c, a, b)` (c ≠ 0 → a).
* If a header is present, its parameter list must equal the set of `args` keys. Without a header, every free
  identifier must be an `args` key or a constant.
* Validation happens at save time: the formula is parsed, and unknown functions, unbound variables or arity errors are
  rejected with position info (`{"message": "...", "position": 7}`).
* Evaluation traps when an arg is null or non-numeric (bools coerce to 1/0, numeric strings are parsed), on division or
  modulo by zero, or when the result is NaN/±∞.

Example: `F(x,y,z) = 2x + 2^y / z^2` with x=10, y=3, z=2 → `2·10 + 8/4 = 22`.

## 4. Conditions

```jsonc
// leaf
{ "left": <operand>, "op": "gt", "right": <operand>, "weight": 1.0, "options": { } }
// groups (arbitrarily nested)
{ "all": [ <condition>, ... ] }
{ "any": [ <condition>, ... ] }
{ "not": <condition> }
{ "at_least": { "n": 2, "of": [ <condition>, ... ] } }
```

Operators:

| op | right operand | notes |
|---|---|---|
| `eq`, `ne` | any | numbers compared numerically, strings exactly (`options.case_insensitive`) |
| `gt`, `gte`, `lt`, `lte` | number (or date-time string vs date-time) | |
| `between` | array `[lo, hi]` (inclusive) | |
| `in`, `not_in` | array | |
| `contains`, `not_contains`, `starts_with`, `ends_with` | string | strings; `contains` also works for array-left |
| `regex` | string (RE2 syntax) | compiled once and cached |
| `is_null`, `is_not_null` | — (omit `right`) | never trap on null |
| `similar` | string | `options.threshold` (default 0.85) with `options.method` `jaro_winkler` (default) \| `levenshtein_ratio` |

Three-valued (Kleene) logic:

* `all`: any `no_match` → `no_match`; otherwise any `trapped` → `trapped`; otherwise `match`.
* `any`: any `match` → `match`; otherwise any `trapped` → `trapped`; otherwise `no_match`.
* `not`: swaps match/no_match; `trapped` stays `trapped`.
* `at_least n`: matches ≥ n → `match`; matches + trapped < n → `no_match`; otherwise `trapped`.

## 5. Evaluation context (field paths)

```jsonc
{
  "event":    { canonical event fields: "event_type", "amount", "currency", "channel", "device_id", "ip_address",
                "instrument_fingerprint", "card_bin", "issuer_country", "geo_country", "promo_code",
                "discount_amount", "cashback_amount", "merchant_id", "ref_transaction_id", "occurred_at", ... },
  "source":   { the raw source record (all original fields, nested), e.g. "source.order.items[0].sku" },
  "customer": { "external_id", "kyc_level", "segment", "status", "registered_at", "risk_label", "account_age_days", "attributes.*" },
  "features": { feature-catalog v1 (see feature-catalog.md) },
  "ml":       { "fraud_probability", "anomaly_score", "cluster_id", "cluster_fraud_rate", "model_version" },
  "graph":    { "distance_to_fraud", "fraud_neighbors_1", "fraud_neighbors_2", "component_size", "shared_entity_count", "community_fraud_rate" }
}
```

Paths use dots and `[n]` for array indexes. Paths are validated against `field_catalog` at save time. Unknown paths are
rejected unless they are under `source.` (source schemas evolve, and missing values then trap or no-match at runtime).

## 6. Kind-specific definitions

### 6.1 `simple`

```json
{ "kind": "simple",
  "when": { "all": [
    { "left": {"type":"field","path":"event.amount"}, "op": "gt", "right": {"type":"const","value": 5000000} },
    { "left": {"type":"field","path":"event.issuer_country"}, "op": "ne", "right": {"type":"field","path":"event.geo_country"} }
  ]},
  "scoring": "binary" }
```

`scoring`:
* `binary` (default): contribution = `risk_score` on match.
* `weighted`: contribution = `risk_score × Σ(weight of matched leaves) / Σ(weight of all leaves)`. The rule counts as
  `match` when the fraction is > 0 **and** the `when` tree does not evaluate to `trapped`. This covers
  "a rule over several fields where each field adds partial risk".

### 6.2 `velocity`

```json
{ "kind": "velocity",
  "history_event_types": ["transaction"],
  "group_by": ["instrument_fingerprint"],
  "window": { "duration": "30d" },
  "aggregate": { "fn": "distinct_count", "field": "customer_id" },
  "statistic": null,
  "include_current": true,
  "min_samples": 1,
  "compare": { "op": "gte", "right": {"type":"const","value": 3} } }
```

* `group_by`: history rows whose field equals the **current event's** value for that field (AND across fields). Allowed
  fields are velocity-enabled `field_catalog` entries (canonical event columns plus `source.*` paths flagged
  velocity-enabled).
* `window`: `{ "duration": "<n>(s|m|h|d|w)" }` (sliding, ending at the event's `occurred_at`) or `{ "last_n": 50 }`.
* `aggregate.fn`: `count`, `sum`, `avg`, `min`, `max`, `distinct_count`, `stddev`, `median`, `percentile`
  (`"p": 0.95`). `field` is optional for `count`.
* `include_current`: whether the current event is part of the history (default `true` for plain aggregates and `false`
  when a `statistic` is set).
* `compare.right` may be any operand, e.g. compare `sum(amount) 24h` against `customer.attributes.monthly_income`
  or against a formula.

`statistic` (optional) turns history into a statistical signal. The compared value becomes the statistic's output:

| `fn` | params | value |
|---|---|---|
| `zscore` | `of` operand (default: current event's `aggregate.field`) | `(x − mean) / std` of the per-event `aggregate.field` history |
| `gaussian_tail` | `of`, `tail`: `upper`\|`lower`\|`two` | tail probability of x under N(mean, std), e.g. `lt 0.01` |
| `percentile_rank` | `of` | fraction of history values ≤ x (0..1) |
| `linear_trend` | `bucket` (e.g. `"1d"`), `output`: `slope`\|`forecast`\|`residual_z` | OLS over bucketed aggregate series; `residual_z = (actual_current_bucket − forecast)/σ_residual` |
| `poisson_tail` | `bucket` | λ = mean of bucketed `count`; value = P(N ≥ current bucket count) |

With fewer than `min_samples` history points (default 5 when a statistic is set), or with σ = 0, the rule traps with
reason `insufficient_history` / `zero_variance`.

### 6.3 `composite` (velocity over a filtered history)

```json
{ "kind": "composite",
  "gate": { "all": [ { "left": {"type":"field","path":"event.promo_code"}, "op": "is_not_null" } ] },
  "history_filter": { "all": [
    { "left": {"type":"hist","path":"promo_code"}, "op": "eq", "right": {"type":"field","path":"event.promo_code"} },
    { "left": {"type":"hist","path":"discount_amount"}, "op": "gt", "right": {"type":"const","value": 0} }
  ]},
  "velocity": {
    "history_event_types": ["promo_redemption", "transaction"],
    "group_by": ["device_id"],
    "window": { "duration": "7d" },
    "aggregate": { "fn": "distinct_count", "field": "customer_id" },
    "compare": { "op": "gte", "right": {"type":"const","value": 3} } } }
```

* `gate` is evaluated on the current event first. If it is `no_match`, the rule is `no_match` and no query runs; if it is
  `trapped`, the rule is `trapped`.
* `history_filter` is compiled to SQL. Its left side must be a `hist` operand. Its right side is a `const` or a
  current-context operand (`field`/`formula`), which is evaluated to a constant before the query. Allowed ops: `eq, ne,
  gt, gte, lt, lte, between, in, not_in, is_null, is_not_null, starts_with, contains`.
* `velocity` has the same body as §6.2 without `kind`.

### 6.4 `reference`

```json
{ "kind": "reference",
  "list": "card_blacklist",
  "key": {"type":"field","path":"event.instrument_fingerprint"},
  "mode": "exists" }
```

```json
{ "kind": "reference",
  "list": "merchant_limits",
  "key": {"type":"field","path":"event.merchant_id"},
  "mode": "attribute",
  "attribute_condition": { "left": {"type":"field","path":"event.amount"}, "op": "gt",
                           "right": {"type":"ref","path":"max_amount"} } }
```

* `mode`: `exists` (the key is in the list and currently valid), `not_exists`, or `attribute` (the entry exists **and**
  `attribute_condition` matches; no entry → `no_match`).
* Lists live in `rules.reference_lists` / `rules.reference_entries`. They can be created at runtime from the UI, by API, or by
  CSV import. A list is either **project-scoped** or **tenant-wide** (shared by all projects of the tenant). Name
  resolution checks the project list first, then the tenant-wide list with the same name. `valid_from` / `valid_until` are respected. An unknown list → `trapped` (`unknown_reference_list`).
* Whitelist semantics come from `action: "force_approve"`; blacklist semantics from `force_decline` or a high
  `risk_score`.

### 6.5 `graph`

```json
{ "kind": "graph",
  "metric": "distance_to_fraud",
  "link_kinds": ["phone", "card", "device", "address", "email", "bank_account", "ref_transaction"],
  "include_similar": true,
  "max_depth": 3,
  "compare": { "op": "lte", "right": {"type":"const","value": 2} } }
```

| metric | value |
|---|---|
| `distance_to_fraud` | min number of customer→customer hops to any customer with `risk_label = 'fraud'`; `+∞` if none within `max_depth` (never traps). On the wire (`graph.distance_to_fraud`) this is `null`; the feature `graph_distance_to_fraud` uses 99 |
| `fraud_neighbors` | count of fraud customers within `max_depth` |
| `shared_entity_count` | number of the customer's entities (restricted to `link_kinds`) that are shared with ≥ 1 other customer |
| `component_size` | size of the connected customer component (bounded BFS, capped at 1000) |
| `degree` | number of distinct customers at distance 1 |
| `community_fraud_rate` | fraud rate of the customer's Louvain community (computed by ml-service into `ml.graph_community_stats`; no community → trapped) |

One hop = two customers sharing an entity of a listed kind. With `include_similar`, "similar" entities also count as
shared (phone numbers differing by ≤ 1 digit or with the same last 9 digits; address trigram similarity ≥
`settings.graph.similarity_threshold`). Supernode entities (degree > `settings.graph.supernode_degree_cap`, e.g. a
public Wi-Fi IP) are skipped.

## 7. Rulesets & scoring

```json
{ "code": "RS-CARDING", "name": "Carding", "event_types": ["transaction"], "typologies": ["carding"],
  "aggregation": "probabilistic_or", "max_score": 100,
  "rules": [ { "rule_id": "…", "weight": 1.0, "pinned_version": null } ] }
```

Per rule: `contribution = weight × risk_score × fraction` on match (fraction is 1 unless `scoring=weighted`),
`weight × trapped_score` when trapped with `on_trapped = "score"`, and 0 otherwise.

| aggregation | ruleset score |
|---|---|
| `sum` | `min(max_score, Σ c)` |
| `max` | `max c` |
| `probabilistic_or` | `100 × (1 − Π(1 − cᵢ/100))`, then capped |
| `weighted_average` | `100 × Σ c / Σ(weightᵢ × risk_scoreᵢ)` |

**Shadow semantics (champion/challenger):**
* A **shadow rule** is evaluated and traced, but it counts nowhere: not in its ruleset's score, not in `rules_score`,
  and it produces no actions or reasons.
* A **shadow ruleset** computes its **own** score from its non-shadow rules exactly as it would when live. That score
  is reported in `rulesets[]` with `shadow: true`, which lets a challenger be compared to the live ruleset on real
  traffic. It is still excluded from `rules_score`, from actions (`force_*`) and from reasons.
* In trace items, `contribution` always holds the would-be value (`weight × risk_score × fraction`, or
  `weight × trapped_score` when trapped with `on_trapped = "score"`). `shadow: true` means that value did not count
  toward the decision.

Trace item stored in `decisions.rule_results`:

```json
{ "rule_id": "…", "rule_code": "RL-CARD-003", "version": 2, "ruleset_code": "RS-CARDING", "kind": "velocity",
  "outcome": "match", "contribution": 45.0, "shadow": false, "action": "score", "trapped_reason": null,
  "trace": { "value": 4, "op": "gte", "right": 3, "samples": 4, "window": "30d" }, "duration_us": 830 }
```

## 8. Validation (`POST /api/v1/rules/validate`)

Returns `{ "valid": bool, "errors": [{ "path": "definition.when.all[1].right", "message": "…" }],
"referenced_fields": [...], "referenced_lists": [...] }`. It checks the JSON shape, enum values, formula parse and
arity, field paths against `field_catalog`, velocity group/aggregate fields being velocity-enabled, `hist`/`ref`
operands appearing only in their allowed positions, regex compile, and `risk_score` ranges. **The LLM service must
call this before creating any proposal.**

## 9. Resolved semantics (implementation notes)

These points were left open above and are fixed by the `rule-engine` crate; its tests encode them.

* **History field names** in velocity/composite (`group_by`, `aggregate.field`, `hist` operands) are bare canonical
  column names (`amount`, `device_id`, `customer_id`) or `source.*` paths. A bare name `x` reads the current value
  from `event.x`.
* **`group_by` must not be empty.** This prevents project-wide scans.
* **`compare` operators** in velocity/composite/graph rules: `eq, ne, gt, gte, lt, lte, between, in, not_in`.
* **Empty history:** `count`, `sum` and `distinct_count` return 0; the other aggregates trap with
  `insufficient_history`.
* **Statistics:**
  * `min_samples` counts history points (buckets for bucketed statistics), excluding the current one.
  * `zscore`, `gaussian_tail` and `percentile_rank` require `aggregate.field`.
  * `linear_trend` and `poisson_tail` require a duration window with `bucket < window`. `linear_trend` needs ≥ 2
    buckets (≥ 3 for `residual_z`).
  * `poisson_tail` uses λ = mean of the earlier buckets and x = the current bucket; λ = 0 with x > 0 gives 0.
* **`missing_as_no_match`** applies where a value is read (leaf operands, group_by values, reference keys, statistic
  inputs), before Kleene combination.
* **`scoring: weighted` under `not`:** the matched and unmatched weights swap. Trapped leaves never count as matched.
* **History filters** accept `all`, `any` and `not`; `at_least` is rejected.
* **Graph `max_depth`** must be between 1 and 3.
* **Formulas:**
  * Implicit multiplication also works across whitespace (`2 x`), but adjacent identifiers are never split.
  * A parameter named `e` shadows the constant.
* **The same rule in several rulesets** is evaluated once per event. Its reason keeps the highest contribution.
* **Per-rule time budget:** a rule exceeding it is `trapped` with reason `timeout`.
* **Traces** serialise non-finite numbers as the strings `"Infinity"` / `"-Infinity"`.
