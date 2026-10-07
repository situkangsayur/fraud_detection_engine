#!/usr/bin/env bash
# Usage: db/tests/test_migrations.sh — spins up a throwaway Postgres, runs init roles + all migrations as migrator, then RLS smoke tests.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
docker rm -f fraud-mig-test >/dev/null 2>&1 || true
docker run -d --name fraud-mig-test -e POSTGRES_DB=fraud -e POSTGRES_PASSWORD=pw \
  -e MIGRATOR_DB_PASSWORD=m -e CORE_API_DB_PASSWORD=c -e RULE_SERVICE_DB_PASSWORD=r -e GRAPH_SERVICE_DB_PASSWORD=g \
  -e ML_SERVICE_DB_PASSWORD=ml -e LLM_SERVICE_DB_PASSWORD=l -e INGEST_SERVICE_DB_PASSWORD=i \
  -v $ROOT/deploy/postgres/init:/docker-entrypoint-initdb.d:ro -v $ROOT/db/migrations:/migrations:ro \
  postgres:16.4-alpine >/dev/null
for i in $(seq 1 60); do
  if docker exec -e PGPASSWORD=m fraud-mig-test psql -h 127.0.0.1 -U migrator -d fraud -tAc 'select 1' >/dev/null 2>&1; then break; fi
  sleep 1
done
# Several smoke tests below are expected to fail inside psql, so the exit code only reflects the migrations.
status=0
for f in "$ROOT"/db/migrations/*.sql; do
  n=$(basename "$f"); echo "== $n"
  docker exec -e PGPASSWORD=m fraud-mig-test psql -h 127.0.0.1 -U migrator -d fraud -v ON_ERROR_STOP=1 -q -f /migrations/$n || status=1
done
echo "== RLS smoke test"
docker exec -i -e PGPASSWORD=m fraud-mig-test psql -h 127.0.0.1 -U migrator -d fraud -v ON_ERROR_STOP=1 -q <<'SQL'
INSERT INTO core.tenants (id, slug, name) VALUES ('11111111-1111-1111-1111-111111111111','t1','T1'),('22222222-2222-2222-2222-222222222222','t2','T2');
INSERT INTO core.projects (id, tenant_id, slug, name, stage) VALUES
 ('aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa','11111111-1111-1111-1111-111111111111','checkout','Checkout','pre_payment'),
 ('bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb','22222222-2222-2222-2222-222222222222','checkout','Checkout','pre_payment');
SQL
docker exec -i -e PGPASSWORD=c fraud-mig-test psql -h 127.0.0.1 -U core_api -d fraud -v ON_ERROR_STOP=1 -tA <<'SQL'
SELECT 'no tenant set -> projects visible: ' || count(*) FROM core.projects;
BEGIN; SELECT set_config('app.tenant_id','11111111-1111-1111-1111-111111111111', true);
SELECT 'tenant1 -> projects visible: ' || count(*) FROM core.projects;
COMMIT;
SQL
echo "== cross-tenant insert must fail:"
docker exec -e PGPASSWORD=c fraud-mig-test psql -h 127.0.0.1 -U core_api -d fraud -tA -c "BEGIN; SELECT set_config('app.tenant_id','11111111-1111-1111-1111-111111111111', true); INSERT INTO core.customers (tenant_id, project_id, external_id) VALUES ('22222222-2222-2222-2222-222222222222','bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb','c1'); COMMIT;" 2>&1 | tail -1
echo "== composite FK (tenant1 row pointing at tenant2 project) must fail:"
docker exec -e PGPASSWORD=m fraud-mig-test psql -h 127.0.0.1 -U migrator -d fraud -tA -c "INSERT INTO core.customers (tenant_id, project_id, external_id) VALUES ('11111111-1111-1111-1111-111111111111','bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb','c1');" 2>&1 | tail -1
echo "== audit append-only:"
docker exec -e PGPASSWORD=c fraud-mig-test psql -h 127.0.0.1 -U core_api -d fraud -tA -c "INSERT INTO core.audit_log (actor_type, action) VALUES ('system','test'); UPDATE core.audit_log SET action='x';" 2>&1 | tail -1
echo "== rule_service velocity read of core.events allowed:"
docker exec -e PGPASSWORD=r fraud-mig-test psql -h 127.0.0.1 -U rule_service -d fraud -tA -c "SELECT count(*) FROM core.events" 2>&1 | tail -1
echo "== rule_service write to core.events denied:"
docker exec -e PGPASSWORD=r fraud-mig-test psql -h 127.0.0.1 -U rule_service -d fraud -tA -c "DELETE FROM core.events" 2>&1 | tail -1
exit $status
