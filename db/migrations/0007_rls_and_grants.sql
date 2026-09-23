-- 0007_rls_and_grants.sql — Row-Level Security on every tenant table + least-privilege grants per service role.
-- Roles are created by deploy/postgres/init/01-roles.sh; grants are skipped for roles that do not exist
-- (e.g. in `#[sqlx::test]` databases).

-- ---------------------------------------------------------------------------
-- RLS: tenant_id = core.current_tenant()  (fails closed when app.tenant_id is unset)
-- ---------------------------------------------------------------------------
DO $$
DECLARE
    t text;
    tenant_tables text[] := ARRAY[
        'core.projects', 'core.project_members', 'core.project_settings', 'core.approvals',
        'core.data_sources', 'core.data_source_mappings', 'core.field_catalog', 'core.ingest_errors',
        'core.customers', 'core.events', 'core.event_features', 'core.decisions', 'core.cases', 'core.labels',
        'rules.rules', 'rules.rule_versions', 'rules.rulesets', 'rules.ruleset_rules', 'rules.reference_lists',
        'rules.reference_entries', 'rules.rule_hits', 'rules.rule_eval_counters', 'rules.proposals',
        'graph.nodes_customer', 'graph.entities', 'graph.entity_links', 'graph.entity_similarity',
        'ml.models', 'ml.clusters', 'ml.event_anomaly', 'ml.graph_communities', 'ml.graph_community_stats',
        'llm.regulations', 'llm.project_regulations', 'llm.regulation_changes', 'llm.reports',
        'llm.conversations', 'llm.messages',
        'ingest.jobs', 'ingest.uploads'
    ];
BEGIN
    FOREACH t IN ARRAY tenant_tables LOOP
        EXECUTE format('ALTER TABLE %s ENABLE ROW LEVEL SECURITY', t);
        EXECUTE format('CREATE POLICY tenant_isolation ON %s USING (tenant_id = core.current_tenant()) '
                       'WITH CHECK (tenant_id = core.current_tenant())', t);
    END LOOP;
END
$$;

-- ---------------------------------------------------------------------------
-- Grants
-- ---------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION pg_temp.grant_if_role(p_role text, p_sql text) RETURNS void AS $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = p_role) THEN
        EXECUTE replace(p_sql, '{role}', quote_ident(p_role));
    END IF;
END;
$$ LANGUAGE plpgsql;

-- Everyone: schema usage for the helper functions and append-only audit
DO $$
DECLARE r text;
BEGIN
    FOREACH r IN ARRAY ARRAY['core_api', 'rule_service', 'graph_service', 'ml_service', 'llm_service', 'ingest_service'] LOOP
        PERFORM pg_temp.grant_if_role(r, 'GRANT USAGE ON SCHEMA core TO {role}');
        PERFORM pg_temp.grant_if_role(r, 'GRANT EXECUTE ON FUNCTION core.current_tenant() TO {role}');
        PERFORM pg_temp.grant_if_role(r, 'GRANT SELECT ON core.projects TO {role}');
        PERFORM pg_temp.grant_if_role(r, 'GRANT INSERT ON core.audit_log TO {role}');
        PERFORM pg_temp.grant_if_role(r, 'GRANT USAGE ON SEQUENCE core.audit_log_id_seq TO {role}');
    END LOOP;
END
$$;

-- core-api: owns core
SELECT pg_temp.grant_if_role('core_api', 'GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA core TO {role}');
SELECT pg_temp.grant_if_role('core_api', 'GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA core TO {role}');
SELECT pg_temp.grant_if_role('core_api', 'GRANT EXECUTE ON FUNCTION core.resolve_source_key(text) TO {role}');
SELECT pg_temp.grant_if_role('core_api', 'ALTER DEFAULT PRIVILEGES IN SCHEMA core GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO {role}');

-- rule-service: owns rules; reads events/features/customers/labels for velocity, composite and backtests
SELECT pg_temp.grant_if_role('rule_service', 'GRANT USAGE ON SCHEMA rules TO {role}');
SELECT pg_temp.grant_if_role('rule_service', 'GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA rules TO {role}');
SELECT pg_temp.grant_if_role('rule_service', 'GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA rules TO {role}');
SELECT pg_temp.grant_if_role('rule_service', 'ALTER DEFAULT PRIVILEGES IN SCHEMA rules GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO {role}');
SELECT pg_temp.grant_if_role('rule_service', 'GRANT SELECT ON core.events, core.event_features, core.customers, core.labels, core.event_labels, core.field_catalog, core.decisions TO {role}');
SELECT pg_temp.grant_if_role('rule_service', 'GRANT SELECT, INSERT, UPDATE ON core.approvals TO {role}');

-- graph-service: owns graph; reads community stats
SELECT pg_temp.grant_if_role('graph_service', 'GRANT USAGE ON SCHEMA graph TO {role}');
SELECT pg_temp.grant_if_role('graph_service', 'GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA graph TO {role}');
SELECT pg_temp.grant_if_role('graph_service', 'GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA graph TO {role}');
SELECT pg_temp.grant_if_role('graph_service', 'ALTER DEFAULT PRIVILEGES IN SCHEMA graph GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO {role}');
SELECT pg_temp.grant_if_role('graph_service', 'GRANT USAGE ON SCHEMA ml TO {role}');
SELECT pg_temp.grant_if_role('graph_service', 'GRANT SELECT ON ml.graph_communities, ml.graph_community_stats TO {role}');

-- ml-service: owns ml; reads training data
SELECT pg_temp.grant_if_role('ml_service', 'GRANT USAGE ON SCHEMA ml TO {role}');
SELECT pg_temp.grant_if_role('ml_service', 'GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA ml TO {role}');
SELECT pg_temp.grant_if_role('ml_service', 'ALTER DEFAULT PRIVILEGES IN SCHEMA ml GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO {role}');
SELECT pg_temp.grant_if_role('ml_service', 'GRANT SELECT ON core.events, core.event_features, core.customers, core.labels, core.event_labels TO {role}');
SELECT pg_temp.grant_if_role('ml_service', 'GRANT SELECT, INSERT, UPDATE ON core.approvals TO {role}');

-- llm-service: owns llm
SELECT pg_temp.grant_if_role('llm_service', 'GRANT USAGE ON SCHEMA llm TO {role}');
SELECT pg_temp.grant_if_role('llm_service', 'GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA llm TO {role}');
SELECT pg_temp.grant_if_role('llm_service', 'GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA llm TO {role}');
SELECT pg_temp.grant_if_role('llm_service', 'ALTER DEFAULT PRIVILEGES IN SCHEMA llm GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO {role}');

-- ingest-service: owns ingest; reads source config, updates cursor/schema
SELECT pg_temp.grant_if_role('ingest_service', 'GRANT USAGE ON SCHEMA ingest TO {role}');
SELECT pg_temp.grant_if_role('ingest_service', 'GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA ingest TO {role}');
SELECT pg_temp.grant_if_role('ingest_service', 'ALTER DEFAULT PRIVILEGES IN SCHEMA ingest GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO {role}');
SELECT pg_temp.grant_if_role('ingest_service', 'GRANT SELECT ON core.data_sources, core.data_source_mappings TO {role}');
SELECT pg_temp.grant_if_role('ingest_service', 'GRANT UPDATE (cursor_state, inferred_schema) ON core.data_sources TO {role}');
