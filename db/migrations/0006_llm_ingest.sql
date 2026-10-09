-- 0006_llm_ingest.sql — LLM (owner: llm-service) and ingest jobs (owner: ingest-service).

-- ---------------------------------------------------------------------------
-- Regulation / policy library (tenant level) + project attachment
-- ---------------------------------------------------------------------------
CREATE TABLE llm.regulations (
    id               uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id        uuid NOT NULL REFERENCES core.tenants(id) ON DELETE CASCADE,
    code             text NOT NULL,                 -- e.g. POJK-12-2024, SOP-VOUCHER-2025
    title            text NOT NULL,
    doc_type         text NOT NULL DEFAULT 'regulation' CHECK (doc_type IN ('regulation', 'internal_policy', 'sop', 'other')),
    issuer           text NOT NULL,                 -- OJK, BI, internal, …
    version          int NOT NULL DEFAULT 1,
    effective_date   date,
    supersedes_id    uuid REFERENCES llm.regulations(id),
    file_name        text NOT NULL,
    file_sha256      text NOT NULL,
    file_path        text NOT NULL,
    status           text NOT NULL DEFAULT 'processing'
                     CHECK (status IN ('processing', 'indexed', 'failed', 'superseded')),
    chunk_count      int NOT NULL DEFAULT 0,
    summary          text,
    error            text,
    uploaded_by      uuid,
    created_at       timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, code, version),
    UNIQUE (tenant_id, file_sha256)
);

CREATE TABLE llm.project_regulations (
    tenant_id      uuid NOT NULL,
    project_id     uuid NOT NULL,
    regulation_id  uuid NOT NULL REFERENCES llm.regulations(id) ON DELETE CASCADE,
    attached_by    uuid,
    attached_at    timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, regulation_id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES core.projects (tenant_id, id) ON DELETE CASCADE
);

CREATE TABLE llm.regulation_changes (
    id                      uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id               uuid NOT NULL,
    regulation_id           uuid NOT NULL REFERENCES llm.regulations(id) ON DELETE CASCADE,
    previous_regulation_id  uuid NOT NULL REFERENCES llm.regulations(id) ON DELETE CASCADE,
    changed_sections        jsonb NOT NULL DEFAULT '[]',   -- [{section, change:"added|removed|modified", before, after}]
    diff_summary            text,
    created_at              timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE llm.reports (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id     uuid NOT NULL,
    project_id    uuid NOT NULL,
    report_type   text NOT NULL CHECK (report_type IN ('rule_relevance', 'fraud_situation', 'regulation_impact', 'recommend_rules')),
    title         text NOT NULL,
    status        text NOT NULL DEFAULT 'running' CHECK (status IN ('running', 'done', 'failed')),
    params        jsonb NOT NULL DEFAULT '{}',
    content_md    text,
    structured    jsonb,
    model         text,
    error         text,
    created_by    uuid,
    created_at    timestamptz NOT NULL DEFAULT now(),
    finished_at   timestamptz,
    FOREIGN KEY (tenant_id, project_id) REFERENCES core.projects (tenant_id, id) ON DELETE CASCADE
);
CREATE INDEX ix_llm_reports_project ON llm.reports (project_id, created_at DESC);

CREATE TABLE llm.conversations (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id   uuid NOT NULL,
    project_id  uuid NOT NULL,
    user_id     uuid,
    title       text,
    created_at  timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (tenant_id, project_id) REFERENCES core.projects (tenant_id, id) ON DELETE CASCADE
);

CREATE TABLE llm.messages (
    id               bigserial PRIMARY KEY,
    tenant_id        uuid NOT NULL,
    conversation_id  uuid NOT NULL REFERENCES llm.conversations(id) ON DELETE CASCADE,
    role             text NOT NULL CHECK (role IN ('system', 'user', 'assistant', 'tool')),
    content          text NOT NULL,
    tool_calls       jsonb,
    citations        jsonb,
    created_at       timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ix_llm_messages_conv ON llm.messages (conversation_id, id);

-- ---------------------------------------------------------------------------
-- Ingest jobs (owner: ingest-service)
-- ---------------------------------------------------------------------------
CREATE TABLE ingest.jobs (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id       uuid NOT NULL,
    project_id      uuid NOT NULL,
    data_source_id  uuid NOT NULL,          -- core.data_sources.id (cross-service, no FK)
    mode            text NOT NULL CHECK (mode IN ('score', 'load_only')),
    upload_id       text,
    status          text NOT NULL DEFAULT 'queued' CHECK (status IN ('queued', 'running', 'done', 'failed', 'cancelled')),
    total_rows      bigint,
    processed_rows  bigint NOT NULL DEFAULT 0,
    accepted_rows   bigint NOT NULL DEFAULT 0,
    rejected_rows   bigint NOT NULL DEFAULT 0,
    error           text,
    created_by      uuid,
    created_at      timestamptz NOT NULL DEFAULT now(),
    started_at      timestamptz,
    finished_at     timestamptz,
    FOREIGN KEY (tenant_id, project_id) REFERENCES core.projects (tenant_id, id) ON DELETE CASCADE
);
CREATE INDEX ix_ingest_jobs_source ON ingest.jobs (data_source_id, created_at DESC);

CREATE TABLE ingest.uploads (
    id              text PRIMARY KEY,       -- random token
    tenant_id       uuid NOT NULL,
    project_id      uuid NOT NULL,
    data_source_id  uuid NOT NULL,
    file_name       text NOT NULL,
    file_path       text NOT NULL,
    file_format     text NOT NULL,
    size_bytes      bigint NOT NULL,
    row_estimate    bigint,
    expires_at      timestamptz NOT NULL,
    created_at      timestamptz NOT NULL DEFAULT now()
);
