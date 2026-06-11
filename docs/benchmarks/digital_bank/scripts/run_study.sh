#!/usr/bin/env bash
# Orchestrate the full pre-registered study (see METHODOLOGY.md):
#   - fixed-load comparison: levels x repetitions x targets, INTERLEAVED (rep-outer, target-inner).
#     MongoDB runs with headroom (uncapped) as the shared dependency, not under test; only the apps
#     are capped (1 CPU / 1 GB). The MongoDB monitor (validity guard) confirms it never bottlenecks.
#   - then the aggregation (median + CV + consistency gate).
# The saturation run is a separate step, run_saturation.sh, not included here.
#
# Knobs via env (defaults are the pre-registered values):
#   LEVELS="200 500 1000"   REPS=3   WARMUP=120s   DURATION=300s   TARGETS="marreta fastapi nest spring"
# For a quick pipeline smoke, override with tiny values, e.g.:
#   LEVELS=100 REPS=1 WARMUP=5s DURATION=10s ./scripts/run_study.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BENCH_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
export RUN_TS="${RUN_TS:-$(date -u +"%Y%m%dT%H%M%SZ")}"
RUN_ROOT="$BENCH_DIR/results/$RUN_TS"

LEVELS="${LEVELS:-200 500 1000}"
REPS="${REPS:-3}"
TARGETS="${TARGETS:-marreta fastapi nest spring}"
export WARMUP="${WARMUP:-120s}"
export DURATION="${DURATION:-300s}"
# MongoDB runs with headroom (no cap override): it is the shared dependency, not under test.
unset COMPOSE_FILES

mkdir -p "$RUN_ROOT"
echo "Study run $RUN_TS -> $RUN_ROOT"
echo "levels=[$LEVELS] reps=$REPS targets=[$TARGETS] warmup=$WARMUP window=$DURATION (mongo headroom)"

failed=0
for level in $LEVELS; do
  for rep in $(seq 1 "$REPS"); do
    # Interleave: every target runs once per rep before the next rep, avoiding ordering bias.
    for target in $TARGETS; do
      SUBDIR="rate_${level}/rep_${rep}/${target}" RATE="$level" \
        "$SCRIPT_DIR/run_one.sh" "$target" || { echo "run failed: $target rate=$level rep=$rep" >&2; failed=1; }
      echo ""
    done
  done
done

(cd "$BENCH_DIR" && docker compose down -v >/dev/null 2>&1 || true)

echo "=== aggregate (median + CV + consistency gate) ==="
python3 "$SCRIPT_DIR/aggregate.py" "$RUN_ROOT" || failed=1

echo "Study outputs: $RUN_ROOT"
exit "$failed"
