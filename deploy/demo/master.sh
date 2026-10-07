#!/usr/bin/env bash
# Master snapshot of a DEMO / experiment install (compose project `fraud-platform`).
#
#   deploy/demo/master.sh snapshot           # stop the stack, archive every data volume as the new master, restart
#   deploy/demo/master.sh restore            # put the current master back (what cron runs at 00:00), then shift
#                                            # every timestamp so the data ends "now" again
#   deploy/demo/master.sh info               # show the current master
#
# The master holds all tenants (demo + experiments). Whatever visitors change during the day (data, rules, models,
# cases, uploads) disappears at the next restore. Masters live in $DEMO_MASTER_DIR (default ~/fraud-demo-master),
# one folder per snapshot; `current` points at the one restore uses.
#
# Everything runs inside main(): editing this file during a run cannot disturb it.
set -euo pipefail
export PATH="$HOME/.local/bin:/usr/local/bin:/usr/bin:/bin:$PATH"

VOLUMES=(pg_data os_data ml_models regulations uploads)
TOOL_IMAGE="postgres:16.4-alpine"   # already pulled for the stack; provides tar/gzip

log() { echo "$(date '+%F %T') master: $*"; }

volume_tar() {   # volume_tar <volume> <dir> <create|extract>
  local vol="fraud-platform_$1" dir="$2"
  if [ "$3" = create ]; then
    docker run --rm -v "$vol:/v:ro" -v "$dir:/b" "$TOOL_IMAGE" tar -czf "/b/$1.tar.gz" -C /v .
  else
    docker volume create "$vol" >/dev/null
    docker run --rm -v "$vol:/v" -v "$dir:/b:ro" "$TOOL_IMAGE" \
      sh -c "find /v -mindepth 1 -delete && tar -xzf /b/$1.tar.gz -C /v"
  fi
}

psql_super() {   # psql_super <sql…> — runs as the Postgres superuser inside the postgres container
  docker compose exec -T postgres sh -c 'psql -v ON_ERROR_STOP=1 -U "$POSTGRES_USER" -d "${POSTGRES_DB:-fraud}" -tA' <<<"$1"
}

snapshot() {
  local root="$1" base="$2" dir
  dir="$base/$(date -u +%Y%m%dT%H%M%SZ)"
  mkdir -p "$dir"
  # remember the logical time of the snapshot inside the database itself, so restore can shift relative to it
  psql_super "CREATE SCHEMA IF NOT EXISTS demo_master;
              CREATE TABLE IF NOT EXISTS demo_master.meta (key text PRIMARY KEY, value text NOT NULL);
              INSERT INTO demo_master.meta VALUES ('snapshot_at', now()::text)
                ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value;" >/dev/null
  psql_super "SELECT t.slug || ' | ' || t.name || ' | ' || count(DISTINCT p.id) || ' projects | ' || count(e.id) || ' events'
              FROM core.tenants t LEFT JOIN core.projects p ON p.tenant_id = t.id
              LEFT JOIN core.events e ON e.project_id = p.id GROUP BY t.slug, t.name ORDER BY 1;" > "$dir/tenants.txt"
  git -C "$root" rev-parse HEAD > "$dir/git-commit.txt"
  docker compose stop
  for v in "${VOLUMES[@]}"; do log "archiving $v"; volume_tar "$v" "$dir" create; done
  docker compose up -d --wait --wait-timeout 900
  ln -sfn "$dir" "$base/current"
  log "snapshot $(basename "$dir") ready ($(du -sh "$dir" | cut -f1))"
}

restore() {
  local root="$1" base="$2" dir
  dir="$(readlink -f "$base/current" || true)"
  [ -d "$dir" ] || { log "no master at $base/current"; return 1; }
  log "restoring $(basename "$dir")"
  docker compose stop
  for v in "${VOLUMES[@]}"; do volume_tar "$v" "$dir" extract; done
  docker compose up -d --wait --wait-timeout 900
  # move every timestamp forward by the time since the snapshot (see shift-time.sql)
  psql_super "$(cat "$root/deploy/demo/shift-time.sql")" >/dev/null
  log "restored and time-shifted"
}

main() {
  local root base
  root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
  cd "$root"
  base="${DEMO_MASTER_DIR:-$HOME/fraud-demo-master}"
  mkdir -p "$base"
  exec 9>"${TMPDIR:-/tmp}/fraud-demo-reset.lock"
  flock -n 9 || { echo "another reset/snapshot is running"; return 0; }
  case "${1:-}" in
    snapshot) snapshot "$root" "$base" ;;
    restore) restore "$root" "$base" ;;
    info) ls -la "$base"; cat "$base/current/tenants.txt" 2>/dev/null || true ;;
    *) echo "usage: $0 snapshot|restore|info" >&2; return 2 ;;
  esac
}

main "$@"
exit
