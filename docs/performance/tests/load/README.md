# MarretaLang — Load Testing

Reproducible load test for the ecommerce example using k6 and Docker.

---

## Prerequisites

- Docker with Compose plugin
- `jq` (optional — for formatted summary in the terminal)

---

## Running

```bash
./tests/load/run.sh
```

That's it. The script handles everything:

1. Builds the marreta Docker image from local source (`--release`)
2. Starts the ecommerce server and waits for it to be healthy
3. Saves a `config.json` snapshot with all test parameters
4. Starts collecting `docker stats` every 2s in the background
5. Runs k6 with the 3 scenarios
6. Tears everything down
7. Prints a summary in the terminal

Total duration: ~11 minutes. Output lands in a timestamped folder:

```
tests/load/results/
└── 20260327T150853Z/
    ├── config.json       # exact parameters used in this run
    ├── k6_summary.json   # aggregated metrics (latency, throughput, errors)
    └── stats.csv         # docker stats sampled every 2s (CPU%, memory)
```

---

## What it tests

Three scenarios run sequentially (~3m30s each):

| Scenario | Route | What it exercises |
|---|---|---|
| `health` | `GET /health` | Baseline — no schema, no task |
| `products` | `POST /products` | Flat schema validation + response serializer |
| `orders` | `POST /orders` | Nested schema + typed list + task contract + response serializer |

Load profile per scenario:

```
ramp-up   → 0 to 50 VUs over 30s
sustained → 50 VUs for 2 minutes
stress    → 50 to 200 VUs over 30s
ramp-down → 200 to 0 VUs over 30s
```

~10% of `/orders` requests intentionally omit `billing.city` to exercise the 422 validation path under load.

---

## Extracting metrics for comparison

After each run, use these commands to pull the canonical numbers.

### Latency — from `k6_summary.json`

```bash
RUN="tests/load/results/<timestamp>"

for scenario in health products orders; do
  jq -r --arg s "$scenario" '
    .metrics["http_req_duration{scenario:\($s)}"] as $m |
    "\($s): min=\($m.min)ms avg=\($m.avg)ms med=\($m.med)ms p90=\($m["p(90)"])ms p95=\($m["p(95)"])ms p99=\($m["p(99)"])ms max=\($m.max)ms"
  ' "$RUN/k6_summary.json"
done
```

### Throughput — from `k6_summary.json`

```bash
jq '.metrics.http_reqs | "total=\(.count) rate=\(.rate | floor) req/s"' "$RUN/k6_summary.json"
```

### Error rates — from `k6_summary.json`

```bash
for scenario in health products orders; do
  jq -r --arg s "$scenario" '
    "\($s): \(.metrics["http_req_failed{scenario:\($s)}"].value * 100 | . * 100 | round / 100)%"
  ' "$RUN/k6_summary.json"
done
```

### Resource usage — from `stats.csv`

```bash
# Peak CPU
tail -n +2 "$RUN/stats.csv" | awk -F',' '{gsub(/%/,"",$2); if($2+0>max) max=$2+0} END{print "Peak CPU: " max"%"}'

# Peak Memory
tail -n +2 "$RUN/stats.csv" | awk -F',' '{mem=$3; gsub(/MiB.*/,"",mem); gsub(/ /,"",mem); if(mem+0>max) max=mem+0} END{print "Peak Mem: " max"MiB"}'
```

---

## Comparing runs apples-to-apples

For results to be comparable across versions, keep these parameters identical:

| Parameter | Canonical value | Where to change |
|---|---|---|
| Sleep between requests | **10ms** (`sleep(0.01)`) | `k6/scenarios.js` |
| VU ramp profile | 0→50 (30s) → 50 (2m) → 50→200 (30s) → 0 (30s) | `k6/scenarios.js` — `stages` |
| Binary | always `--release` | Dockerfile |

The `config.json` inside each run folder records the exact parameters used — check it if results look different from expected.

---

## Baseline reference

See [`docs/performance/BASELINE_040.md`](../../docs/performance/BASELINE_040.md) for the v0.4.0 official baseline numbers.
When a new version is released, create `docs/performance/BASELINE_<version>.md` in the same format.
