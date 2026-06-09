#!/usr/bin/env bash
# Collects docker stats for a named container every 2 seconds.
# Usage: ./collect_stats_doc.sh <container_name> <output_file>

CONTAINER="${1:-marreta-ecommerce}"
OUTPUT="${2:-results/stats.csv}"

mkdir -p "$(dirname "$OUTPUT")"
echo "timestamp,cpu_perc,mem_usage,mem_limit,mem_perc" > "$OUTPUT"

while true; do
  LINE=$(docker stats "$CONTAINER" --no-stream --format \
    "{{.CPUPerc}},{{.MemUsage}},{{.MemPerc}}" 2>/dev/null)
  if [ -n "$LINE" ]; then
    TS=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
    echo "${TS},${LINE}" >> "$OUTPUT"
  fi
  sleep 2
done
