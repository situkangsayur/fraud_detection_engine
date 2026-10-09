-- 0004_graph.sql — graph data set per project (bipartite customer ↔ entity + entity similarity).
-- Owner service: graph-service. customer_id references core.customers logically (cross-service, no FK),
-- risk_label is mirrored here so graph traversal never needs a cross-schema join on the hot path.

CREATE TABLE graph.nodes_customer (
    customer_id   uuid PRIMARY KEY,
    tenant_id     uuid NOT NULL,
    project_id    uuid NOT NULL,
    external_id   text NOT NULL,
    risk_label    text NOT NULL DEFAULT 'unknown' CHECK (risk_label IN ('fraud', 'legit', 'unknown')),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (tenant_id, project_id) REFERENCES core.projects (tenant_id, id) ON DELETE CASCADE
);
CREATE INDEX ix_graph_customers_fraud ON graph.nodes_customer (project_id) WHERE risk_label = 'fraud';

CREATE TABLE graph.entities (
    id                bigserial PRIMARY KEY,
    tenant_id         uuid NOT NULL,
    project_id        uuid NOT NULL,
    kind              text NOT NULL CHECK (kind IN ('email', 'phone', 'device', 'ip', 'card', 'bank_account',
                                                    'address', 'ref_transaction', 'api_client')),
    value_normalized  text NOT NULL,       -- tenant-peppered hashes for card/bank_account; normalised text otherwise
    display_value     text NOT NULL,       -- masked, safe for UI
    customer_count    int NOT NULL DEFAULT 0,
    attributes        jsonb NOT NULL DEFAULT '{}',
    first_seen_at     timestamptz NOT NULL DEFAULT now(),
    last_seen_at      timestamptz NOT NULL DEFAULT now(),
    UNIQUE (project_id, kind, value_normalized),
    FOREIGN KEY (tenant_id, project_id) REFERENCES core.projects (tenant_id, id) ON DELETE CASCADE
);
CREATE INDEX ix_graph_entities_trgm ON graph.entities USING gin (value_normalized gin_trgm_ops)
    WHERE kind IN ('address', 'phone', 'email');

CREATE TABLE graph.entity_links (
    customer_id     uuid NOT NULL REFERENCES graph.nodes_customer(customer_id) ON DELETE CASCADE,
    entity_id       bigint NOT NULL REFERENCES graph.entities(id) ON DELETE CASCADE,
    tenant_id       uuid NOT NULL,
    project_id      uuid NOT NULL,
    event_count     int NOT NULL DEFAULT 1,
    first_event_id  uuid,
    first_seen_at   timestamptz NOT NULL DEFAULT now(),
    last_seen_at    timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (customer_id, entity_id)
);
CREATE INDEX ix_graph_links_entity ON graph.entity_links (entity_id);

CREATE TABLE graph.entity_similarity (
    entity_a    bigint NOT NULL REFERENCES graph.entities(id) ON DELETE CASCADE,
    entity_b    bigint NOT NULL REFERENCES graph.entities(id) ON DELETE CASCADE,
    tenant_id   uuid NOT NULL,
    project_id  uuid NOT NULL,
    score       real NOT NULL,
    method      text NOT NULL,                 -- trigram | phone_suffix | phone_edit1 | email_local
    created_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (entity_a, entity_b),
    CHECK (entity_a < entity_b)
);
CREATE INDEX ix_graph_similarity_b ON graph.entity_similarity (entity_b);
