#!/usr/bin/env bash
# Seeding entry point for demo and research installs (compose project `fraud-platform`).
#
#   deploy/seed.sh demo                     # wipe everything, seed tenant `demo` (synthetic simulator, 4 projects)
#   deploy/seed.sh research <dataset>…      # add/refresh experiment tenants from public datasets (no wipe):
#                                           #   sparkov → exp-sparkov, paysim → exp-paysim, saml-d → exp-saml-d
#   deploy/seed.sh master                   # demo + all research tenants, then snapshot as the new master
#   deploy/seed.sh restore                  # put the master back (cron 00:00) — see deploy/demo/master.sh
#
# Both modes use the same protocol: history (70 %, load_only) → train + approve models → online (30 %, scored) →
# analyst activity → research export (~/datasets/fraud-live-demo for demo, ~/datasets/fraud-research for research).
# Accounts of every seeded tenant: sim-admin@<tenant>.local, analyst@<tenant>.local, approver@<tenant>.local, all with
# DEMO_USER_PASSWORD (the demo tenant's tenant admin uses ADMIN_PASSWORD).
set -euo pipefail
export PATH="$HOME/.local/bin:/usr/local/bin:/usr/bin:/bin:$PATH"

DATASETS=(sparkov paysim saml-d)

main() {
  local root mode
  root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
  cd "$root"
  mode="${1:-}"
  shift || true
  set -a
  # shellcheck disable=SC1091
  . ./.env
  set +a
  case "$mode" in
    demo) deploy/demo/reset-demo.sh ;;
    research)
      [ $# -gt 0 ] || set -- "${DATASETS[@]}"
      for d in "$@"; do
        uv run --directory tools/research_seed python -m research_seed --dataset "$d"
      done
      ;;
    master)
      deploy/demo/reset-demo.sh
      for d in "${DATASETS[@]}"; do
        uv run --directory tools/research_seed python -m research_seed --dataset "$d"
      done
      deploy/demo/master.sh snapshot
      ;;
    restore) deploy/demo/master.sh restore ;;
    *) sed -n '2,15p' "$0"; return 2 ;;
  esac
}

main "$@"
exit
