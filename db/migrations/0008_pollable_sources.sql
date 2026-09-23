-- 0008_pollable_sources.sql — cross-tenant discovery of SQL pull connectors for ingest-service.
-- RLS prevents ingest_service from listing data sources without a tenant context, so the poller discovers
-- (tenant, project, source) routing tuples through this SECURITY DEFINER function, then sets app.tenant_id per source.
-- Only routing ids are returned (no connection details); those are read afterwards under the tenant's RLS context.

CREATE OR REPLACE FUNCTION core.list_pollable_sources()
RETURNS TABLE (tenant_id uuid, project_id uuid, data_source_id uuid)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = core, pg_temp AS $$
    SELECT ds.tenant_id, ds.project_id, ds.id
    FROM core.data_sources ds
    JOIN core.projects p ON p.id = ds.project_id AND p.status = 'active'
    JOIN core.tenants  t ON t.id = ds.tenant_id  AND t.status = 'active'
    WHERE ds.is_active
      AND ds.kind IN ('postgres', 'mysql')
      AND COALESCE((ds.connection -> 'poll' ->> 'enabled')::boolean, false)
$$;

REVOKE ALL ON FUNCTION core.list_pollable_sources() FROM PUBLIC;
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'ingest_service') THEN
        GRANT EXECUTE ON FUNCTION core.list_pollable_sources() TO ingest_service;
    END IF;
END
$$;
