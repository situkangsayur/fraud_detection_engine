-- 0009_user_fk_set_null.sql — "who did it" references to core.app_users must not block user/tenant deletion.
-- Every FK from a metadata column (created_by, decided_by, assigned_to, …) to core.app_users becomes
-- ON DELETE SET NULL. Memberships (project_members) and refresh tokens keep ON DELETE CASCADE.
-- The audit trail keeps the actor id as text in core.audit_log, so no history is lost.

DO $$
DECLARE
    r record;
BEGIN
    FOR r IN
        SELECT c.conname,
               c.conrelid::regclass AS tbl,
               a.attname           AS col
        FROM pg_constraint c
        JOIN pg_attribute a ON a.attrelid = c.conrelid AND a.attnum = c.conkey[1]
        WHERE c.contype = 'f'
          AND c.confrelid = 'core.app_users'::regclass
          AND array_length(c.conkey, 1) = 1
          AND c.confdeltype = 'a'            -- NO ACTION (the default) only; CASCADE ones are intentional
    LOOP
        EXECUTE format('ALTER TABLE %s DROP CONSTRAINT %I', r.tbl, r.conname);
        EXECUTE format('ALTER TABLE %s ADD CONSTRAINT %I FOREIGN KEY (%I) REFERENCES core.app_users(id) ON DELETE SET NULL',
                       r.tbl, r.conname, r.col);
    END LOOP;
END
$$;
