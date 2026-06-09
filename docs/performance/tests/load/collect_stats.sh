#!/usr/bin/env bash
# Collects docker stats for the marreta-ecommerce container every 2 seconds.
# Usage: ./collect_stats.sh <output_file>
# Run in background; kill when the test is done.

OUTPUT="${1:-results/stats.csv}"
CONTAINER="marreta-ecommerce"

mkdir -p "$(dirname "$OUTPUT")"

# Header
echo "timestamp,cpu_perc,mem_usage,mem_limit,mem_perc" > "$OUTPUT"

while true; do
  LINE=$(docker stats "$CONTAINER" --no-stream --format \
    "{{.CPUPerc}},{{.MemUsage}},{{.MemPerc}}" 2>/dev/null)
  if [ -n "$LINE" ]; then
    # MemUsage is "123MiB / 7.77GiB" — keep as-is, quoted
    TS=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
    echo "${TS},${LINE}" >> "$OUTPUT"
  fi
  sleep 2
done
