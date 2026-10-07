#!/usr/bin/env bash
# Deletes ONE tenant and all of its data from a demo/experiment install (compose project `fraud-platform`), so an
# experiment can be seeded again from scratch without touching the other tenants.
#
#   deploy/demo/wipe-tenant.sh exp-sparkov
#
# Postgres: rows reachable from core.tenants are removed by ON DELETE CASCADE; the few tables without a foreign key to
# the tenant/project are cleaned explicitly first. core.audit_log is append-only and keeps its history.
# OpenSearch: the tenant's regulation index (reg-chunks-<tenant_id>) is dropped. ML artefact files of the tenant's
# models are left in the ml_models volume (unreferenced, removed by the next master restore).
set -euo pipefail

main() {
  local slug="${1:?usage: $0 <tenant-slug>}" root tid
  [ "$slug" != "demo" ] || [ "${ALLOW_DEMO:-}" = 1 ] || { echo "refusing to wipe 'demo' (set ALLOW_DEMO=1)"; return 2; }
  root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
  cd "$root"
  tid="$(psql_super "SELECT id FROM core.tenants WHERE slug = '${slug//\'/}'")"
  [ -n "$tid" ] || { echo "tenant '$slug' not found"; return 0; }
  psql_super "
    BEGIN;
    DELETE FROM rules.ruleset_rules     WHERE tenant_id = '$tid';  -- RESTRICT towards rules.rules
    DELETE FROM ml.graph_communities    WHERE tenant_id = '$tid';
    DELETE FROM ml.graph_community_stats WHERE tenant_id = '$tid';
    DELETE FROM ingest.uploads          WHERE tenant_id = '$tid';
    DELETE FROM core.tenants            WHERE id = '$tid';
    COMMIT;" >/dev/null
  docker compose exec -T opensearch curl -s -o /dev/null -X DELETE "localhost:9200/reg-chunks-$tid" || true
  echo "tenant $slug ($tid) wiped"
}

psql_super() {
  docker compose exec -T postgres sh -c 'psql -v ON_ERROR_STOP=1 -U "$POSTGRES_USER" -d "${POSTGRES_DB:-fraud}" -tA' <<<"$1"
}

main "$@"
exit
