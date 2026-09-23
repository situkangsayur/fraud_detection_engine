-- 0002_core_data.sql — data sources & mapping, customers, events, features, decisions, cases, labels.
-- Owner service: core-api. All tables project-scoped (tenant_id + project_id, composite FK to core.projects).

-- ---------------------------------------------------------------------------
-- Data sources & mappings (pluggable data — docs/technical/data-sources.md)
-- ---------------------------------------------------------------------------
CREATE TABLE core.data_sources (
    id                  uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id           uuid NOT NULL,
    project_id          uuid NOT NULL,
    slug                text NOT NULL CHECK (slug ~ '^[a-z0-9][a-z0-9_-]{1,62}$'),
    name                text NOT NULL,
    description         text,
    kind                text NOT NULL CHECK (kind IN ('webhook', 'file', 'postgres', 'mysql', 'internal')),
    default_event_type  text,
    mode                text NOT NULL DEFAULT 'score' CHECK (mode IN ('score', 'load_only')),
    connection          jsonb NOT NULL DEFAULT '{}',          -- never contains passwords (password_env)
    cursor_state        jsonb NOT NULL DEFAULT '{}',
    inferred_schema     jsonb,
    api_key_prefix      text UNIQUE,                          -- first 12 chars of the key, lookup index
    api_key_hash        text,                                 -- argon2id of the full key
    is_active           boolean NOT NULL DEFAULT true,
    created_by          uuid REFERENCES core.app_users(id),
    created_at          timestamptz NOT NULL DEFAULT now(),
    updated_at          timestamptz NOT NULL DEFAULT now(),
    UNIQUE (project_id, slug),
    FOREIGN KEY (tenant_id, project_id) REFERENCES core.projects (tenant_id, id) ON DELETE CASCADE
);
CREATE TRIGGER trg_data_sources_updated BEFORE UPDATE ON core.data_sources FOR EACH ROW EXECUTE FUNCTION core.set_updated_at();

-- Webhook key resolution before any tenant context exists (bypasses RLS, returns only routing fields).
CREATE OR REPLACE FUNCTION core.resolve_source_key(p_prefix text)
RETURNS TABLE (tenant_id uuid, project_id uuid, data_source_id uuid, slug text, api_key_hash text, is_active boolean)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = core, pg_temp AS $$
    SELECT ds.tenant_id, ds.project_id, ds.id, ds.slug, ds.api_key_hash, ds.is_active AND p.status = 'active' AND t.status = 'active'
    FROM core.data_sources ds
    JOIN core.projects p ON p.id = ds.project_id
    JOIN core.tenants t ON t.id = ds.tenant_id
    WHERE ds.api_key_prefix = p_prefix
$$;

CREATE TABLE core.data_source_mappings (
    tenant_id       uuid NOT NULL,
    project_id      uuid NOT NULL,
    data_source_id  uuid NOT NULL REFERENCES core.data_sources(id) ON DELETE CASCADE,
    version         int  NOT NULL,
    mapping         jsonb NOT NULL,
    status          text NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'active', 'archived')),
    created_by      uuid REFERENCES core.app_users(id),
    created_at      timestamptz NOT NULL DEFAULT now(),
    activated_by    uuid REFERENCES core.app_users(id),
    activated_at    timestamptz,
    PRIMARY KEY (data_source_id, version),
    FOREIGN KEY (tenant_id, project_id) REFERENCES core.projects (tenant_id, id) ON DELETE CASCADE
);
CREATE UNIQUE INDEX uq_mapping_active ON core.data_source_mappings (data_source_id) WHERE status = 'active';

-- Source-specific fields only. Built-in paths (event.*, customer.*, features.*, ml.*, graph.*) are defined in code
-- (Rust `contracts::catalog`) and merged at read time.
CREATE TABLE core.field_catalog (
    tenant_id         uuid NOT NULL,
    project_id        uuid NOT NULL,
    path              text NOT NULL,                             -- source.order.total
    data_type         text NOT NULL CHECK (data_type IN ('integer', 'number', 'string', 'bool', 'datetime', 'array', 'object')),
    description       text,
    data_source_id    uuid REFERENCES core.data_sources(id) ON DELETE CASCADE,
    velocity_enabled  boolean NOT NULL DEFAULT false,
    pii               boolean NOT NULL DEFAULT false,
    sample_values     jsonb,
    created_at        timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, path),
    FOREIGN KEY (tenant_id, project_id) REFERENCES core.projects (tenant_id, id) ON DELETE CASCADE
);

CREATE TABLE core.ingest_errors (
    id              bigserial PRIMARY KEY,
    tenant_id       uuid NOT NULL,
    project_id      uuid NOT NULL,
    data_source_id  uuid NOT NULL REFERENCES core.data_sources(id) ON DELETE CASCADE,
    job_id          uuid,                                          -- ingest.jobs.id (cross-service, no FK)
    record          jsonb NOT NULL,
    reason          text NOT NULL,
    created_at      timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (tenant_id, project_id) REFERENCES core.projects (tenant_id, id) ON DELETE CASCADE
);
CREATE INDEX ix_ingest_errors_source ON core.ingest_errors (data_source_id, created_at DESC);

-- ---------------------------------------------------------------------------
-- Customers & events
-- ---------------------------------------------------------------------------
CREATE TABLE core.customers (
    id                uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id         uuid NOT NULL,
    project_id        uuid NOT NULL,
    external_id       text NOT NULL,
    full_name         text,
    email             text,
    email_normalized  text,
    phone             text,
    phone_normalized  text,
    kyc_level         smallint,
    segment           text,
    status            text NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'suspended', 'closed')),
    risk_label        text NOT NULL DEFAULT 'unknown' CHECK (risk_label IN ('fraud', 'legit', 'unknown')),
    registered_at     timestamptz,
    attributes        jsonb NOT NULL DEFAULT '{}',
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now(),
    UNIQUE (project_id, external_id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES core.projects (tenant_id, id) ON DELETE CASCADE
);
CREATE TRIGGER trg_customers_updated BEFORE UPDATE ON core.customers FOR EACH ROW EXECUTE FUNCTION core.set_updated_at();
CREATE INDEX ix_customers_risk_label ON core.customers (project_id, risk_label) WHERE risk_label <> 'unknown';
CREATE INDEX ix_customers_name_trgm ON core.customers USING gin (full_name gin_trgm_ops);

CREATE TABLE core.events (
    id                      uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id               uuid NOT NULL,
    project_id              uuid NOT NULL,
    data_source_id          uuid NOT NULL REFERENCES core.data_sources(id),
    external_id             text NOT NULL,
    event_type              text NOT NULL,
    customer_id             uuid NOT NULL REFERENCES core.customers(id) ON DELETE CASCADE,
    occurred_at             timestamptz NOT NULL,
    received_at             timestamptz NOT NULL DEFAULT now(),
    channel                 text,
    status                  text,
    amount                  numeric(20, 2),
    currency                char(3),
    merchant_id             text,
    merchant_category       text,
    payment_method          text,
    instrument_fingerprint  text,
    card_bin                text,
    card_last4              text,
    issuer_country          char(2),
    recipient_fingerprint   text,
    device_id               text,
    ip_address              inet,
    user_agent              text,
    geo_country             char(2),
    geo_city                text,
    latitude                double precision,
    longitude               double precision,
    promo_code              text,
    discount_amount         numeric(20, 2),
    cashback_amount         numeric(20, 2),
    ref_transaction_id      text,
    shipping_address        text,
    billing_address         text,
    account_change_type     text,
    login_success           boolean,
    api_client_id           text,
    payload                 jsonb NOT NULL DEFAULT '{}',    -- raw source record minus dropped/PII fields → source.*
    load_only               boolean NOT NULL DEFAULT false,
    needs_rescore           boolean NOT NULL DEFAULT false,
    UNIQUE (data_source_id, external_id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES core.projects (tenant_id, id) ON DELETE CASCADE
);
-- Access paths for features and velocity (always project-leading)
CREATE INDEX ix_events_customer_time    ON core.events (project_id, customer_id, occurred_at DESC);
CREATE INDEX ix_events_type_time        ON core.events (project_id, event_type, occurred_at DESC);
CREATE INDEX ix_events_time             ON core.events (project_id, occurred_at DESC);
CREATE INDEX ix_events_instrument_time  ON core.events (project_id, instrument_fingerprint, occurred_at DESC) WHERE instrument_fingerprint IS NOT NULL;
CREATE INDEX ix_events_device_time      ON core.events (project_id, device_id, occurred_at DESC) WHERE device_id IS NOT NULL;
CREATE INDEX ix_events_ip_time          ON core.events (project_id, ip_address, occurred_at DESC) WHERE ip_address IS NOT NULL;
CREATE INDEX ix_events_promo_time       ON core.events (project_id, promo_code, occurred_at DESC) WHERE promo_code IS NOT NULL;
CREATE INDEX ix_events_recipient_time   ON core.events (project_id, recipient_fingerprint, occurred_at DESC) WHERE recipient_fingerprint IS NOT NULL;
CREATE INDEX ix_events_api_client_time  ON core.events (project_id, api_client_id, occurred_at DESC) WHERE api_client_id IS NOT NULL;
CREATE INDEX ix_events_merchant_time    ON core.events (project_id, merchant_id, occurred_at DESC) WHERE merchant_id IS NOT NULL;
CREATE INDEX ix_events_ref_txn          ON core.events (project_id, ref_transaction_id) WHERE ref_transaction_id IS NOT NULL;
CREATE INDEX ix_events_rescore          ON core.events (project_id) WHERE needs_rescore;
CREATE INDEX ix_events_payload          ON core.events USING gin (payload jsonb_path_ops);

CREATE TABLE core.event_features (
    event_id             uuid PRIMARY KEY REFERENCES core.events(id) ON DELETE CASCADE,
    tenant_id            uuid NOT NULL,
    project_id           uuid NOT NULL,
    feature_set_version  int NOT NULL,
    features             jsonb NOT NULL,
    computed_at          timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (tenant_id, project_id) REFERENCES core.projects (tenant_id, id) ON DELETE CASCADE
);
CREATE INDEX ix_event_features_project ON core.event_features (project_id);

-- ---------------------------------------------------------------------------
-- Decisions, cases, labels
-- ---------------------------------------------------------------------------
CREATE TABLE core.decisions (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id       uuid NOT NULL,
    project_id      uuid NOT NULL,
    event_id        uuid NOT NULL UNIQUE REFERENCES core.events(id) ON DELETE CASCADE,
    decision        text NOT NULL CHECK (decision IN ('approve', 'review', 'decline')),
    final_score     real NOT NULL,
    engine_scores   jsonb NOT NULL,     -- {rules, supervised, unsupervised, graph}
    ml              jsonb NOT NULL DEFAULT '{}',
    graph           jsonb NOT NULL DEFAULT '{}',
    reasons         jsonb NOT NULL DEFAULT '[]',
    rule_results    jsonb NOT NULL DEFAULT '[]',
    degraded        text[] NOT NULL DEFAULT '{}',
    latency_ms      int NOT NULL,
    created_at      timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (tenant_id, project_id) REFERENCES core.projects (tenant_id, id) ON DELETE CASCADE
);
CREATE INDEX ix_decisions_project_time ON core.decisions (project_id, created_at DESC);
CREATE INDEX ix_decisions_project_decision ON core.decisions (project_id, decision, created_at DESC);

CREATE TABLE core.cases (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id     uuid NOT NULL,
    project_id    uuid NOT NULL,
    customer_id   uuid NOT NULL REFERENCES core.customers(id) ON DELETE CASCADE,
    event_id      uuid REFERENCES core.events(id) ON DELETE SET NULL,
    decision_id   uuid REFERENCES core.decisions(id) ON DELETE SET NULL,
    status        text NOT NULL DEFAULT 'open'
                  CHECK (status IN ('open', 'in_review', 'resolved_fraud', 'resolved_legit')),
    priority      smallint NOT NULL DEFAULT 3 CHECK (priority BETWEEN 1 AND 5),
    typologies    text[] NOT NULL DEFAULT '{}',
    assigned_to   uuid REFERENCES core.app_users(id),
    notes         jsonb NOT NULL DEFAULT '[]',    -- [{at, by, text}]
    event_ids     uuid[] NOT NULL DEFAULT '{}',
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    resolved_at   timestamptz,
    FOREIGN KEY (tenant_id, project_id) REFERENCES core.projects (tenant_id, id) ON DELETE CASCADE
);
CREATE TRIGGER trg_cases_updated BEFORE UPDATE ON core.cases FOR EACH ROW EXECUTE FUNCTION core.set_updated_at();
CREATE INDEX ix_cases_project_status ON core.cases (project_id, status, priority, created_at DESC);
CREATE UNIQUE INDEX uq_cases_open_customer ON core.cases (customer_id) WHERE status IN ('open', 'in_review');

CREATE TABLE core.labels (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id     uuid NOT NULL,
    project_id    uuid NOT NULL,
    subject_type  text NOT NULL CHECK (subject_type IN ('event', 'customer')),
    subject_id    uuid NOT NULL,
    label         text NOT NULL CHECK (label IN ('fraud', 'legit')),
    fraud_type    text CHECK (fraud_type IN ('carding', 'account_takeover', 'bank_account_takeover', 'system_breach',
                                             'promo_abuse', 'refund_abuse', 'money_mule', 'other')),
    source        text NOT NULL CHECK (source IN ('analyst', 'chargeback', 'customer_report', 'dataset', 'system')),
    notes         text,
    created_by    uuid REFERENCES core.app_users(id),
    created_at    timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (tenant_id, project_id) REFERENCES core.projects (tenant_id, id) ON DELETE CASCADE
);
CREATE INDEX ix_labels_subject ON core.labels (subject_type, subject_id, created_at DESC);
CREATE INDEX ix_labels_project ON core.labels (project_id, created_at DESC);

-- Latest label per event (training, backtests). security_invoker → RLS of core.labels applies.
CREATE VIEW core.event_labels WITH (security_invoker = true) AS
SELECT DISTINCT ON (subject_id)
       subject_id AS event_id, tenant_id, project_id, label, fraud_type, source, created_at
FROM core.labels
WHERE subject_type = 'event'
ORDER BY subject_id, created_at DESC;
