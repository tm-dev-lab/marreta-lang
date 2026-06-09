#!/usr/bin/env bash
#
# Release smoke test.
#
# Given a marreta binary, exercise the core CLI loop end to end (init, fmt,
# lint, doctor, test, serve + curl /greetings) and stop the server. Used by the
# Release Smoke Test workflow to confirm a published binary is minimally
# functional on each target. Exits non-zero on the first failure.

set -euo pipefail

MARRETA="${1:?usage: release_smoke.sh <path-to-marreta-binary>}"
chmod +x "$MARRETA"
# macOS binaries downloaded via the API are not quarantined, but clear the
# attribute defensively in case Gatekeeper ever flags it. No-op elsewhere.
command -v xattr >/dev/null 2>&1 && xattr -dr com.apple.quarantine "$MARRETA" 2>/dev/null || true
# Resolve to an absolute path so it still works after we cd into the project.
MARRETA="$(cd "$(dirname "$MARRETA")" && pwd)/$(basename "$MARRETA")"

echo "== marreta --version =="
"$MARRETA" --version

WORK="$(mktemp -d)"
cd "$WORK"

echo "== marreta init demo =="
"$MARRETA" init demo
cd demo

echo "== marreta fmt --check =="
"$MARRETA" fmt --check

echo "== marreta lint =="
"$MARRETA" lint

echo "== marreta doctor =="
"$MARRETA" doctor

echo "== marreta test =="
"$MARRETA" test

echo "== marreta serve + curl /greetings =="
"$MARRETA" serve > serve.log 2>&1 &
SERVER_PID=$!
cleanup() {
  kill "$SERVER_PID" 2>/dev/null || true
  wait "$SERVER_PID" 2>/dev/null || true
}
trap cleanup EXIT

ready=
for _ in $(seq 1 60); do
  if curl -fsS http://localhost:8080/greetings -o body.txt 2>/dev/null; then
    ready=1
    break
  fi
  sleep 0.5
done

if [ -z "$ready" ]; then
  echo "ERROR: server did not respond on /greetings"
  echo "--- serve.log ---"
  cat serve.log || true
  exit 1
fi

echo "GET /greetings ->"
cat body.txt
echo
test -s body.txt # body must be non-empty

echo "== stopping server =="
cleanup
trap - EXIT

echo "SMOKE OK"
