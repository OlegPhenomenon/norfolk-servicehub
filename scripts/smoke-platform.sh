#!/usr/bin/env bash
# Platform smoke test: seed a temporary demo database, start the server and walk the auth flow with curl.
#
#   scripts/smoke-platform.sh            # uses a temp DATA_DIR and port 18080 (override with SMOKE_PORT)
#
# Requires: cargo, curl, python3.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PORT="${SMOKE_PORT:-18080}"
BASE="http://127.0.0.1:${PORT}"
DATA_DIR="$(mktemp -d -t nsh-smoke-XXXXXX)"
JAR="${DATA_DIR}/cookies.txt"
LOG="${DATA_DIR}/server.log"
SERVER_PID=""

cleanup() {
  if [[ -n "${SERVER_PID}" ]]; then kill "${SERVER_PID}" 2>/dev/null || true; wait "${SERVER_PID}" 2>/dev/null || true; fi
  rm -rf "${DATA_DIR}"
}
trap cleanup EXIT

fail() { echo "FAIL: $*" >&2; [[ -f "${LOG}" ]] && { echo "--- server log (tail) ---" >&2; tail -n 30 "${LOG}" >&2; }; exit 1; }
ok() { echo "ok   $*"; }

# json <expr> — evaluates a Python expression over the JSON document `d` read from stdin.
json() { python3 -c "import json,sys; d=json.load(sys.stdin); v=($1); print(json.dumps(v) if not isinstance(v,str) else v)"; }

# req METHOD PATH [BODY] — sets STATUS and BODY; sends the CSRF token for non-GET requests.
CSRF=""
req() {
  local method="$1" path="$2" body="${3-}"
  [[ -n "${body}" ]] || body='{}'
  local args=(-sS -o "${DATA_DIR}/body" -w '%{http_code}' -b "${JAR}" -c "${JAR}" -X "${method}" "${BASE}${path}")
  if [[ "${method}" != "GET" ]]; then
    args+=(-H "X-CSRF-Token: ${CSRF}" -H 'Content-Type: application/json' --data "${body}")
  fi
  STATUS="$(curl "${args[@]}")"
  BODY="$(cat "${DATA_DIR}/body")"
}

export DEMO_MODE=true COOKIE_SECURE=false DATA_DIR PORT RUST_LOG="${RUST_LOG:-warn}"
export WEB_DIST="${ROOT}/web/dist"
export SEED_DATA_DIR="${ROOT}/server/seed-data"

echo "== build"
(cd "${ROOT}/server" && cargo build --quiet)
BIN="${ROOT}/server/target/debug/servicehub"

echo "== seed-demo into ${DATA_DIR}"
"${BIN}" seed-demo >"${DATA_DIR}/seed.log" 2>&1 || { cat "${DATA_DIR}/seed.log" >&2; fail "seed-demo"; }
ok "seed-demo"

echo "== serve on ${BASE}"
"${BIN}" serve >"${LOG}" 2>&1 &
SERVER_PID=$!
for _ in $(seq 1 100); do
  curl -fsS "${BASE}/api/health" >/dev/null 2>&1 && break
  kill -0 "${SERVER_PID}" 2>/dev/null || fail "server exited during start-up"
  sleep 0.1
done
curl -fsS "${BASE}/api/health" >/dev/null || fail "server did not become healthy"
ok "GET /api/health"

echo "== auth flow"
req GET /api/me
[[ "${STATUS}" == 200 ]] || fail "anonymous /api/me → ${STATUS}"
[[ "$(echo "${BODY}" | json 'd["user"]')" == "null" ]] || fail "anonymous /api/me has a user"
CSRF="$(echo "${BODY}" | json 'd["csrf_token"]')"
ok "GET /api/me (anonymous, csrf token issued)"

req POST /api/demo/login '{"persona":"olga"}'
[[ "${STATUS}" == 200 ]] || fail "demo login olga → ${STATUS} ${BODY}"
CSRF="$(echo "${BODY}" | json 'd["csrf_token"]')"
ok "POST /api/demo/login olga"

req GET /api/me
[[ "$(echo "${BODY}" | json 'd["mfa_required"]')" == "true" ]] || fail "olga should need TOTP: ${BODY}"
ok "GET /api/me → mfa_required: true"

req GET /api/demo/authenticator
CODE="$(echo "${BODY}" | json '[x["code"] for x in d if x["persona"]=="olga"][0]')"
[[ "${CODE}" =~ ^[0-9]{6}$ ]] || fail "no authenticator code for olga: ${BODY}"
ok "GET /api/demo/authenticator → olga code ${CODE}"

req POST /api/auth/totp "{\"code\":\"${CODE}\"}"
[[ "${STATUS}" == 200 ]] || fail "TOTP rejected: ${STATUS} ${BODY}"
[[ "$(echo "${BODY}" | json 'd["mfa_required"]')" == "false" ]] || fail "mfa still required after TOTP"
ok "POST /api/auth/totp → mfa_required: false"

# Replay: a fresh session must not accept the same code again.
req POST /api/demo/login '{"persona":"olga"}'
CSRF="$(echo "${BODY}" | json 'd["csrf_token"]')"
req POST /api/auth/totp "{\"code\":\"${CODE}\"}"
[[ "${STATUS}" == 422 ]] || fail "replayed TOTP code should be rejected, got ${STATUS} ${BODY}"
ok "replayed TOTP code rejected (${STATUS})"

req POST /api/demo/login '{"persona":"alexey"}'
[[ "${STATUS}" == 200 ]] || fail "demo login alexey → ${STATUS}"
[[ "$(echo "${BODY}" | json 'd["mfa_required"]')" == "false" ]] || fail "resident should not need TOTP"
CSRF="$(echo "${BODY}" | json 'd["csrf_token"]')"
req GET /api/notifications
[[ "${STATUS}" == 200 ]] || fail "alexey notifications → ${STATUS}"
ok "resident alexey logged in without TOTP"

SAVED="${CSRF}"; CSRF="not-the-token"
req POST /api/auth/logout '{}'
[[ "${STATUS}" == 403 ]] || fail "POST with a wrong CSRF token should be 403, got ${STATUS}"
CSRF="${SAVED}"
req GET /api/me
[[ "$(echo "${BODY}" | json 'd["user"]["persona_key"]')" == "alexey" ]] || fail "session lost after rejected logout"
ok "CSRF enforced (403 with a wrong token; session intact)"

echo "== routing"
req GET /api/x
[[ "${STATUS}" == 404 && "$(echo "${BODY}" | json 'd["error"]["code"]')" == "not_found" ]] || fail "unknown /api path: ${STATUS} ${BODY}"
ok "GET /api/x → JSON 404"

HEADERS="$(curl -sS -D - -o "${DATA_DIR}/index" "${BASE}/")"
echo "${HEADERS}" | grep -qi '^content-security-policy:' || fail "missing CSP header"
if [[ -f "${WEB_DIST}/index.html" ]]; then
  echo "${HEADERS}" | head -1 | grep -q ' 200' || fail "GET / should serve index.html"
  grep -qi '<div id="root"\|<!doctype html' "${DATA_DIR}/index" || fail "GET / did not return the SPA"
  curl -sS -o /dev/null -w '%{http_code}' "${BASE}/staff/cases/1" | grep -q 200 || fail "SPA fallback for deep link"
  ok "GET / serves web/dist/index.html (SPA fallback works)"
else
  echo "note web/dist/index.html not found — skipped the SPA check (build the web app to include it)"
fi

echo "PASS platform smoke"
