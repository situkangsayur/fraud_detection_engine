#!/usr/bin/env bash
# Runs core-api integration tests against a throwaway Postgres 16 with the real init script + migrations.
#   services/rust/crates/core-api/tests/run-integration.sh [extra cargo test args]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../../../../.." && pwd)"
NAME="core-api-it-$$"
cleanup() { docker rm -f "$NAME" >/dev/null 2>&1 || true; }
trap cleanup EXIT

docker run -d --name "$NAME" -p "127.0.0.1:${IT_PG_PORT:-}:5432" \
  -e POSTGRES_DB=fraud -e POSTGRES_PASSWORD=pw \
  -e MIGRATOR_DB_PASSWORD=m -e CORE_API_DB_PASSWORD=c -e RULE_SERVICE_DB_PASSWORD=r \
  -e GRAPH_SERVICE_DB_PASSWORD=g -e ML_SERVICE_DB_PASSWORD=ml -e LLM_SERVICE_DB_PASSWORD=l \
  -e INGEST_SERVICE_DB_PASSWORD=i \
  -v "$ROOT/deploy/postgres/init:/docker-entrypoint-initdb.d:ro" \
  postgres:16.4-alpine >/dev/null

# IT_PG_PORT pins the host port; by default Docker picks a free one.
PORT="$(docker port "$NAME" 5432/tcp | head -1 | awk -F: '{print $NF}')"
for _ in $(seq 1 60); do
  if docker exec -e PGPASSWORD=m "$NAME" psql -h 127.0.0.1 -U migrator -d fraud -tAc 'select 1' >/dev/null 2>&1; then
    break
  fi
  sleep 1
done

export CORE_API_TEST_MIGRATOR_URL="postgres://migrator:m@127.0.0.1:${PORT}/fraud"
export CORE_API_TEST_APP_URL="postgres://core_api:c@127.0.0.1:${PORT}/fraud"
export CORE_API_TEST_MIGRATIONS_DIR="$ROOT/db/migrations"
cd "$ROOT/services/rust"
cargo test -p core-api --test integration "$@"
