-- 0010_core_summary_reads.sql — core-api reads rule/model status for the project summary
-- (GET /projects/{pid} → summary.active_rules, summary.active_models). Read-only, status columns only,
-- documented cross-service read like rule-service → core.events.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'core_api') THEN
        GRANT USAGE ON SCHEMA rules, ml TO core_api;
        GRANT SELECT (id, tenant_id, project_id, status) ON rules.rules TO core_api;
        GRANT SELECT (id, tenant_id, project_id, kind, status) ON ml.models TO core_api;
    END IF;
END
$$;
