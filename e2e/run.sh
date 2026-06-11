#!/usr/bin/env bash
#
# e2e runner — the in-memory guardian of the language surface.
#
# Given a marreta binary, this:
#   1. lints the project,
#   2. runs the scenario tests (`marreta test`) — the deep, per-endpoint
#      assertions live there, one concern per file under tests/,
#   3. serves the project and smoke-tests the live HTTP path, focusing on what the
#      scenario runner cannot exercise (real api_key hashing, real query/header/raw
#      binding, real http_client self-calls, the rescue recovery path, and the
#      generated docs).
#
# Dependencies: a POSIX shell and curl only (no jq/python). Exits non-zero on the
# first failure.

set -euo pipefail

MARRETA="${1:?usage: run.sh <path-to-marreta-binary>}"
chmod +x "$MARRETA"
command -v xattr >/dev/null 2>&1 && xattr -dr com.apple.quarantine "$MARRETA" 2>/dev/null || true
MARRETA="$(cd "$(dirname "$MARRETA")" && pwd)/$(basename "$MARRETA")"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

BASE="http://127.0.0.1:8080"
PASS=0
SERVE_LOG="$(mktemp)"

fail() {
  echo "FAIL: $*"
  echo "--- serve log ---"
  cat "$SERVE_LOG" 2>/dev/null || true
  exit 1
}

expect_status() {
  local method="$1" path="$2" want="$3"
  shift 3
  local code
  code="$(curl -s -o /dev/null -w '%{http_code}' -X "$method" "$@" "${BASE}${path}")"
  [ "$code" = "$want" ] || fail "${method} ${path} status: want ${want}, got ${code}"
  PASS=$((PASS + 1))
}

expect_body() {
  local method="$1" path="$2" want="$3"
  shift 3
  local body
  body="$(curl -s -X "$method" "$@" "${BASE}${path}")"
  [ "$body" = "$want" ] || fail "${method} ${path} body:
  want: ${want}
  got:  ${body}"
  PASS=$((PASS + 1))
}

expect_match() {
  local method="$1" path="$2" re="$3"
  shift 3
  local body
  body="$(curl -s -X "$method" "$@" "${BASE}${path}")"
  # here-string, not `echo | grep`: some shells' echo interprets backslashes and
  # would truncate a JSON body before the match.
  grep -Eq "$re" <<<"$body" || fail "${method} ${path} did not match /${re}/: ${body}"
  PASS=$((PASS + 1))
}

JSON='-H Content-Type:application/json'

echo "== marreta --version =="; "$MARRETA" --version
echo "== marreta lint =="; "$MARRETA" lint
echo "== marreta test (deep per-endpoint scenarios) =="; "$MARRETA" test

echo "== serve =="
# Serve log goes to a temp file (cleaned up on exit), so the project workspace
# is never written to.
"$MARRETA" serve > "$SERVE_LOG" 2>&1 &
SERVER_PID=$!
trap 'kill "$SERVER_PID" 2>/dev/null || true; wait "$SERVER_PID" 2>/dev/null || true; rm -f "$SERVE_LOG"' EXIT
ready=
for _ in $(seq 1 60); do
  if curl -fsS "${BASE}/responses/text" >/dev/null 2>&1; then ready=1; break; fi
  sleep 0.5
done
[ -n "$ready" ] || fail "server did not become ready"

echo "== live HTTP smoke =="

# the HTTP path serves and routes (json / text / html / fail / a POST pipeline)
expect_body   GET  "/control/match/200" '{"code":200,"label":"OK"}'
expect_body   POST "/transforms/labels" '{"labeled":["low","medium","high"]}' $JSON -d '{"scores":[10,60,110,0]}'
expect_body   GET  "/responses/text"    'pong'
expect_status GET  "/responses/html"    200
expect_status GET  "/responses/fail"    418

# live-only: real query / header / raw binding (scenarios fall back to defaults)
expect_body GET  "/binding/query?term=hi&limit=5" '{"term":"hi","limit":"5"}'
expect_body GET  "/binding/headers"  '{"accept":"application/json"}' -H 'Accept: application/json'
expect_body POST "/binding/raw"      '{"length":9,"body":"hello-raw"}' --data-binary 'hello-raw'

# live-only: real http_client self-call across all five verbs over loopback
expect_body GET "/httpclient/verbs" '{"get":"GET","post":"POST","put":"PUT","patch":"PATCH","delete":"DELETE","get_status":200}'

# live-only: real api_key hashing (scenarios mock the provider)
expect_status GET "/auth/apikey" 401
expect_body   GET "/auth/apikey" '{"ok":true,"principal":"e2e"}' -H 'x-api-key: e2e-secret'

# live-only: rescue recovery from a malformed body (scenarios always send valid JSON)
expect_body POST "/errors/rescue" '{"recovered":true,"code":"runtime_error"}' --data-binary 'not json'

# live: file-namespace resolution (Spec 061) against the served process — a
# cross-file exported task (text.shout), one calling a private same-file helper
# (text.wrap -> decorate), and a namespaced task as a pipeline stage (>> text.shout)
expect_body GET "/tasks/namespaced/hello" '{"shout":"HELLO!","wrapped":"<hello>","piped":"HELLO!"}'

# non-deterministic values exercised live
expect_match GET "/ns/uuid" '"v4":"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}"'
expect_match GET "/ns/time" '"formatted":"27/04/2026"'

# Spec 068: reserved words normalize back as names (map keys + member access) over the live path
expect_body GET "/ns/reserved-as-names" '{"doc":"d","feature":"f","env":"e","date":"dt","db":"b","time":"t"}'

# generated docs (public contract)
expect_status GET "/openapi.json" 200
expect_match  GET "/openapi.json" '/control/match'
expect_status GET "/docs" 200

echo "== stopping server =="
echo "OK: marreta test scenarios + ${PASS} live smoke assertions passed"
