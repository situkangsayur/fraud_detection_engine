-- 0011_ruleset_versions.sql — rulesets become versioned + approval-gated, like rules.
-- A ruleset version is an immutable snapshot of its config AND membership. What serves is decided by the
-- approval ledger (core.approvals, subject_type 'ruleset', subject_version), so editing a live ruleset creates a
-- new draft version while the last approved version keeps serving.

CREATE TABLE rules.ruleset_versions (
    ruleset_id   uuid NOT NULL REFERENCES rules.rulesets(id) ON DELETE CASCADE,
    tenant_id    uuid NOT NULL,
    version      int  NOT NULL,
    config       jsonb NOT NULL,   -- {name, description, event_types, typologies, aggregation, max_score,
                                   --  members: [{rule_id, weight, pinned_version, position}]}
    change_note  text,
    created_by   uuid,
    created_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (ruleset_id, version)
);

ALTER TABLE rules.ruleset_versions ENABLE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON rules.ruleset_versions
    USING (tenant_id = core.current_tenant()) WITH CHECK (tenant_id = core.current_tenant());

-- Backfill: current config + membership of existing rulesets becomes their current version.
INSERT INTO rules.ruleset_versions (ruleset_id, tenant_id, version, config, created_by, created_at)
SELECT rs.id, rs.tenant_id, rs.version,
       jsonb_build_object(
           'name', rs.name, 'description', rs.description, 'event_types', to_jsonb(rs.event_types),
           'typologies', to_jsonb(rs.typologies), 'aggregation', rs.aggregation, 'max_score', rs.max_score,
           'members', COALESCE((SELECT jsonb_agg(jsonb_build_object('rule_id', m.rule_id, 'weight', m.weight,
                                                   'pinned_version', m.pinned_version, 'position', m.position)
                                                 ORDER BY m.position)
                                FROM rules.ruleset_rules m WHERE m.ruleset_id = rs.id), '[]'::jsonb)),
       rs.created_by, rs.created_at
FROM rules.rulesets rs
ON CONFLICT DO NOTHING;

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'rule_service') THEN
        GRANT SELECT, INSERT, UPDATE, DELETE ON rules.ruleset_versions TO rule_service;
        -- ruleset backtests use the project's decision thresholds
        GRANT SELECT ON core.project_settings TO rule_service;
    END IF;
END
$$;
