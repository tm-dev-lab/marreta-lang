#!/usr/bin/env bash
# Saturation run: find each stack's maximum sustainable throughput within the 1 CPU / 1 GB app cap.
# MongoDB runs with HEADROOM (no cap override, the base compose) so the APP container is the limiting
# resource, not the database (METHODOLOGY.md). The validity guard is enforced, not just sampled: at
# the breaking point the app's and MongoDB's peak CPU decide the limiter, and the result is labeled
# "app" (the 1-CPU app cap is the wall) or "whole-system" (the app did not reach its cap, so
# MongoDB or another shared resource limited it). That label is written to saturation_summary.json.
#
# Knobs: TARGETS, RATES, WARMUP, DURATION, P95_BOUND_MS, APP_CPU_LIMITED (default 90).
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BENCH="$(cd "$SCRIPT_DIR/.." && pwd)"
export RUN_TS="${RUN_TS:-sat_$(date -u +%Y%m%dT%H%M%SZ)}"
RUN_ROOT="$BENCH/results/$RUN_TS"

TARGETS="${TARGETS:-marreta fastapi nest spring}"
# 250-step ladder in the range the 1-CPU-capped apps actually break (all of them top out by
# ~1500). The same granularity for every contender so no stack gets its ceiling measured more
# precisely than another (a coarse 2x ladder rounded faster stacks down to the last rung).
RATES="${RATES:-500 750 1000 1250 1500 2000}"
# Warmup matches the fixed-load run (120s): a short warmup measures a half-compiled JVM, so a JIT
# stack would "break" at a rate it sustains cleanly once warm, contradicting the fixed-load cells.
# The window stays shorter than fixed-load (this run only locates the breaking point, it is not the
# precise steady-state measurement) but is kept over one minute.
export WARMUP="${WARMUP:-120s}"
export DURATION="${DURATION:-90s}"
P95_BOUND_MS="${P95_BOUND_MS:-500}"
APP_CPU_LIMITED="${APP_CPU_LIMITED:-90}"   # app CPU% at break >= this => the 1-CPU cap was the wall
unset COMPOSE_FILES   # MongoDB has headroom (uncapped) for the saturation run, by design.

mkdir -p "$RUN_ROOT"
echo "Saturation $RUN_TS -> $RUN_ROOT (mongo headroom, p95 bound ${P95_BOUND_MS}ms)"
declare -A MAX ACPU MCPU LIMITER

for target in $TARGETS; do
  echo "=== saturation :: $target ==="
  best=0 acpu_brk=0 mcpu_brk=0 limiter="ceiling-not-reached (raise RATES)"
  for rate in $RATES; do
    SUBDIR="saturation/${target}/rate_${rate}" RATE="$rate" "$SCRIPT_DIR/run_one.sh" "$target" >/dev/null 2>&1 || true
    f="$RUN_ROOT/saturation/${target}/rate_${rate}/summary.json"
    if [[ ! -f "$f" ]]; then echo "  rate=$rate: no summary (infra failure), stop"; limiter="error"; break; fi
    thr=$(jq -r '.throughput_rps' "$f"); err=$(jq -r '.error_rate' "$f")
    p95=$(jq -r '.latency_ms.p95' "$f")
    acpu=$(jq -r '.cpu_percent.peak' "$f"); mcpu=$(jq -r '.mongodb.cpu_peak' "$f")
    sustained=$(awk "BEGIN{print (($thr>=0.95*$rate)&&($err<0.01)&&($p95<$P95_BOUND_MS))?1:0}")
    echo "  rate=$rate -> thr=$thr p95=${p95}ms err=$err appCpuPeak=${acpu}% mongoCpuPeak=${mcpu}% sustained=$sustained"
    if [[ "$sustained" == "1" ]]; then
      best=$thr
    else
      acpu_brk=$acpu; mcpu_brk=$mcpu
      limiter=$(awk "BEGIN{print ($acpu>=$APP_CPU_LIMITED)?\"app\":\"whole-system\"}")
      echo "  -> breaking point at offered rate $rate; limiter=$limiter (app cpu peak ${acpu}%, mongo cpu peak ${mcpu}%)"
      break
    fi
  done
  MAX[$target]=$best; ACPU[$target]=$acpu_brk; MCPU[$target]=$mcpu_brk; LIMITER[$target]=$limiter
done

( cd "$BENCH" && docker compose down -v >/dev/null 2>&1 || true )

{
  echo "{"
  first=1
  for t in $TARGETS; do
    [[ $first -eq 0 ]] && echo ","
    first=0
    printf '  "%s": {"max_sustained_rps": %s, "breaking_app_cpu_peak": %s, "breaking_mongo_cpu_peak": %s, "limiter": "%s"}' \
      "$t" "${MAX[$t]:-0}" "${ACPU[$t]:-0}" "${MCPU[$t]:-0}" "${LIMITER[$t]:-unknown}"
  done
  echo ""; echo "}"
} > "$RUN_ROOT/saturation_summary.json"

echo "=== max sustainable throughput (req/s), with limiter ==="
for t in $TARGETS; do echo "  $t: ${MAX[$t]:-0} req/s  (limiter: ${LIMITER[$t]:-unknown})"; done
echo "wrote $RUN_ROOT/saturation_summary.json"
