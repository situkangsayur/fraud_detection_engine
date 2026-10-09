#!/usr/bin/env bash
# Resets a DEMO install to its default state and produces that day's research dataset:
#   1. wipe all platform data, start the stack
#   2. seed the history part (first 70 % of a 60-day window, load_only) + its labels
#   3. upload/attach regulations, train + approve one supervised and one unsupervised model per project
#   4. seed the online part (scored with every engine active)
#   5. simulate analyst activity (cases, proposals, reports, …)
#   6. export the research dataset to $DEMO_DATASET_DIR/<date>
#
#   deploy/demo/reset-demo.sh            # run from anywhere; logs to stdout
#
# Destroys every tenant, user, rule, case, model and upload of the `fraud-platform` compose project. Never point it
# at a real installation. Schedule example (every day at 00:00):
#   0 0 * * * /path/to/repo/deploy/demo/reset-demo.sh >> ~/.local/state/fraud-demo-reset.log 2>&1
#
# Everything runs inside main(): bash parses the whole function before executing it, so editing this file while a
# reset is running cannot make the running reset execute shifted bytes.
set -euo pipefail
export PATH="$HOME/.local/bin:$HOME/.cargo/bin:/usr/local/bin:/usr/bin:/bin:$PATH"   # cron has a minimal PATH

log() { echo "$(date '+%F %T') reset: $*"; }

simulate() {   # simulate <phase> <end> <seed>
  docker compose --profile seed run --rm -e SIM_PHASE="$1" -e SIM_END="$2" -e SIM_SEED="$3" simulator
}

fresh_history() {   # fresh_history <end> <seed>
  docker compose down --remove-orphans
  for v in pg_data os_data ml_models regulations uploads; do   # ollama_data (model cache) is kept
    docker volume rm -f "fraud-platform_${v}" >/dev/null
  done
  log "data volumes removed"
  docker compose up -d --wait --wait-timeout 900
  log "stack healthy"
  simulate history "$1" "$2"
}

main() {
  local root state_dir dataset_dir end seed
  root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
  cd "$root"
  state_dir="${DEMO_STATE_DIR:-$HOME/.local/state/fraud-demo}"
  dataset_dir="${DEMO_DATASET_DIR:-$HOME/datasets/fraud-live-demo}"
  mkdir -p "$state_dir" "$dataset_dir"

  exec 9>"${TMPDIR:-/tmp}/fraud-demo-reset.lock"
  flock -n 9 || { echo "another reset is running"; return 0; }

  # Reproducible run: the simulator's ground truth is a pure function of seed + window end. A new seed per day
  # gives independent replications of the same generator (one research dataset per day).
  end="$(date -u +%Y-%m-%dT%H:%M:%S)"
  seed="${SIM_SEED:-$(date +%Y%m%d)}"
  printf 'SIM_END=%s\nSIM_SEED=%s\nSIM_CUSTOMERS=%s\nSIM_DAYS=%s\n' "$end" "$seed" "${SIM_CUSTOMERS:-2000}" \
    "${SIM_DAYS:-60}" > "$state_dir/last-run.env"
  log "start (seed $seed, window end $end)"

  # A half-sent seed cannot be resumed safely, so a failure (usually a Docker Hub/DNS hiccup) repeats the whole
  # wipe + history seed once.
  fresh_history "$end" "$seed" || { log "history seed failed, starting over"; sleep 30; fresh_history "$end" "$seed"; }
  log "history seeded"

  set -a
  # shellcheck disable=SC1091
  . ./.env
  set +a
  python3 deploy/demo/post_seed.py
  log "models and regulations ready"

  # Duplicate external ids are rejected by the platform, so retrying the online phase cannot double events.
  simulate online "$end" "$seed" || { log "online seed failed, retrying"; sleep 30; simulate online "$end" "$seed"; }
  log "online seeded"

  # ground truth of this run (pure function of seed + window end): analysts resolve cases against it, and the
  # research export uses it as the evaluation reference
  local truth="$state_dir/truth.jsonl"
  uv run --directory tools/simulator python -m simulator stats --end "$end" --seed "$seed" \
    --customers "${SIM_CUSTOMERS:-2000}" --days "${SIM_DAYS:-60}" --truth-out "$truth" >/dev/null

  DEMO_TRUTH_FILE="$truth" python3 deploy/demo/activity.py || log "activity simulation had errors (continuing)"
  log "activity simulated"

  uv run --directory tools/research_export python -m research_export --out "$dataset_dir/$(date +%F)" \
    --truth "$truth" || log "research export failed"
  cp "$truth" "$dataset_dir/$(date +%F)/ground_truth.jsonl" 2>/dev/null || true
  log "done"
}

main "$@"
exit
