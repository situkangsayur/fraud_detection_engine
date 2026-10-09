-- 0001_foundation.sql — schemas, extensions, tenancy primitives, identity, projects, audit.
-- Run by the `migrate` job as role `migrator` (owner of every schema). Services connect as non-owner roles,
-- so Row-Level Security always applies to them. See docs/technical/multi-tenancy.md.

CREATE EXTENSION IF NOT EXISTS pgcrypto;
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS citext;

CREATE SCHEMA IF NOT EXISTS core;
CREATE SCHEMA IF NOT EXISTS rules;
CREATE SCHEMA IF NOT EXISTS graph;
CREATE SCHEMA IF NOT EXISTS ml;
CREATE SCHEMA IF NOT EXISTS llm;
CREATE SCHEMA IF NOT EXISTS ingest;

-- ---------------------------------------------------------------------------
-- Helpers
-- ---------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION core.set_updated_at() RETURNS trigger AS $$
BEGIN
    NEW.updated_at = now();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Tenant of the current transaction; NULL when unset → RLS matches nothing (fail closed).
CREATE OR REPLACE FUNCTION core.current_tenant() RETURNS uuid AS $$
    SELECT NULLIF(current_setting('app.tenant_id', true), '')::uuid
$$ LANGUAGE sql STABLE;

-- ---------------------------------------------------------------------------
-- Tenants & identity (no RLS: platform-level tables, access controlled by core-api)
-- ---------------------------------------------------------------------------
CREATE TABLE core.tenants (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    slug        text NOT NULL UNIQUE CHECK (slug ~ '^[a-z0-9][a-z0-9-]{1,62}$'),
    name        text NOT NULL,
    status      text NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'suspended')),
    settings    jsonb NOT NULL DEFAULT '{}',
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);
CREATE TRIGGER trg_tenants_updated BEFORE UPDATE ON core.tenants FOR EACH ROW EXECUTE FUNCTION core.set_updated_at();

CREATE TABLE core.app_users (
    id                 uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id          uuid REFERENCES core.tenants(id) ON DELETE CASCADE,   -- NULL only for platform admins
    email              citext NOT NULL UNIQUE,
    full_name          text NOT NULL,
    password_hash      text NOT NULL,                                        -- argon2id
    tenant_role        text NOT NULL DEFAULT 'member' CHECK (tenant_role IN ('tenant_admin', 'member')),
    is_platform_admin  boolean NOT NULL DEFAULT false,
    is_active          boolean NOT NULL DEFAULT true,
    last_login_at      timestamptz,
    created_at         timestamptz NOT NULL DEFAULT now(),
    updated_at         timestamptz NOT NULL DEFAULT now(),
    CHECK (tenant_id IS NOT NULL OR is_platform_admin)
);
CREATE TRIGGER trg_app_users_updated BEFORE UPDATE ON core.app_users FOR EACH ROW EXECUTE FUNCTION core.set_updated_at();
CREATE INDEX ix_app_users_tenant ON core.app_users (tenant_id);

CREATE TABLE core.refresh_tokens (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id      uuid NOT NULL REFERENCES core.app_users(id) ON DELETE CASCADE,
    token_hash   text NOT NULL UNIQUE,              -- sha256 of the opaque token
    family_id    uuid NOT NULL,                     -- rotation family; reuse of a revoked token revokes the family
    expires_at   timestamptz NOT NULL,
    revoked_at   timestamptz,
    created_at   timestamptz NOT NULL DEFAULT now(),
    user_agent   text,
    ip_address   inet
);
CREATE INDEX ix_refresh_tokens_user ON core.refresh_tokens (user_id);

-- ---------------------------------------------------------------------------
-- Projects (tenant-scoped, RLS)
-- ---------------------------------------------------------------------------
CREATE TABLE core.projects (
    id                uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id         uuid NOT NULL REFERENCES core.tenants(id) ON DELETE CASCADE,
    slug              text NOT NULL CHECK (slug ~ '^[a-z0-9][a-z0-9-]{1,62}$'),
    name              text NOT NULL,
    description       text,
    stage             text NOT NULL DEFAULT 'custom'
                      CHECK (stage IN ('pre_payment', 'post_payment', 'returns', 'promo', 'account_security', 'payout', 'custom')),
    business_context  text,
    timezone          text NOT NULL DEFAULT 'Asia/Jakarta',
    currency          char(3) NOT NULL DEFAULT 'IDR',
    status            text NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'archived')),
    ml_config         jsonb NOT NULL DEFAULT '{
        "supervised":   {"algorithm": "mlp_backprop", "params": {}},
        "unsupervised": {"anomaly_algorithm": "isolation_forest", "anomaly_params": {},
                         "clustering_algorithm": "hdbscan", "clustering_params": {}},
        "features":     {"include": ["*"], "exclude": [], "extra_source_fields": []}}',
    llm_config        jsonb NOT NULL DEFAULT '{"chat_model": null, "temperature": 0.1, "language": "id", "system_prompt_extra": ""}',
    graph_config      jsonb NOT NULL DEFAULT '{
        "link_kinds": ["email","phone","device","card","bank_account","address","ref_transaction"],
        "include_similar": true, "max_depth": 3, "supernode_degree_cap": 50, "similarity_threshold": 0.85}',
    created_by        uuid REFERENCES core.app_users(id),
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, slug),
    UNIQUE (tenant_id, id)            -- target of composite FKs (tenant_id, project_id)
);
CREATE TRIGGER trg_projects_updated BEFORE UPDATE ON core.projects FOR EACH ROW EXECUTE FUNCTION core.set_updated_at();

CREATE TABLE core.project_members (
    tenant_id   uuid NOT NULL,
    project_id  uuid NOT NULL,
    user_id     uuid NOT NULL REFERENCES core.app_users(id) ON DELETE CASCADE,
    role        text NOT NULL CHECK (role IN ('project_admin', 'approver', 'analyst', 'viewer')),
    created_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, user_id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES core.projects (tenant_id, id) ON DELETE CASCADE
);
CREATE INDEX ix_project_members_user ON core.project_members (user_id);

-- Project settings; defaults are inserted by core-api when a project is created (see settings defaults in code).
-- Keys: decision_thresholds, engine_weights, graph_scores, timeouts, cases, rules_unavailable_decision
CREATE TABLE core.project_settings (
    tenant_id   uuid NOT NULL,
    project_id  uuid NOT NULL,
    key         text NOT NULL,
    value       jsonb NOT NULL,
    updated_by  uuid REFERENCES core.app_users(id),
    updated_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, key),
    FOREIGN KEY (tenant_id, project_id) REFERENCES core.projects (tenant_id, id) ON DELETE CASCADE
);

-- ---------------------------------------------------------------------------
-- Maker–checker trail (shared by core, rules, ml — each writes its own subject types)
-- ---------------------------------------------------------------------------
CREATE TABLE core.approvals (
    id               uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id        uuid NOT NULL,
    project_id       uuid NOT NULL,
    subject_type     text NOT NULL CHECK (subject_type IN ('rule', 'ruleset', 'model', 'proposal', 'mapping')),
    subject_id       uuid NOT NULL,
    subject_version  int,
    requested_by     uuid REFERENCES core.app_users(id),
    requested_at     timestamptz NOT NULL DEFAULT now(),
    decided_by       uuid REFERENCES core.app_users(id),
    decided_at       timestamptz,
    decision         text CHECK (decision IN ('approved', 'rejected')),
    target_status    text,
    comment          text,
    FOREIGN KEY (tenant_id, project_id) REFERENCES core.projects (tenant_id, id) ON DELETE CASCADE,
    CHECK (decided_by IS NULL OR requested_by IS NULL OR decided_by <> requested_by)
);
CREATE INDEX ix_approvals_subject ON core.approvals (subject_type, subject_id);
CREATE INDEX ix_approvals_pending ON core.approvals (project_id) WHERE decision IS NULL;

-- ---------------------------------------------------------------------------
-- Audit (append-only). tenant_id NULL = platform-level action.
-- ---------------------------------------------------------------------------
CREATE TABLE core.audit_log (
    id            bigserial PRIMARY KEY,
    occurred_at   timestamptz NOT NULL DEFAULT now(),
    tenant_id     uuid,
    project_id    uuid,
    actor_type    text NOT NULL CHECK (actor_type IN ('user', 'service', 'system')),
    actor_id      text,
    action        text NOT NULL,              -- e.g. rule.create, rule.approve, project.settings.update
    subject_type  text,
    subject_id    text,
    before        jsonb,
    after         jsonb,
    metadata      jsonb NOT NULL DEFAULT '{}',
    request_id    text
);
CREATE INDEX ix_audit_tenant_time ON core.audit_log (tenant_id, occurred_at DESC);
CREATE INDEX ix_audit_subject ON core.audit_log (subject_type, subject_id);

CREATE OR REPLACE FUNCTION core.audit_log_immutable() RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'audit_log is append-only';
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER trg_audit_no_update BEFORE UPDATE OR DELETE ON core.audit_log
    FOR EACH ROW EXECUTE FUNCTION core.audit_log_immutable();
