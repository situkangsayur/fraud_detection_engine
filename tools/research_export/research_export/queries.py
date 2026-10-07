"""SQL for every exported table. All queries are read-only and scoped to one tenant (by slug).

The connection is a superuser on purpose: RLS hides other tenants from service roles, and the export must see every
project of the tenant. Nothing here writes.
"""

from __future__ import annotations

# Flat event table: one row per event with its latest decision, per-engine scores and labels.
EVENTS = """
with t as (select id from core.tenants where slug = %(tenant)s)
select p.slug                                   as project,
       p.stage                                  as project_stage,
       e.id                                     as event_id,
       e.external_id,
       e.event_type,
       e.occurred_at,
       e.received_at,
       e.load_only,
       c.external_id                            as customer_external_id,
       c.kyc_level                              as customer_kyc_level,
       c.segment                                as customer_segment,
       c.registered_at                          as customer_registered_at,
       e.channel, e.status as event_status, e.amount, e.currency, e.merchant_id, e.merchant_category,
       e.payment_method, e.card_bin, e.issuer_country, e.geo_country, e.geo_city,
       e.promo_code is not null                 as has_promo,
       e.discount_amount, e.cashback_amount, e.account_change_type, e.login_success,
       e.device_id is not null                  as has_device,
       d.decision,
       d.final_score,
       (d.engine_scores->>'rules')::real        as score_rules,
       (d.engine_scores->>'supervised')::real   as score_supervised,
       (d.engine_scores->>'unsupervised')::real as score_unsupervised,
       (d.engine_scores->>'graph')::real        as score_graph,
       (d.ml->>'fraud_probability')::real       as ml_fraud_probability,
       (d.ml->>'anomaly_score')::real           as ml_anomaly_score,
       (d.ml->>'cluster_id')::int               as ml_cluster_id,
       (d.ml->>'cluster_fraud_rate')::real      as ml_cluster_fraud_rate,
       (d.graph->>'degree')::int                as graph_degree,
       (d.graph->>'component_size')::int        as graph_component_size,
       (d.graph->>'distance_to_fraud')::int     as graph_distance_to_fraud,
       (d.graph->>'fraud_neighbors_1')::int     as graph_fraud_neighbors_1,
       (d.graph->>'fraud_neighbors_2')::int     as graph_fraud_neighbors_2,
       (d.graph->>'shared_entity_count')::int   as graph_shared_entity_count,
       (d.graph->>'community_fraud_rate')::real as graph_community_fraud_rate,
       array_to_string(d.degraded, ',')         as degraded_engines,
       d.latency_ms,
       d.reasons::text                          as reasons_json,
       el.label                                 as event_label,
       el.fraud_type                            as event_label_fraud_type,
       c.risk_label                             as customer_risk_label,
       cs.status                                as case_status
from core.events e
join core.projects p on p.id = e.project_id
join t on t.id = e.tenant_id
left join core.customers c on c.id = e.customer_id
left join lateral (select * from core.decisions d where d.event_id = e.id order by d.created_at desc limit 1) d on true
left join lateral (select l.label, l.fraud_type from core.labels l
                   where l.subject_type = 'event' and l.subject_id = e.id order by l.created_at desc limit 1) el on true
left join lateral (select x.status from core.cases x where e.id = any(x.event_ids) or x.event_id = e.id
                   order by x.created_at desc limit 1) cs on true
order by p.slug, e.occurred_at
"""

RULE_HITS = """
select p.slug as project, h.event_id, r.code as rule_code, r.kind as rule_kind, h.rule_version,
       rs.code as ruleset_code, h.outcome, h.contribution, h.shadow, h.occurred_at
from rules.rule_hits h
join core.projects p on p.id = h.project_id
join core.tenants t on t.id = h.tenant_id and t.slug = %(tenant)s
join rules.rules r on r.id = h.rule_id
left join rules.rulesets rs on rs.id = h.ruleset_id
"""

FEATURES = """
select f.event_id, f.feature_set_version, f.features::text as features_json
from core.event_features f
join core.tenants t on t.id = f.tenant_id and t.slug = %(tenant)s
"""

ANOMALY = """
select a.event_id, a.model_id, a.anomaly_score, a.cluster_id, a.pca_x, a.pca_y
from ml.event_anomaly a
join core.tenants t on t.id = a.tenant_id and t.slug = %(tenant)s
"""

RULES = """
select p.slug as project, r.code, r.name, r.kind, array_to_string(r.typologies, ',') as typologies,
       array_to_string(r.event_types, ',') as event_types, r.status, r.current_version,
       v.risk_score, v.action, v.on_trapped, v.definition::text as definition_json
from rules.rules r
join core.projects p on p.id = r.project_id
join core.tenants t on t.id = r.tenant_id and t.slug = %(tenant)s
left join rules.rule_versions v on v.rule_id = r.id and v.version = r.current_version
"""

RULESETS = """
select p.slug as project, s.code, s.name, s.aggregation, s.max_score, s.status, s.version,
       array_to_string(s.typologies, ',') as typologies
from rules.rulesets s
join core.projects p on p.id = s.project_id
join core.tenants t on t.id = s.tenant_id and t.slug = %(tenant)s
"""

MODELS = """
select p.slug as project, m.kind, m.version, m.status, m.algorithms::text as algorithms_json,
       m.params::text as params_json, m.trained_rows, m.metrics::text as metrics_json,
       m.training_started_at, m.training_finished_at, m.activated_at
from ml.models m
join core.projects p on p.id = m.project_id
join core.tenants t on t.id = m.tenant_id and t.slug = %(tenant)s
"""

CLUSTERS = """
select p.slug as project, c.model_id, c.cluster_id, c.size, c.fraud_rate, c.labeled_count, c.label,
       c.top_features::text as top_features_json
from ml.clusters c
join ml.models m on m.id = c.model_id
join core.projects p on p.id = m.project_id
join core.tenants t on t.id = c.tenant_id and t.slug = %(tenant)s
"""

CASES = """
select p.slug as project, c.id as case_id, c.status, c.priority, array_to_string(c.typologies, ',') as typologies,
       cardinality(c.event_ids) as n_events, c.created_at, c.resolved_at
from core.cases c
join core.projects p on p.id = c.project_id
join core.tenants t on t.id = c.tenant_id and t.slug = %(tenant)s
"""

LABELS = """
select p.slug as project, l.subject_type, l.subject_id, l.label, l.fraud_type, l.source, l.created_at
from core.labels l
join core.projects p on p.id = l.project_id
join core.tenants t on t.id = l.tenant_id and t.slug = %(tenant)s
"""

PROPOSALS = """
select p.slug as project, x.source, x.proposal_type, x.status, x.llm_model, x.rationale,
       x.citations::text as citations_json, x.validation::text as validation_json, x.backtest::text as backtest_json,
       x.definition::text as definition_json, x.created_at, x.reviewed_at
from rules.proposals x
join core.projects p on p.id = x.project_id
join core.tenants t on t.id = x.tenant_id and t.slug = %(tenant)s
"""

REPORTS = """
select p.slug as project, r.report_type, r.title, r.status, r.model, r.content_md,
       r.structured::text as structured_json, r.created_at, r.finished_at
from llm.reports r
join core.projects p on p.id = r.project_id
join core.tenants t on t.id = r.tenant_id and t.slug = %(tenant)s
"""

COMMUNITIES = """
select p.slug as project, g.community_id, g.size, g.fraud_count, g.fraud_rate, g.computed_at
from ml.graph_community_stats g
join core.projects p on p.id = g.project_id
join core.tenants t on t.id = g.tenant_id and t.slug = %(tenant)s
"""

PROJECT_SETTINGS = """
select p.slug as project, p.stage, s.key, s.value::text as value_json
from core.projects p
join core.tenants t on t.id = p.tenant_id and t.slug = %(tenant)s
left join core.project_settings s on s.project_id = p.id
"""

TABLES: dict[str, str] = {
    "rule_hits": RULE_HITS,
    "features": FEATURES,
    "anomaly": ANOMALY,
    "rules": RULES,
    "rulesets": RULESETS,
    "models": MODELS,
    "clusters": CLUSTERS,
    "cases": CASES,
    "labels": LABELS,
    "proposals": PROPOSALS,
    "llm_reports": REPORTS,
    "graph_communities": COMMUNITIES,
    "project_settings": PROJECT_SETTINGS,
}
