#!/usr/bin/env bash
# One measured run of one stack at one load level: warm up (discarded), then a steady-state
# measurement window with k6 plus container resource sampling. Emits a compact summary.json.
# The orchestrator (run_study.sh) calls this for every level/rep/target. Knobs via env:
#   RATE       target arrival rate (req/s)         default 500
#   WARMUP     warmup duration (discarded)          default 120s
#   DURATION   measurement window                   default 300s
#   SUBDIR     results subdirectory under the run   default the target name
#   COMPOSE_FILES  extra `-f` compose files (e.g. the mongo cap override)
set -euo pipefail

TARGET="${1:?target required: marreta|fastapi|nest|spring}"

case "$TARGET" in
  marreta) CONTAINER="bench-bank-marreta"; HOST_PORT="18080" ;;
  fastapi) CONTAINER="bench-bank-fastapi"; HOST_PORT="18081" ;;
  nest)    CONTAINER="bench-bank-nest";    HOST_PORT="18082" ;;
  spring)  CONTAINER="bench-bank-spring";  HOST_PORT="18083" ;;
  *) echo "invalid target '$TARGET'; expected marreta, fastapi, nest, or spring" >&2; exit 2 ;;
esac

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BENCH_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
TS="${RUN_TS:-$(date -u +"%Y%m%dT%H%M%SZ")}"
SUBDIR="${SUBDIR:-$TARGET}"
RUN_DIR="$BENCH_DIR/results/$TS/$SUBDIR"
RATE="${RATE:-500}"
WARMUP="${WARMUP:-120s}"
DURATION="${DURATION:-300s}"
STATS_INTERVAL="${STATS_INTERVAL:-2}"
ACCOUNTS="${ACCOUNTS:-50}"
MARRETA_IMAGE="${MARRETA_IMAGE:-marreta-lang:dev}"
read -r -a COMPOSE_ARGS <<< "${COMPOSE_FILES:-}"

mkdir -p "$RUN_DIR"; chmod 777 "$RUN_DIR"
cd "$BENCH_DIR"

echo "=== digital_bank :: $TARGET :: rate=$RATE warmup=$WARMUP window=$DURATION ==="

compose() { docker compose -f docker-compose.yml "${COMPOSE_ARGS[@]}" "$@"; }

if [[ "$TARGET" == "marreta" ]]; then
  if ! docker image inspect "$MARRETA_IMAGE" >/dev/null 2>&1; then
    echo "marreta image '$MARRETA_IMAGE' not found; build it in marreta-lang first" >&2
    exit 1
  fi
else
  compose build --quiet "$TARGET"
fi
export MARRETA_IMAGE

compose up -d --wait mongodb
compose exec -T mongodb mongosh -u marreta -p marreta-secret --authenticationDatabase admin \
  --quiet --eval 'db.getSiblingDB("bank").dropDatabase()' >/dev/null

# Startup time: from container start to the first healthy response.
START_EPOCH="$(date +%s.%N)"
compose up -d "$TARGET"

cleanup() {
  [[ -n "${STATS_PID:-}" ]] && kill "$STATS_PID" 2>/dev/null || true
  [[ -n "${MONGO_STATS_PID:-}" ]] && kill "$MONGO_STATS_PID" 2>/dev/null || true
  compose stop "$TARGET" >/dev/null 2>&1 || true
  docker logs "$CONTAINER" > "$RUN_DIR/container.log" 2>&1 || true
  compose rm -f "$TARGET" >/dev/null 2>&1 || true
}
trap cleanup EXIT

BASE_URL="http://127.0.0.1:${HOST_PORT}"
# Poll readiness at a fine interval so startup_ms reflects the real time-to-first-request, not a
# coarse polling floor (a 0.5s sleep rounded every startup up to the next half second). 20ms x 6000
# keeps the ~120s ceiling for the slowest (JVM) stack.
for _ in $(seq 1 6000); do curl -fsS "$BASE_URL/health" >/dev/null 2>&1 && break; sleep 0.02; done
if ! curl -fsS "$BASE_URL/health" >/dev/null 2>&1; then
  compose logs "$TARGET" >&2; echo "service '$TARGET' did not become ready" >&2; exit 1
fi
STARTUP_MS="$(awk "BEGIN{printf \"%.0f\", ($(date +%s.%N) - $START_EPOCH) * 1000}")"

# Idle footprint: one memory sample before any load.
IDLE_MEM_MIB="$(docker stats --no-stream --format '{{.MemUsage}}' "$CONTAINER" | awk '{
  v=$1; if (v ~ /KiB$/) printf "%.1f",(v+0)/1024; else if (v ~ /GiB$/) printf "%.1f",(v+0)*1024; else printf "%.1f",v+0 }')"

# Committed per-run config snapshot (artifact policy): exactly what produced this run.
MONGO_CAPPED=$([[ "${COMPOSE_FILES:-}" == *mongo-capped* ]] && echo true || echo false)
cat > "$RUN_DIR/config.json" <<EOF
{
  "timestamp": "$TS",
  "benchmark": "digital_bank",
  "target": "$TARGET",
  "datastore": "mongodb",
  "rate": $RATE,
  "warmup": "$WARMUP",
  "window": "$DURATION",
  "accounts": $ACCOUNTS,
  "app_limits": { "cpus": 1.0, "memory": "1g" },
  "mongo_capped": $MONGO_CAPPED,
  "marreta_image": "$MARRETA_IMAGE",
  "host": { "cores": $(nproc), "ram_gb": $(awk '/MemTotal/{printf "%.0f", $2/1024/1024}' /proc/meminfo) }
}
EOF

NETWORK_ID="$(docker inspect "$CONTAINER" --format='{{range .NetworkSettings.Networks}}{{.NetworkID}}{{end}}' | head -1)"
run_k6() { # $1=duration $2=summary-export-path-or-empty
  local export_arg=()
  [[ -n "$2" ]] && export_arg=(--summary-export "$2")
  docker run --rm --network "$NETWORK_ID" -v "$BENCH_DIR/k6:/scripts:ro" -v "$RUN_DIR:/results" \
    -e BASE_URL="http://${CONTAINER}:8080" -e RATE="$RATE" -e DURATION="$1" -e ACCOUNTS="$ACCOUNTS" \
    grafana/k6@sha256:6a3a6bc75c7f409f327c6d6e3c1ed1168545487a542885b6994a5b1a6c263651 run "${export_arg[@]}" /scripts/scenarios.js
}

echo "-- warmup ($WARMUP, discarded) --"
run_k6 "$WARMUP" "" >/dev/null 2>&1 || true

echo "-- measurement window ($DURATION) --"
STATS_INTERVAL="$STATS_INTERVAL" "$SCRIPT_DIR/collect_stats.sh" "$CONTAINER" "$RUN_DIR/stats.csv" &
STATS_PID=$!
# Validity guard: sample MongoDB too, so a saturated database is visible and never mistaken for app
# load (especially on the saturation run, where MongoDB has headroom).
STATS_INTERVAL="$STATS_INTERVAL" "$SCRIPT_DIR/collect_stats.sh" bench-bank-mongodb "$RUN_DIR/mongo_stats.csv" &
MONGO_STATS_PID=$!
# A k6 threshold breach (a slow stack) exits non-zero. That is data, not a harness failure, so do
# not abort: the summary is still written and the slow numbers are recorded.
run_k6 "$DURATION" "/results/k6_summary.json" || true
kill "$STATS_PID" "$MONGO_STATS_PID" 2>/dev/null || true; STATS_PID=""; MONGO_STATS_PID=""

# Compact per-run summary.json (the committed artifact; raw logs/csv are gitignored).
read -r CPU_AVG CPU_PEAK < <(tail -n +2 "$RUN_DIR/stats.csv" | awk -F',' '
  { gsub(/%/,"",$2); s+=$2+0; n++; if($2+0>m) m=$2+0 } END{ if(n==0)print "0 0"; else printf "%.1f %.1f\n", s/n, m }')
read -r MEM_AVG MEM_PEAK < <(tail -n +2 "$RUN_DIR/stats.csv" | awk -F',' '
  function mib(v){ if(v~/KiB$/)return (v+0)/1024; if(v~/MiB$/)return v+0; if(v~/GiB$/)return (v+0)*1024; return v+0 }
  { split($3,p," / "); x=mib(p[1]); s+=x; n++; if(x>m)m=x } END{ if(n==0)print "0 0"; else printf "%.1f %.1f\n", s/n, m }')
# MongoDB peak utilization over the window (validity guard).
MONGO_CPU_PEAK=$(tail -n +2 "$RUN_DIR/mongo_stats.csv" 2>/dev/null | awk -F',' '{ gsub(/%/,"",$2); if($2+0>m)m=$2+0 } END{ printf "%.1f", m+0 }')
MONGO_MEM_PEAK=$(tail -n +2 "$RUN_DIR/mongo_stats.csv" 2>/dev/null | awk -F',' '
  function mib(v){ if(v~/KiB$/)return (v+0)/1024; if(v~/MiB$/)return v+0; if(v~/GiB$/)return (v+0)*1024; return v+0 }
  { split($3,p," / "); x=mib(p[1]); if(x>m)m=x } END{ printf "%.1f", m+0 }')
: "${MONGO_CPU_PEAK:=0}" "${MONGO_MEM_PEAK:=0}"

jq -n \
  --arg target "$TARGET" --argjson rate "$RATE" --arg warmup "$WARMUP" --arg window "$DURATION" \
  --argjson startup_ms "$STARTUP_MS" --argjson idle_mem "$IDLE_MEM_MIB" \
  --argjson cpu_avg "$CPU_AVG" --argjson cpu_peak "$CPU_PEAK" --argjson mem_avg "$MEM_AVG" --argjson mem_peak "$MEM_PEAK" \
  --argjson mongo_cpu "$MONGO_CPU_PEAK" --argjson mongo_mem "$MONGO_MEM_PEAK" \
  --slurpfile k6 "$RUN_DIR/k6_summary.json" '
  ($k6[0].metrics) as $m | def ms(v): ((v // 0) * 1000 | round / 1000);
  {
    target: $target, rate: $rate, warmup: $warmup, window: $window,
    throughput_rps: ($m.http_reqs.rate | round),
    requests: $m.http_reqs.count,
    error_rate: ($m.http_req_failed.value),
    latency_ms: { avg: ms($m.http_req_duration.avg),
                  p50: ms($m.http_req_duration["p(50)"] // $m.http_req_duration.med),
                  p90: ms($m.http_req_duration["p(90)"]), p95: ms($m.http_req_duration["p(95)"]),
                  p99: ms($m.http_req_duration["p(99)"]) },
    cpu_percent: { avg: $cpu_avg, peak: $cpu_peak },
    memory_mib: { avg: $mem_avg, peak: $mem_peak, idle: $idle_mem },
    mongodb: { cpu_peak: $mongo_cpu, mem_peak: $mongo_mem },
    startup_ms: $startup_ms
  }' > "$RUN_DIR/summary.json"

echo "-- summary --"; cat "$RUN_DIR/summary.json"
echo "files=$RUN_DIR"
