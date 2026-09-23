#!/usr/bin/env bash
# Runs rule-service integration tests against a throwaway PostgreSQL 16 container:
#   services/rust/crates/rule-service/tests/run-integration.sh [test-binary args, e.g. --nocapture]
# The container gets the production role init script (deploy/postgres/init) and is always removed afterwards.
# Migrations are applied by the tests themselves with sqlx as the `migrator` role (same as the `migrate` job).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../../../../.." && pwd)"
NAME="rule-service-it-$$"
cleanup() { docker rm -f "$NAME" >/dev/null 2>&1 || true; }
trap cleanup EXIT

docker run -d --name "$NAME" \
  -e POSTGRES_DB=fraud -e POSTGRES_PASSWORD=pw \
  -e MIGRATOR_DB_PASSWORD=m -e CORE_API_DB_PASSWORD=c -e RULE_SERVICE_DB_PASSWORD=r \
  -e GRAPH_SERVICE_DB_PASSWORD=g -e ML_SERVICE_DB_PASSWORD=ml -e LLM_SERVICE_DB_PASSWORD=l \
  -e INGEST_SERVICE_DB_PASSWORD=i \
  -v "$ROOT/deploy/postgres/init:/docker-entrypoint-initdb.d:ro" \
  -p 127.0.0.1::5432 postgres:16.4-alpine >/dev/null

PORT="$(docker port "$NAME" 5432/tcp | head -1 | awk -F: '{print $NF}')"
for _ in $(seq 1 60); do
  if docker exec -e PGPASSWORD=m "$NAME" psql -h 127.0.0.1 -U migrator -d fraud -tAc 'select 1' >/dev/null 2>&1; then
    break
  fi
  sleep 1
done

export RULE_IT_MIGRATOR_URL="postgres://migrator:m@127.0.0.1:${PORT}/fraud"
export RULE_IT_SERVICE_URL="postgres://rule_service:r@127.0.0.1:${PORT}/fraud"
export RULE_IT_MIGRATIONS_DIR="$ROOT/db/migrations"
cd "$ROOT/services/rust"
cargo test -p rule-service --test integration -- --test-threads=4 "$@"
