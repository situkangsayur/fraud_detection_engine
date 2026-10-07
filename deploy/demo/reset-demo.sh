#!/usr/bin/env bash
# Resets a DEMO install to its default state: wipes all platform data, starts the stack, seeds fresh synthetic data
# that ends "now" (so dashboards always show recent activity), then attaches regulations and activates ML models.
#
#   deploy/demo/reset-demo.sh            # run from anywhere; logs to stdout
#
# Destroys every tenant, user, rule, case, model and upload. Never point this at a real installation.
# Schedule example (every day at 00:00):
#   0 0 * * * /path/to/repo/deploy/demo/reset-demo.sh >> ~/.local/state/fraud-demo-reset.log 2>&1
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
DATA_VOLUMES=(pg_data os_data ml_models regulations uploads)   # ollama_data (model cache) is kept

exec 9>"${TMPDIR:-/tmp}/fraud-demo-reset.lock"
flock -n 9 || { echo "another reset is running"; exit 0; }

ts() { date '+%F %T'; }
echo "$(ts) reset: start"

fresh_seed() {
  docker compose down --remove-orphans
  for v in "${DATA_VOLUMES[@]}"; do docker volume rm -f "fraud-platform_${v}" >/dev/null; done
  echo "$(ts) reset: data volumes removed"
  docker compose up -d --wait --wait-timeout 900
  echo "$(ts) reset: stack healthy"
  docker compose --profile seed run --rm simulator
}

# A half-sent seed cannot be resumed safely (the simulator skips labelled projects), so a failure — usually a
# Docker Hub/DNS hiccup — repeats the whole wipe + seed once.
fresh_seed || { echo "$(ts) reset: seed failed, starting over"; sleep 30; fresh_seed; }
echo "$(ts) reset: seed done"

set -a
# shellcheck disable=SC1091
. ./.env
set +a
python3 deploy/demo/post_seed.py
echo "$(ts) reset: done"
