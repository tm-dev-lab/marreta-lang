#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BENCH_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
export RUN_TS="${RUN_TS:-$(date -u +"%Y%m%dT%H%M%SZ")}"

failed=0

for target in marreta fastapi nest; do
  if "$SCRIPT_DIR/run_one.sh" "$target"; then
    :
  else
    status=$?
    echo "target '$target' failed with exit code $status" >&2
    failed=1
  fi
  echo ""
done

# Tear down the shared MongoDB (and its data) once all targets have run.
(cd "$BENCH_DIR" && docker compose down -v >/dev/null 2>&1 || true)

echo "All benchmark outputs:"
echo "$BENCH_DIR/results/$RUN_TS"

exit "$failed"
