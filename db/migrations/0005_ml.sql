-- 0005_ml.sql — ML plugin registry, models, unsupervised results, graph communities. Owner service: ml-service.

-- Global plugin catalogue (platform level, no tenant) — docs/technical/ml-plugins.md
CREATE TABLE ml.algorithms (
    name          text PRIMARY KEY CHECK (name ~ '^[a-z][a-z0-9_]{2,40}$'),
    kind          text NOT NULL CHECK (kind IN ('supervised', 'anomaly', 'clustering')),
    version       text NOT NULL,
    display_name  text NOT NULL,
    description   text,
    param_schema  jsonb NOT NULL,
    source        text NOT NULL CHECK (source IN ('builtin', 'plugin')),
    module        text NOT NULL,
    status        text NOT NULL DEFAULT 'available' CHECK (status IN ('available', 'invalid', 'disabled')),
    error         text,
    loaded_at     timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE ml.models (
    id                    uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id             uuid NOT NULL,
    project_id            uuid NOT NULL,
    kind                  text NOT NULL CHECK (kind IN ('supervised', 'unsupervised')),
    version               int NOT NULL,
    algorithms            jsonb NOT NULL,     -- {"supervised": {name, version}} | {"anomaly": {...}, "clustering": {...}}
    params                jsonb NOT NULL DEFAULT '{}',
    feature_set_version   int NOT NULL,
    feature_names         jsonb NOT NULL DEFAULT '[]',
    metrics               jsonb NOT NULL DEFAULT '{}',
    training_history      jsonb NOT NULL DEFAULT '{}',
    artifact_path         text,
    status                text NOT NULL DEFAULT 'training'
                          CHECK (status IN ('training', 'ready', 'pending_approval', 'active', 'archived', 'failed')),
    progress              real NOT NULL DEFAULT 0,
    trained_rows          bigint,
    error                 text,
    submitted_by          uuid,
    created_by            uuid,
    training_started_at   timestamptz NOT NULL DEFAULT now(),
    training_finished_at  timestamptz,
    activated_at          timestamptz,
    UNIQUE (project_id, kind, version),
    FOREIGN KEY (tenant_id, project_id) REFERENCES core.projects (tenant_id, id) ON DELETE CASCADE
);
CREATE UNIQUE INDEX uq_ml_models_active ON ml.models (project_id, kind) WHERE status = 'active';

CREATE TABLE ml.clusters (
    model_id       uuid NOT NULL REFERENCES ml.models(id) ON DELETE CASCADE,
    tenant_id      uuid NOT NULL,
    cluster_id     int NOT NULL,           -- -1 = noise / outliers
    size           int NOT NULL,
    fraud_rate     real,
    labeled_count  int NOT NULL DEFAULT 0,
    centroid       jsonb,
    profile        jsonb NOT NULL DEFAULT '{}',
    top_features   jsonb NOT NULL DEFAULT '[]',
    label          text,                   -- analyst-given name, e.g. "promo farm"
    notes          text,
    PRIMARY KEY (model_id, cluster_id)
);

CREATE TABLE ml.event_anomaly (
    model_id       uuid NOT NULL REFERENCES ml.models(id) ON DELETE CASCADE,
    event_id       uuid NOT NULL,          -- core.events.id (cross-service, no FK)
    tenant_id      uuid NOT NULL,
    anomaly_score  real NOT NULL,
    cluster_id     int,
    pca_x          real,
    pca_y          real,
    PRIMARY KEY (model_id, event_id)
);
CREATE INDEX ix_event_anomaly_score ON ml.event_anomaly (model_id, anomaly_score DESC);

CREATE TABLE ml.graph_communities (
    customer_id    uuid PRIMARY KEY,
    tenant_id      uuid NOT NULL,
    project_id     uuid NOT NULL,
    community_id   int NOT NULL,
    computed_at    timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ix_graph_communities_comm ON ml.graph_communities (project_id, community_id);

CREATE TABLE ml.graph_community_stats (
    tenant_id      uuid NOT NULL,
    project_id     uuid NOT NULL,
    community_id   int NOT NULL,
    size           int NOT NULL,
    fraud_count    int NOT NULL,
    fraud_rate     real NOT NULL,
    computed_at    timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, community_id)
);
