-- 0003_rules.sql — rule engine schema. Owner service: rule-service.

CREATE TABLE rules.rules (
    id               uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id        uuid NOT NULL,
    project_id       uuid NOT NULL,
    code             text NOT NULL CHECK (code ~ '^[A-Z0-9-]{3,40}$'),
    name             text NOT NULL,
    description      text,
    kind             text NOT NULL CHECK (kind IN ('simple', 'velocity', 'composite', 'reference', 'graph')),
    typologies       text[] NOT NULL DEFAULT '{}',
    event_types      text[] NOT NULL DEFAULT '{}',
    current_version  int NOT NULL DEFAULT 1,
    status           text NOT NULL DEFAULT 'draft'
                     CHECK (status IN ('draft', 'pending_approval', 'active', 'shadow', 'retired')),
    submitted_by     uuid,
    submitted_at     timestamptz,
    created_by       uuid,
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz NOT NULL DEFAULT now(),
    UNIQUE (project_id, code),
    FOREIGN KEY (tenant_id, project_id) REFERENCES core.projects (tenant_id, id) ON DELETE CASCADE
);
CREATE TRIGGER trg_rules_updated BEFORE UPDATE ON rules.rules FOR EACH ROW EXECUTE FUNCTION core.set_updated_at();
CREATE INDEX ix_rules_project_status ON rules.rules (project_id, status);

CREATE TABLE rules.rule_versions (
    rule_id              uuid NOT NULL REFERENCES rules.rules(id) ON DELETE CASCADE,
    tenant_id            uuid NOT NULL,
    version              int NOT NULL,
    definition           jsonb NOT NULL,          -- kind-specific body (rule-dsl §6)
    risk_score           real NOT NULL CHECK (risk_score BETWEEN 0 AND 100),
    trapped_score        real NOT NULL DEFAULT 0 CHECK (trapped_score BETWEEN 0 AND 100),
    action               text NOT NULL DEFAULT 'score'
                         CHECK (action IN ('score', 'force_review', 'force_decline', 'force_approve')),
    on_trapped           text NOT NULL DEFAULT 'ignore' CHECK (on_trapped IN ('ignore', 'score', 'review')),
    missing_as_no_match  boolean NOT NULL DEFAULT false,
    change_note          text,
    created_by           uuid,
    created_at           timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (rule_id, version)
);

CREATE TABLE rules.rulesets (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id     uuid NOT NULL,
    project_id    uuid NOT NULL,
    code          text NOT NULL CHECK (code ~ '^[A-Z0-9-]{3,40}$'),
    name          text NOT NULL,
    description   text,
    event_types   text[] NOT NULL DEFAULT '{}',
    typologies    text[] NOT NULL DEFAULT '{}',
    aggregation   text NOT NULL DEFAULT 'probabilistic_or'
                  CHECK (aggregation IN ('sum', 'max', 'probabilistic_or', 'weighted_average')),
    max_score     real NOT NULL DEFAULT 100 CHECK (max_score BETWEEN 0 AND 100),
    version       int NOT NULL DEFAULT 1,
    status        text NOT NULL DEFAULT 'draft'
                  CHECK (status IN ('draft', 'pending_approval', 'active', 'shadow', 'retired')),
    submitted_by  uuid,
    submitted_at  timestamptz,
    created_by    uuid,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    UNIQUE (project_id, code),
    FOREIGN KEY (tenant_id, project_id) REFERENCES core.projects (tenant_id, id) ON DELETE CASCADE
);
CREATE TRIGGER trg_rulesets_updated BEFORE UPDATE ON rules.rulesets FOR EACH ROW EXECUTE FUNCTION core.set_updated_at();

CREATE TABLE rules.ruleset_rules (
    ruleset_id      uuid NOT NULL REFERENCES rules.rulesets(id) ON DELETE CASCADE,
    rule_id         uuid NOT NULL REFERENCES rules.rules(id) ON DELETE RESTRICT,
    tenant_id       uuid NOT NULL,
    weight          real NOT NULL DEFAULT 1.0 CHECK (weight >= 0 AND weight <= 10),
    pinned_version  int,
    position        int NOT NULL DEFAULT 0,
    PRIMARY KEY (ruleset_id, rule_id)
);

-- project_id NULL = tenant-wide list (usable by every project of the tenant)
CREATE TABLE rules.reference_lists (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id    uuid NOT NULL REFERENCES core.tenants(id) ON DELETE CASCADE,
    project_id   uuid,
    name         text NOT NULL CHECK (name ~ '^[a-z0-9][a-z0-9_]{1,62}$'),
    description  text,
    list_type    text NOT NULL CHECK (list_type IN ('blacklist', 'whitelist', 'watchlist', 'lookup')),
    key_kind     text NOT NULL DEFAULT 'generic',
    columns      jsonb NOT NULL DEFAULT '[]',        -- [{name, type}]
    created_by   uuid,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (tenant_id, project_id) REFERENCES core.projects (tenant_id, id) ON DELETE CASCADE
);
CREATE TRIGGER trg_reference_lists_updated BEFORE UPDATE ON rules.reference_lists FOR EACH ROW EXECUTE FUNCTION core.set_updated_at();
-- Name resolution in a project: project list first, then tenant-wide list with the same name.
CREATE UNIQUE INDEX uq_reflist_project_name ON rules.reference_lists (project_id, name) WHERE project_id IS NOT NULL;
CREATE UNIQUE INDEX uq_reflist_tenant_name  ON rules.reference_lists (tenant_id, name) WHERE project_id IS NULL;

CREATE TABLE rules.reference_entries (
    id           bigserial PRIMARY KEY,
    tenant_id    uuid NOT NULL,
    list_id      uuid NOT NULL REFERENCES rules.reference_lists(id) ON DELETE CASCADE,
    key          text NOT NULL,
    attributes   jsonb NOT NULL DEFAULT '{}',
    valid_from   timestamptz NOT NULL DEFAULT now(),
    valid_until  timestamptz,
    reason       text,
    created_by   uuid,
    created_at   timestamptz NOT NULL DEFAULT now(),
    UNIQUE (list_id, key)
);

-- One row per matched/trapped rule evaluation (fast analytics & LLM evidence). no_match rows are only counted.
CREATE TABLE rules.rule_hits (
    id            bigserial PRIMARY KEY,
    tenant_id     uuid NOT NULL,
    project_id    uuid NOT NULL,
    event_id      uuid NOT NULL,                 -- core.events.id (cross-service, no FK)
    rule_id       uuid NOT NULL REFERENCES rules.rules(id) ON DELETE CASCADE,
    rule_version  int NOT NULL,
    ruleset_id    uuid REFERENCES rules.rulesets(id) ON DELETE SET NULL,
    outcome       text NOT NULL CHECK (outcome IN ('match', 'trapped')),
    contribution  real NOT NULL DEFAULT 0,
    shadow        boolean NOT NULL DEFAULT false,
    occurred_at   timestamptz NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ix_rule_hits_rule_time ON rules.rule_hits (rule_id, occurred_at DESC);
CREATE INDEX ix_rule_hits_event ON rules.rule_hits (event_id);

CREATE TABLE rules.rule_eval_counters (
    tenant_id  uuid NOT NULL,
    rule_id    uuid NOT NULL REFERENCES rules.rules(id) ON DELETE CASCADE,
    day        date NOT NULL,
    evaluated  bigint NOT NULL DEFAULT 0,
    matched    bigint NOT NULL DEFAULT 0,
    trapped    bigint NOT NULL DEFAULT 0,
    PRIMARY KEY (rule_id, day)
);

CREATE TABLE rules.proposals (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id       uuid NOT NULL,
    project_id      uuid NOT NULL,
    source          text NOT NULL CHECK (source IN ('llm', 'analyst')),
    proposal_type   text NOT NULL CHECK (proposal_type IN ('new_rule', 'modify_rule', 'retire_rule', 'tune_threshold')),
    target_rule_id  uuid REFERENCES rules.rules(id) ON DELETE SET NULL,
    definition      jsonb,                  -- full rule envelope (rule-dsl §2); null for retire
    rationale       text NOT NULL,
    citations       jsonb NOT NULL DEFAULT '[]',
    evidence        jsonb NOT NULL DEFAULT '{}',
    validation      jsonb NOT NULL DEFAULT '{}',
    backtest        jsonb,
    report_id       uuid,                   -- llm.reports.id (cross-service, no FK)
    llm_model       text,
    status          text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'approved', 'rejected', 'applied')),
    created_by      uuid,
    created_at      timestamptz NOT NULL DEFAULT now(),
    reviewed_by     uuid,
    reviewed_at     timestamptz,
    review_comment  text,
    applied_rule_id uuid REFERENCES rules.rules(id) ON DELETE SET NULL,
    FOREIGN KEY (tenant_id, project_id) REFERENCES core.projects (tenant_id, id) ON DELETE CASCADE
);
CREATE INDEX ix_proposals_project_status ON rules.proposals (project_id, status, created_at DESC);
