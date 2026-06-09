#!/usr/bin/env bash
set -euo pipefail

TARGET="${1:?target required: marreta|fastapi|nest}"

case "$TARGET" in
  marreta)
    CONTAINER="bench-bank-marreta"
    HOST_PORT="18080"
    ;;
  fastapi)
    CONTAINER="bench-bank-fastapi"
    HOST_PORT="18081"
    ;;
  nest)
    CONTAINER="bench-bank-nest"
    HOST_PORT="18082"
    ;;
  *)
    echo "invalid target '$TARGET'; expected marreta, fastapi, or nest" >&2
    exit 2
    ;;
esac

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BENCH_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
TS="${RUN_TS:-$(date -u +"%Y%m%dT%H%M%SZ")}"
RUN_DIR="$BENCH_DIR/results/$TS/$TARGET"
RATE="${RATE:-500}"
DURATION="${DURATION:-60s}"
STATS_INTERVAL="${STATS_INTERVAL:-2}"
ACCOUNTS="${ACCOUNTS:-50}"
MARRETA_IMAGE="${MARRETA_IMAGE:-marreta-lang:dev}"

mkdir -p "$RUN_DIR"
chmod 777 "$RUN_DIR"

cd "$BENCH_DIR"

echo "=== digital_bank :: $TARGET ==="
echo "Output   : $RUN_DIR"
echo "Rate     : $RATE req/s"
echo "Duration : $DURATION"

if [[ "$TARGET" == "marreta" ]]; then
  # The marreta runtime image is built by the marreta-lang repo; the bench only
  # consumes it. We never build the runtime here.
  if ! docker image inspect "$MARRETA_IMAGE" >/dev/null 2>&1; then
    echo "marreta image '$MARRETA_IMAGE' not found; build it in the marreta-lang repo first" >&2
    echo "  cd marreta-lang && cargo build --release && docker build -t $MARRETA_IMAGE ." >&2
    exit 1
  fi
else
  docker compose build --quiet "$TARGET"
fi

export MARRETA_IMAGE

# Shared dependency: bring MongoDB up and wait until it is healthy.
docker compose up -d --wait mongodb

# Clean slate: drop the bank database so every run starts empty.
docker compose exec -T mongodb mongosh -u marreta -p marreta-secret \
  --authenticationDatabase admin --quiet \
  --eval 'db.getSiblingDB("bank").dropDatabase()' >/dev/null

docker compose up -d "$TARGET"

cleanup() {
  if [[ -n "${STATS_PID:-}" ]]; then
    kill "$STATS_PID" 2>/dev/null || true
  fi
  docker compose stop "$TARGET" >/dev/null 2>&1 || true
  docker logs "$CONTAINER" > "$RUN_DIR/container.log" 2>&1 || true
  docker compose rm -f "$TARGET" >/dev/null 2>&1 || true
}
trap cleanup EXIT

BASE_URL="http://127.0.0.1:${HOST_PORT}"
for _ in $(seq 1 90); do
  if curl -fsS "$BASE_URL/health" >/dev/null 2>&1; then
    break
  fi
  sleep 0.5
done

if ! curl -fsS "$BASE_URL/health" >/dev/null 2>&1; then
  docker compose logs "$TARGET" >&2
  echo "service '$TARGET' did not become ready" >&2
  exit 1
fi

cat > "$RUN_DIR/config.json" <<EOF
{
  "timestamp": "$TS",
  "benchmark": "digital_bank",
  "target": "$TARGET",
  "container": "$CONTAINER",
  "datastore": "mongodb",
  "resource_limits": {
    "cpus": 1.0,
    "memory": "1g"
  },
  "rate": $RATE,
  "duration": "$DURATION",
  "accounts": $ACCOUNTS,
  "stats_interval_seconds": $STATS_INTERVAL,
  "endpoints": ["/health", "/accounts", "/accounts/:id", "/accounts/:id/balance", "/accounts/:id/deposit", "/accounts/:id/withdraw", "/transfers", "/accounts/:id/transactions"],
  "host": {
    "cores": $(nproc),
    "ram_gb": $(awk '/MemTotal/{printf "%.0f", $2/1024/1024}' /proc/meminfo)
  }
}
EOF

STATS_INTERVAL="$STATS_INTERVAL" "$SCRIPT_DIR/collect_stats.sh" "$CONTAINER" "$RUN_DIR/stats.csv" &
STATS_PID=$!

NETWORK_ID="$(docker inspect "$CONTAINER" --format='{{range .NetworkSettings.Networks}}{{.NetworkID}}{{end}}' | head -1)"

docker run --rm \
  --network "$NETWORK_ID" \
  -v "$BENCH_DIR/k6:/scripts:ro" \
  -v "$RUN_DIR:/results" \
  -e BASE_URL="http://${CONTAINER}:8080" \
  -e RATE="$RATE" \
  -e DURATION="$DURATION" \
  -e ACCOUNTS="$ACCOUNTS" \
  grafana/k6:latest run \
    --summary-export /results/k6_summary.json \
    /scripts/scenarios.js

kill "$STATS_PID" 2>/dev/null || true
STATS_PID=""

echo ""
echo "--- Summary: $TARGET ---"
if command -v jq >/dev/null 2>&1 && [[ -f "$RUN_DIR/k6_summary.json" ]]; then
  jq -r '
    .metrics.http_reqs as $reqs |
    .metrics.http_req_failed as $failed |
    .metrics.http_req_duration as $dur |
    def ms($v): (($v // 0) * 1000 | round / 1000);
    "throughput=\($reqs.rate | round) req/s total=\($reqs.count)",
    "error_rate=\($failed.value * 100 | . * 100 | round / 100)%",
    "latency_ms avg=\(ms($dur.avg)) p50=\(ms($dur["p(50)"] // $dur.med)) p90=\(ms($dur["p(90)"])) p95=\(ms($dur["p(95)"])) p99=\(ms($dur["p(99)"]))"
  ' "$RUN_DIR/k6_summary.json"
fi

if [[ -f "$RUN_DIR/stats.csv" ]]; then
  # CPU and memory: report both sustained average and peak over the run window.
  read -r cpu_avg cpu_peak < <(tail -n +2 "$RUN_DIR/stats.csv" | awk -F',' '
    { gsub(/%/,"",$2); sum+=$2+0; n++; if($2+0>max) max=$2+0 }
    END { if(n==0){print "0 0"} else {printf "%.1f %.1f\n", sum/n, max} }')
  read -r mem_avg mem_peak < <(tail -n +2 "$RUN_DIR/stats.csv" | awk -F',' '
    function to_mib(v) {
      if (v ~ /KiB$/) return (v + 0) / 1024
      if (v ~ /MiB$/) return v + 0
      if (v ~ /GiB$/) return (v + 0) * 1024
      return v + 0
    }
    { split($3, parts, " / "); m = to_mib(parts[1]); sum+=m; n++; if(m>max) max=m }
    END { if(n==0){print "0 0"} else {printf "%.1f %.1f\n", sum/n, max} }')
  echo "cpu_percent avg=${cpu_avg}% peak=${cpu_peak}%"
  echo "memory_mib  avg=${mem_avg} peak=${mem_peak}"
fi

echo "files=$RUN_DIR"
