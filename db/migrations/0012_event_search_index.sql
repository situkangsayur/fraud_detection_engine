-- 0012_event_search_index.sql — event search by external id (exact or prefix).
-- The event list filter `q` matched `external_id LIKE 'x%'` without a usable index and timed out (5 s statement
-- timeout) once a project held tens of thousands of events. The query now searches the prefix as a range with the
-- text_pattern_ops operators (~>=~ / ~<~), which this index serves even with bind parameters.
CREATE INDEX IF NOT EXISTS ix_events_external_prefix ON core.events (project_id, external_id text_pattern_ops);
