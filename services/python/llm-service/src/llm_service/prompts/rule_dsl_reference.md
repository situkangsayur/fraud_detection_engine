## Rule DSL reference (condensed — the rule-service validator is authoritative)

Envelope:
```json
{"code":"RL-XXX-001","name":"...","description":"...","kind":"simple|velocity|composite|reference|graph",
 "typologies":["carding|account_takeover|bank_account_takeover|system_breach|promo_abuse|refund_abuse|money_mule|other"],
 "event_types":["transaction"],"risk_score":0-100,"trapped_score":0,"action":"score|force_review|force_decline|force_approve",
 "on_trapped":"ignore|score|review","missing_as_no_match":false,"definition":{...}}
```
Operands: `{"type":"const","value":X}`, `{"type":"field","path":"event.amount"}`,
`{"type":"formula","expr":"F(x,y) = 2x + y^2","args":{"x":<operand>,"y":<operand>}}`,
`{"type":"hist","path":"amount"}` (composite.history_filter only), `{"type":"ref","path":"attr"}` (reference.attribute_condition only).
Condition leaf: `{"left":<operand>,"op":"eq|ne|gt|gte|lt|lte|between|in|not_in|contains|not_contains|starts_with|ends_with|regex|is_null|is_not_null|similar","right":<operand>}`.
Groups: `{"all":[...]}`, `{"any":[...]}`, `{"not":<cond>}`, `{"at_least":{"n":2,"of":[...]}}`.
Field roots: `event.*` (canonical fields), `source.*` (raw source record), `customer.*`, `features.*`, `ml.*` (fraud_probability, anomaly_score, cluster_id), `graph.*` (distance_to_fraud, fraud_neighbors_2, component_size, shared_entity_count).

Definitions:
- simple: `{"kind":"simple","when":<condition>,"scoring":"binary|weighted"}`
- velocity: `{"kind":"velocity","history_event_types":["transaction"],"group_by":["customer_id"],"window":{"duration":"24h"},"aggregate":{"fn":"count|sum|avg|min|max|distinct_count|stddev|median|percentile","field":"amount"},"statistic":null|{"fn":"zscore|gaussian_tail|percentile_rank|linear_trend|poisson_tail",...},"include_current":true,"min_samples":1,"compare":{"op":"gte","right":<operand>}}`
- composite: `{"kind":"composite","gate":<condition>,"history_filter":<condition over hist operands>,"velocity":{<velocity body without kind>}}`
- reference: `{"kind":"reference","list":"card_blacklist","key":<operand>,"mode":"exists|not_exists|attribute","attribute_condition":<condition>}`
- graph: `{"kind":"graph","metric":"distance_to_fraud|fraud_neighbors|shared_entity_count|component_size|degree|community_fraud_rate","link_kinds":["phone","card","device","address","email","bank_account","ref_transaction"],"include_similar":true,"max_depth":3,"compare":{"op":"lte","right":{"type":"const","value":2}}}`
Only use field paths that exist in the project field catalog you were given.
