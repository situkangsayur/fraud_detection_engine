-- Used by deploy/demo/master.sh restore: moves every timestamptz value of the platform schemas forward by the time
-- elapsed since the master snapshot (demo_master.meta.snapshot_at), so restored data ends "now" again and the
-- dashboards (which default to recent windows) are never empty. Triggers such as the append-only audit log are
-- bypassed for this session only. Runs as the Postgres superuser.
SET session_replication_role = replica;

DO $$
DECLARE
  delta interval := now() - (SELECT value::timestamptz FROM demo_master.meta WHERE key = 'snapshot_at');
  r record;
  cols text;
BEGIN
  FOR r IN
    SELECT table_schema, table_name FROM information_schema.tables
     WHERE table_schema IN ('core', 'rules', 'graph', 'ml', 'llm', 'ingest') AND table_type = 'BASE TABLE'
  LOOP
    SELECT string_agg(format('%I = %I + %L::interval', column_name, column_name, delta), ', ')
      INTO cols
      FROM information_schema.columns
     WHERE table_schema = r.table_schema AND table_name = r.table_name
       AND data_type = 'timestamp with time zone';
    IF cols IS NOT NULL THEN
      EXECUTE format('UPDATE %I.%I SET %s', r.table_schema, r.table_name, cols);
    END IF;
  END LOOP;
  UPDATE demo_master.meta SET value = now()::text WHERE key = 'snapshot_at';
  RAISE NOTICE 'timestamps shifted by %', delta;
END $$;

DELETE FROM core.refresh_tokens;
