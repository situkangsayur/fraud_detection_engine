#!/bin/sh
# Creates the schema-owner role (migrator) and least-privilege service roles on first DB initialisation.
# Migrations (db/migrations, run by the `migrate` job as `migrator`) create schemas and apply grants + RLS.
set -eu

psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" \
  -v migrator_pw="$MIGRATOR_DB_PASSWORD" \
  -v core_pw="$CORE_API_DB_PASSWORD" \
  -v rule_pw="$RULE_SERVICE_DB_PASSWORD" \
  -v graph_pw="$GRAPH_SERVICE_DB_PASSWORD" \
  -v ml_pw="$ML_SERVICE_DB_PASSWORD" \
  -v llm_pw="$LLM_SERVICE_DB_PASSWORD" \
  -v ingest_pw="$INGEST_SERVICE_DB_PASSWORD" \
  -v db="$POSTGRES_DB" <<'SQL'
CREATE ROLE migrator       LOGIN PASSWORD :'migrator_pw';
CREATE ROLE core_api       LOGIN PASSWORD :'core_pw';
CREATE ROLE rule_service   LOGIN PASSWORD :'rule_pw';
CREATE ROLE graph_service  LOGIN PASSWORD :'graph_pw';
CREATE ROLE ml_service     LOGIN PASSWORD :'ml_pw';
CREATE ROLE llm_service    LOGIN PASSWORD :'llm_pw';
CREATE ROLE ingest_service LOGIN PASSWORD :'ingest_pw';

-- migrator owns the database and public schema → can create extensions (trusted) and schemas.
ALTER DATABASE :"db" OWNER TO migrator;
ALTER SCHEMA public OWNER TO migrator;
REVOKE ALL ON DATABASE :"db" FROM PUBLIC;
REVOKE CREATE ON SCHEMA public FROM PUBLIC;
GRANT CONNECT ON DATABASE :"db" TO core_api, rule_service, graph_service, ml_service, llm_service, ingest_service;

-- Each service resolves unqualified names in its own schema first (code still schema-qualifies everything).
ALTER ROLE core_api       SET search_path = core, public;
ALTER ROLE rule_service   SET search_path = rules, core, public;
ALTER ROLE graph_service  SET search_path = graph, core, public;
ALTER ROLE ml_service     SET search_path = ml, core, public;
ALTER ROLE llm_service    SET search_path = llm, core, public;
ALTER ROLE ingest_service SET search_path = ingest, core, public;
SQL
