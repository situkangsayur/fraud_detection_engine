#!/usr/bin/env bash
# Smoke test of the BUILT app in mock mode (no backend needed):
#   npm run build && scripts/smoke-mock.sh
# Starts .output/server with MOCK_API=1, signs in through the BFF, then checks SSR pages, the API proxy and the
# BFF security behaviour (401 without session, CSRF header required, no token leakage, traversal blocked).
set -uo pipefail
PORT="${PORT:-3917}"
BASE="http://127.0.0.1:${PORT}"
TMP="$(mktemp -d)"
JAR="$TMP/cookies.txt"
fail=0
check() { # name expected actual
  if [[ "$2" == "$3" ]]; then printf '  ok    %-60s %s\n' "$1" "$3"; else printf '  FAIL  %-60s expected %s got %s\n' "$1" "$2" "$3"; fail=1; fi
}

MOCK_API=1 PORT="$PORT" NUXT_SESSION_SECRET="smoke-test-secret-0123456789abcdef0123" node .output/server/index.mjs >"$TMP/server.log" 2>&1 &
SERVER=$!
trap 'kill $SERVER 2>/dev/null; rm -rf "$TMP"' EXIT
for _ in $(seq 1 60); do curl -fs "$BASE/healthz" >/dev/null && break; sleep 0.25; done

echo "== BFF"
check "GET /healthz" 200 "$(curl -s -o /dev/null -w '%{http_code}' "$BASE/healthz")"
check "unauthenticated page redirects to /login" 302 "$(curl -s -o /dev/null -w '%{http_code}' "$BASE/projects")"
check "unauthenticated API → 401" 401 "$(curl -s -o /dev/null -w '%{http_code}' "$BASE/api/projects")"
check "login wrong password → 401" 401 "$(curl -s -o /dev/null -w '%{http_code}' -X POST -H 'content-type: application/json' -d '{"email":"owner@demo.local","password":"wrong"}' "$BASE/api/auth/login")"
check "login → 200" 200 "$(curl -s -o /dev/null -w '%{http_code}' -X POST -H 'content-type: application/json' -H 'x-requested-with: fraud-web' -d '{"email":"owner@demo.local","password":"demo"}' -c "$JAR" "$BASE/api/auth/login")"
check "session cookie is HttpOnly" 1 "$(grep -c '#HttpOnly_' "$JAR")"
SESSION_JSON="$(curl -s -b "$JAR" "$BASE/api/_auth/session")"
check "session endpoint does not leak tokens" 0 "$(grep -c -E 'access|refresh|Bearer' <<<"$SESSION_JSON")"
check "mutating API without CSRF header → 403" 403 "$(curl -s -o /dev/null -w '%{http_code}' -b "$JAR" -X POST -H 'content-type: application/json' -d '{}' "$BASE/api/projects")"
check "cross-origin mutating request → 403" 403 "$(curl -s -o /dev/null -w '%{http_code}' -b "$JAR" -X POST -H 'x-requested-with: fraud-web' -H 'origin: https://evil.example' -H 'content-type: application/json' -d '{}' "$BASE/api/projects")"
check "path traversal blocked → 404" 404 "$(curl -s --path-as-is -o /dev/null -w '%{http_code}' -b "$JAR" "$BASE/api/projects/%2e%2e/tenants")"
check "reserved /api/v1 prefix not proxied → 404" 404 "$(curl -s --path-as-is -o /dev/null -w '%{http_code}' -b "$JAR" "$BASE/api/v1/projects")"
PID="$(curl -s -b "$JAR" "$BASE/api/projects" | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>console.log(JSON.parse(s).items[0].id))')"
check "proxied list has pagination envelope" 1 "$(curl -s -b "$JAR" "$BASE/api/projects/$PID/rules" | grep -c '"page_size"')"
check "proxied 422 keeps problem+json" "422 application/problem+json" "$(curl -s -o /dev/null -w '%{http_code} %{content_type}' -b "$JAR" -X POST -H 'x-requested-with: fraud-web' -H 'content-type: application/json' -d '{"code":"bad code","name":"x","kind":"simple","typologies":[],"event_types":[],"risk_score":1,"definition":{"kind":"simple","when":{"all":[]}}}' "$BASE/api/projects/$PID/rules")"
check "SSE chat streams text/event-stream" 1 "$(curl -s -N -b "$JAR" -X POST -H 'x-requested-with: fraud-web' -H 'accept: text/event-stream' -H 'content-type: application/json' -d '{"message":"hai"}' "$BASE/api/projects/$PID/llm/chat/stream" | grep -c 'event: done')"

echo "== SSR pages (200 and no error page)"
for p in / /projects /projects/new "/p/$PID" "/p/$PID/events" "/p/$PID/cases" "/p/$PID/rules" "/p/$PID/rules/new?kind=velocity" "/p/$PID/rules/new?kind=composite" \
  "/p/$PID/rulesets" "/p/$PID/reference-lists" "/p/$PID/formulas" "/p/$PID/proposals" "/p/$PID/ml/supervised" "/p/$PID/ml/unsupervised" \
  "/p/$PID/ml/algorithms" "/p/$PID/graph" "/p/$PID/llm/chat" "/p/$PID/llm/reports" "/p/$PID/llm/regulations" "/p/$PID/data-sources" \
  "/p/$PID/field-catalog" "/p/$PID/settings" "/p/$PID/members" "/p/$PID/audit" /tenant/users /tenant/regulations /tenant/reference-lists; do
  code="$(curl -s -o "$TMP/page.html" -w '%{http_code}' -b "$JAR" "$BASE$p")"
  [[ "$code" == 302 ]] && code="$(curl -s -L -o "$TMP/page.html" -w '%{http_code}' -b "$JAR" "$BASE$p")"
  errors="$(grep -c -E 'statusCode":(4|5)[0-9][0-9]|Terjadi kesalahan|Halaman tidak ditemukan' "$TMP/page.html")"
  check "GET $p" "200 0" "$code $errors"
done

# detail pages discovered from list endpoints
EV="$(curl -s -b "$JAR" "$BASE/api/projects/$PID/events?page_size=1" | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>console.log(JSON.parse(s).items[0].id))')"
CASE="$(curl -s -b "$JAR" "$BASE/api/projects/$PID/cases?status=" | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>console.log(JSON.parse(s).items[0].id))')"
RULE="$(curl -s -b "$JAR" "$BASE/api/projects/$PID/rules" | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>console.log(JSON.parse(s).items[0].id))')"
for p in "/p/$PID/events/$EV" "/p/$PID/cases/$CASE" "/p/$PID/rules/$RULE"; do
  check "GET $p" 200 "$(curl -s -o /dev/null -w '%{http_code}' -b "$JAR" "$BASE$p")"
done

check "logout → 200" 200 "$(curl -s -o /dev/null -w '%{http_code}' -b "$JAR" -c "$JAR" -X POST -H 'x-requested-with: fraud-web' "$BASE/api/auth/logout")"
check "API after logout → 401" 401 "$(curl -s -o /dev/null -w '%{http_code}' -b "$JAR" "$BASE/api/projects")"

if grep -qiE 'error|unhandled' "$TMP/server.log"; then echo "== server log errors:"; grep -iE 'error|unhandled' "$TMP/server.log" | head -20; fi
[[ $fail == 0 ]] && echo "SMOKE PASSED" || { echo "SMOKE FAILED"; exit 1; }
