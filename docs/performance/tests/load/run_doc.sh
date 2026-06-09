#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
TS=$(date -u +"%Y%m%dT%H%M%SZ")
RUN_DIR="$SCRIPT_DIR/results/$TS"
COMPOSE_FILE="$ROOT_DIR/examples/ecommerce/docker-compose.yml"
CONTAINER_NAME=$(docker compose -f "$COMPOSE_FILE" ps -q marreta 2>/dev/null | head -1)

mkdir -p "$RUN_DIR"
chmod 777 "$RUN_DIR"

echo "=== MarretaLang Load Test — doc.* (MongoDB) ==="
echo "Timestamp : $TS"
echo "Output    : $RUN_DIR"
echo ""

# --- 1. Build Docker image ---
echo "[1/7] Building Docker image..."
docker compose -f "$COMPOSE_FILE" build --quiet

# --- 2. Start services ---
echo "[2/7] Starting MongoDB + marreta..."
docker compose -f "$COMPOSE_FILE" up -d

# --- 3. Wait for healthcheck ---
echo "[3/7] Waiting for server to be healthy..."
ATTEMPTS=0
until [ "$(docker inspect --format='{{.State.Health.Status}}' "$CONTAINER_NAME" 2>/dev/null)" = "healthy" ]; do
  ATTEMPTS=$((ATTEMPTS + 1))
  if [ "$ATTEMPTS" -ge 30 ]; then
    echo "ERROR: Server did not become healthy after 90s."
    docker compose -f "$COMPOSE_FILE" logs
    docker compose -f "$COMPOSE_FILE" down
    exit 1
  fi
  sleep 3
done
echo "Server healthy."

# --- 4. Snapshot test configuration ---
MARRETA_VERSION=$(cd "$ROOT_DIR" && git describe --tags --always 2>/dev/null || git rev-parse --short HEAD)
SLEEP_VALUE=$(grep -oP 'sleep\(\K[0-9.]+' "$SCRIPT_DIR/k6/scenarios_doc.js" | head -1)
cat > "$RUN_DIR/config.json" <<EOF
{
  "timestamp": "$TS",
  "marreta_version": "$MARRETA_VERSION",
  "binary": "release",
  "sleep_seconds": $SLEEP_VALUE,
  "vu_stages": "0→50 (30s) → 50 (2m) → 50→200 (30s) → 0 (30s)",
  "scenarios": ["health", "products", "orders"],
  "db_provider": "mongodb",
  "error_injection_rate": 0.10,
  "stats_interval_seconds": 2,
  "host": {
    "cpu": "$(grep 'model name' /proc/cpuinfo | head -1 | cut -d: -f2 | xargs)",
    "cores": $(nproc),
    "ram_gb": $(awk '/MemTotal/{printf "%.0f", $2/1024/1024}' /proc/meminfo)
  },
  "docker_version": "$(docker --version | grep -oP '[\d.]+'| head -1)",
  "k6_image": "grafana/k6:latest",
  "scenarios_script": "tests/load/k6/scenarios_doc.js",
  "thresholds_script": "tests/load/k6/thresholds_doc.js"
}
EOF
echo "[4/7] Config saved → $RUN_DIR/config.json"

# --- 5. Start stats collector ---
echo "[5/7] Starting stats collector..."
bash "$SCRIPT_DIR/collect_stats_doc.sh" "$CONTAINER_NAME" "$RUN_DIR/stats.csv" &
STATS_PID=$!
cleanup() {
  kill "$STATS_PID" 2>/dev/null || true
  docker compose -f "$COMPOSE_FILE" down 2>/dev/null || true
}
trap cleanup EXIT

# --- 6. Run k6 ---
echo "[6/7] Running k6 scenarios (~11 minutes)..."
NETWORK=$(docker inspect "$CONTAINER_NAME" \
  --format='{{range .NetworkSettings.Networks}}{{.NetworkID}}{{end}}' | head -1)

docker run --rm \
  --network "$NETWORK" \
  -v "$SCRIPT_DIR/k6:/scripts" \
  -v "$RUN_DIR:/results" \
  -e BASE_URL="http://$CONTAINER_NAME:8080" \
  grafana/k6:latest run \
    --summary-export /results/k6_summary.json \
    /scripts/scenarios_doc.js

# --- 7. Print summary ---
kill "$STATS_PID" 2>/dev/null || true
echo ""
echo "[7/7] Summary"
echo "============================================================"

if command -v jq &>/dev/null && [ -f "$RUN_DIR/k6_summary.json" ]; then
  echo ""
  echo "--- Latency per scenario (ms) ---"
  for scenario in health products orders; do
    jq -r --arg s "$scenario" '
      .metrics["http_req_duration{scenario:\($s)}"] as $m |
      "  \($s): min=\($m.min | . * 1000 | round / 1000)ms avg=\($m.avg | . * 1000 | round / 1000)ms med=\($m.med | . * 1000 | round / 1000)ms p90=\($m["p(90)"] | . * 1000 | round / 1000)ms p95=\($m["p(95)"] | . * 1000 | round / 1000)ms p99=\($m["p(99)"] | . * 1000 | round / 1000)ms max=\($m.max | . * 1000 | round / 1000)ms"
    ' "$RUN_DIR/k6_summary.json" 2>/dev/null || true
  done

  echo ""
  echo "--- Throughput ---"
  jq -r '.metrics.http_reqs | "  total=\(.count) rate=\(.rate | floor) req/s"' \
    "$RUN_DIR/k6_summary.json" 2>/dev/null || true

  echo ""
  echo "--- Error rates ---"
  for scenario in health products orders; do
    jq -r --arg s "$scenario" '
      .metrics["http_req_failed{scenario:\($s)}"].value as $rate |
      "  \($s): \($rate * 100 | . * 100 | round / 100)%"
    ' "$RUN_DIR/k6_summary.json" 2>/dev/null || true
  done
fi

echo ""
echo "--- Peak resource usage ---"
if [ -f "$RUN_DIR/stats.csv" ]; then
  PEAK_CPU=$(tail -n +2 "$RUN_DIR/stats.csv" | awk -F',' '{gsub(/%/,"",$2); if($2+0>max) max=$2+0} END{print max"%"}')
  PEAK_MEM=$(tail -n +2 "$RUN_DIR/stats.csv" | awk -F',' '{mem=$3; gsub(/MiB.*/,"",mem); gsub(/ /,"",mem); if(mem+0>max) max=mem+0} END{print max"MiB"}')
  echo "  Peak CPU    : $PEAK_CPU"
  echo "  Peak Memory : $PEAK_MEM"
fi

echo ""
echo "Output files:"
echo "  $RUN_DIR/config.json       — test parameters snapshot"
echo "  $RUN_DIR/k6_summary.json   — aggregated metrics (latency, throughput, errors)"
echo "  $RUN_DIR/stats.csv         — docker stats sampled every 2s (CPU, memory)"
echo "============================================================"
