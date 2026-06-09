#!/usr/bin/env bash
set -euo pipefail

CONTAINER="${1:?container name required}"
OUTPUT="${2:?output file required}"
INTERVAL="${STATS_INTERVAL:-2}"

mkdir -p "$(dirname "$OUTPUT")"
echo "timestamp,cpu_perc,mem_usage,mem_limit,mem_perc" > "$OUTPUT"

while true; do
  line="$(docker stats "$CONTAINER" --no-stream --format "{{.CPUPerc}},{{.MemUsage}},{{.MemPerc}}" 2>/dev/null || true)"
  if [[ -n "$line" ]]; then
    ts="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
    echo "${ts},${line}" >> "$OUTPUT"
  fi
  sleep "$INTERVAL"
done
