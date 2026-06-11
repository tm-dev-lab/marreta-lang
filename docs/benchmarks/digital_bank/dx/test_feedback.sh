#!/usr/bin/env bash
# Measure the test feedback loop: the same provider-free, route-level suite per stack (parity of
# strategy), reporting each framework's OWN logged test time (marreta "..ms", pytest "in ..s",
# jest "Time: ..s", surefire "Time elapsed"). The framework number is more faithful than wall time
# around `docker run`: it excludes container start and the differing base images. Wall time is kept
# alongside for reference. Writes dx/feedback.json, consumed by measure.py.
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BENCH="$(cd "$SCRIPT_DIR/.." && pwd)"
APPS="$BENCH/apps"
OUT="$SCRIPT_DIR/feedback.json"
MARRETA_BIN="${MARRETA_BIN:-$HOME/.local/bin/marreta}"

declare -A SELF WALL NOTE
fail=0
elapsed() { awk "BEGIN{printf \"%.2f\", $2-$1}"; }

# Framework self-reported test time (seconds), parsed from each runner's output.
sr_marreta() { grep -oE '[0-9]+ms' "$1" | tail -1 | grep -oE '[0-9]+' | awk '{printf "%.3f", $1/1000}'; }
sr_pytest()  { grep -oE 'in [0-9.]+s'             "$1" | tail -1 | grep -oE '[0-9.]+'; }
sr_jest()    { grep -oE 'Time:[[:space:]]+[0-9.]+' "$1" | grep -oE '[0-9.]+' | tail -1; }
sr_maven()   { grep -oE 'Time elapsed: [0-9.]+'   "$1" | grep -oE '[0-9.]+' | sort -rn | head -1; }

echo "== marreta =="
# Fail loudly if the runtime is missing, instead of silently reporting n/a (it once ran a
# non-existent MARRETA_BIN and the suite recorded no time without complaint).
[ -x "$MARRETA_BIN" ] || { echo "marreta binary not found/executable at $MARRETA_BIN (set MARRETA_BIN)"; exit 1; }
( cd "$APPS/marreta" && "$MARRETA_BIN" test ) >/tmp/tf_marreta 2>&1 || true   # warm
s=$(date +%s.%N); ( cd "$APPS/marreta" && "$MARRETA_BIN" test ) >/tmp/tf_marreta 2>&1; rc=$?; e=$(date +%s.%N)
WALL[marreta]=$(elapsed "$s" "$e"); SELF[marreta]=$(sr_marreta /tmp/tf_marreta)
NOTE[marreta]="in-memory scenario runner; provider stubbed"
[ $rc -ne 0 ] && { echo "TESTS FAILED (marreta):"; cat /tmp/tf_marreta; fail=1; }
[ -z "${SELF[marreta]}" ] && { echo "marreta test time not parsed from output:"; cat /tmp/tf_marreta; fail=1; }

run_docker() { # name dir tag parser note
  local name=$1 dir=$2 tag=$3 parser=$4 note=$5
  echo "== $name (build test image) =="
  if ! docker build -q -f "$APPS/$dir/Dockerfile.test" -t "$tag" "$APPS/$dir" >/dev/null 2>/tmp/tf_build_$name; then
    echo "BUILD FAILED ($name):"; tail -20 /tmp/tf_build_$name; fail=1; return
  fi
  local s e rc
  s=$(date +%s.%N); docker run --rm "$tag" >/tmp/tf_$name 2>&1; rc=$?; e=$(date +%s.%N)
  WALL[$name]=$(elapsed "$s" "$e"); SELF[$name]=$("$parser" /tmp/tf_$name); NOTE[$name]=$note
  [ $rc -ne 0 ] && { echo "TESTS FAILED ($name):"; tail -25 /tmp/tf_$name; fail=1; }
}

run_docker fastapi fastapi bench-bank-fastapi-test sr_pytest "in-process ASGI (TestClient); no network server; provider mocked"
run_docker nest    nest    bench-bank-nest-test    sr_jest   "in-process app (supertest); no network server; provider mocked"
run_docker spring  spring  bench-bank-spring-test  sr_maven  "MVC test slice (@WebMvcTest); no server; provider mocked"

{
  echo "{"
  first=1
  for name in marreta fastapi nest spring; do
    [ $first -eq 0 ] && echo ","
    first=0
    printf '  "%s": {"seconds": %s, "wall_seconds": %s, "note": "%s"}' \
      "$name" "${SELF[$name]:-null}" "${WALL[$name]:-null}" "${NOTE[$name]}"
  done
  echo ""; echo "}"
} > "$OUT"

echo ""; echo "Wrote $OUT:"; cat "$OUT"
echo ""; echo "Regenerate DX.md: python3 dx/measure.py"
exit "$fail"
