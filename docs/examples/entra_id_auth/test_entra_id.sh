#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT_DIR"

fail() {
  echo "FAIL $*" >&2
  exit 1
}

skip() {
  echo "SKIP $*"
  exit 0
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || fail "missing command '$1'"
}

if [[ -z "${MARRETA_BIN:-}" ]]; then
  if command -v marreta >/dev/null 2>&1; then
    MARRETA_BIN="$(command -v marreta)"
  elif [[ -x "$ROOT_DIR/../../marreta-lang/target/debug/marreta" ]]; then
    MARRETA_BIN="$ROOT_DIR/../../marreta-lang/target/debug/marreta"
  else
    fail "set MARRETA_BIN or put marreta in PATH"
  fi
fi

require_command curl
require_command python3

if [[ -z "${ENTRA_ACCESS_TOKEN:-}" ]]; then
  require_command node

  for name in AZURE_TENANT_ID AZURE_API_CLIENT_ID AZURE_CLIENT_ID AZURE_CLIENT_SECRET; do
    [[ -n "${!name:-}" ]] || skip "$name is not set and ENTRA_ACCESS_TOKEN was not provided"
  done

  if [[ ! -d "$ROOT_DIR/node_modules/@azure/identity" ]]; then
    fail "run 'npm install' in $ROOT_DIR or provide ENTRA_ACCESS_TOKEN directly"
  fi

  ENTRA_ACCESS_TOKEN="$(node "$ROOT_DIR/get_token.mjs")"
fi

if [[ -z "${AZURE_TENANT_ID:-}" && -z "${ENTRA_TENANT_ID:-}" ]]; then
  skip "AZURE_TENANT_ID or ENTRA_TENANT_ID is required"
fi

TENANT_ID="${ENTRA_TENANT_ID:-${AZURE_TENANT_ID:-}}"
API_CLIENT_ID="${AZURE_API_CLIENT_ID:-}"

export ENTRA_ISSUER="${ENTRA_ISSUER:-https://sts.windows.net/${TENANT_ID}/}"
export ENTRA_AUDIENCE="${ENTRA_AUDIENCE:-api://${API_CLIENT_ID}}"
export ENTRA_JWKS_URL="${ENTRA_JWKS_URL:-https://login.microsoftonline.com/common/discovery/keys}"

[[ -n "$ENTRA_ISSUER" ]] || skip "ENTRA_ISSUER could not be resolved"
[[ -n "$ENTRA_AUDIENCE" ]] || skip "ENTRA_AUDIENCE could not be resolved"
[[ -n "$ENTRA_JWKS_URL" ]] || skip "ENTRA_JWKS_URL could not be resolved"
[[ "$ENTRA_AUDIENCE" != "api://" ]] || skip "AZURE_API_CLIENT_ID or ENTRA_AUDIENCE is required"

PORT="${MARRETA_PORT:-4902}"
export MARRETA_HOST="${MARRETA_HOST:-127.0.0.1}"
export MARRETA_PORT="$PORT"

SERVER_LOG="$(mktemp)"
BODY_FILE="$(mktemp)"
SERVER_PID=""

cleanup() {
  if [[ -n "$SERVER_PID" ]] && kill -0 "$SERVER_PID" >/dev/null 2>&1; then
    kill "$SERVER_PID" >/dev/null 2>&1 || true
    wait "$SERVER_PID" >/dev/null 2>&1 || true
  fi
  rm -f "$SERVER_LOG" "$BODY_FILE"
}
trap cleanup EXIT

"$MARRETA_BIN" serve >"$SERVER_LOG" 2>&1 &
SERVER_PID="$!"

for _ in $(seq 1 60); do
  if curl -fsS "http://127.0.0.1:${PORT}/_health" >/dev/null 2>&1; then
    break
  fi
  if ! kill -0 "$SERVER_PID" >/dev/null 2>&1; then
    cat "$SERVER_LOG" >&2
    fail "marreta serve exited before becoming healthy"
  fi
  sleep 0.25
done

if ! curl -fsS "http://127.0.0.1:${PORT}/_health" >/dev/null 2>&1; then
  cat "$SERVER_LOG" >&2
  fail "health check did not become ready"
fi
echo "PASS health route is available"

http_status() {
  local token="$1"
  local path="$2"

  if [[ -n "$token" ]]; then
    curl -sS -o "$BODY_FILE" -w "%{http_code}" \
      -H "Authorization: Bearer ${token}" \
      "http://127.0.0.1:${PORT}${path}"
  else
    curl -sS -o "$BODY_FILE" -w "%{http_code}" \
      "http://127.0.0.1:${PORT}${path}"
  fi
}

status="$(http_status "" "/secure")"
[[ "$status" == "401" ]] || fail "missing token expected 401, got $status: $(cat "$BODY_FILE")"
echo "PASS missing token rejected"

status="$(http_status "$ENTRA_ACCESS_TOKEN" "/secure")"
[[ "$status" == "200" ]] || fail "valid token expected 200, got $status: $(cat "$BODY_FILE")"

python3 - "$BODY_FILE" "$ENTRA_ISSUER" "$ENTRA_AUDIENCE" <<'PY'
import json
import sys

path, expected_issuer, expected_audience = sys.argv[1:]
with open(path, "r", encoding="utf-8") as handle:
    body = json.load(handle)

assert body.get("ok") is True, body
assert body.get("issuer") == expected_issuer, body
assert body.get("audience") == expected_audience, body
assert body.get("subject") or body.get("user_id"), body
PY
echo "PASS valid Entra token accepted"

if [[ -n "${ENTRA_BAD_AUDIENCE_TOKEN:-}" ]]; then
  status="$(http_status "$ENTRA_BAD_AUDIENCE_TOKEN" "/secure")"
  [[ "$status" == "401" ]] || fail "bad audience token expected 401, got $status: $(cat "$BODY_FILE")"
  echo "PASS wrong audience token rejected"
else
  echo "SKIP wrong audience token not provided"
fi

if [[ -n "${ENTRA_BAD_ISSUER_TOKEN:-}" ]]; then
  status="$(http_status "$ENTRA_BAD_ISSUER_TOKEN" "/secure")"
  [[ "$status" == "401" ]] || fail "bad issuer token expected 401, got $status: $(cat "$BODY_FILE")"
  echo "PASS wrong issuer token rejected"
else
  echo "SKIP wrong issuer token not provided"
fi

if [[ -n "${ENTRA_EXPIRED_TOKEN:-}" ]]; then
  status="$(http_status "$ENTRA_EXPIRED_TOKEN" "/secure")"
  [[ "$status" == "401" ]] || fail "expired token expected 401, got $status: $(cat "$BODY_FILE")"
  echo "PASS expired token rejected"
else
  echo "SKIP expired token not provided"
fi

if [[ "${ENTRA_TEST_ROLE_CHECK:-0}" == "1" ]]; then
  status="$(http_status "$ENTRA_ACCESS_TOKEN" "/secure/role")"
  [[ "$status" == "200" ]] || fail "role route expected 200, got $status: $(cat "$BODY_FILE")"
  echo "PASS role claim authorized"
else
  echo "SKIP role route check disabled; set ENTRA_TEST_ROLE_CHECK=1 to enable"
fi
