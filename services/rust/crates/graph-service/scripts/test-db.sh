#!/usr/bin/env bash
# Starts a throwaway Postgres 16 with the platform roles + all db/migrations applied, for
# graph-service integration tests. Prints the env vars to export; `--stop` removes the container.
#
#   eval "$(crates/graph-service/scripts/test-db.sh)"   # from services/rust
#   cargo test -p graph-service
#   crates/graph-service/scripts/test-db.sh --stop
set -euo pipefail
NAME=${GRAPH_TEST_PG_NAME:-graph-svc-test-pg}
ROOT="$(cd "$(dirname "$0")/../../../../.." && pwd)"

if [[ "${1:-}" == "--stop" ]]; then
  docker rm -f "$NAME" >/dev/null 2>&1 || true
  exit 0
fi

docker rm -f "$NAME" >/dev/null 2>&1 || true
docker run -d --name "$NAME" -p 127.0.0.1::5432 \
  -e POSTGRES_DB=fraud -e POSTGRES_PASSWORD=pw \
  -e MIGRATOR_DB_PASSWORD=m -e CORE_API_DB_PASSWORD=c -e RULE_SERVICE_DB_PASSWORD=r \
  -e GRAPH_SERVICE_DB_PASSWORD=g -e ML_SERVICE_DB_PASSWORD=ml -e LLM_SERVICE_DB_PASSWORD=l \
  -e INGEST_SERVICE_DB_PASSWORD=i \
  -v "$ROOT/deploy/postgres/init:/docker-entrypoint-initdb.d:ro" \
  -v "$ROOT/db/migrations:/migrations:ro" \
  postgres:16.4-alpine >/dev/null

for _ in $(seq 1 60); do
  if docker exec -e PGPASSWORD=m "$NAME" psql -h 127.0.0.1 -U migrator -d fraud -tAc 'select 1' >/dev/null 2>&1; then
    break
  fi
  sleep 1
done
for f in "$ROOT"/db/migrations/*.sql; do
  docker exec -e PGPASSWORD=m "$NAME" psql -h 127.0.0.1 -U migrator -d fraud -v ON_ERROR_STOP=1 -q \
    -f "/migrations/$(basename "$f")" >/dev/null
done
PORT=$(docker port "$NAME" 5432/tcp | head -1 | awk -F: '{print $NF}')
echo "export GRAPH_TEST_MIGRATOR_URL=postgres://migrator:m@127.0.0.1:${PORT}/fraud"
echo "export GRAPH_TEST_SERVICE_URL=postgres://graph_service:g@127.0.0.1:${PORT}/fraud"
