# MarretaLang — Performance Baseline v0.4.0

This document records the official performance baseline for v0.4.0. Its purpose is to serve
as a reference point for future runs — any version bump should produce a new baseline doc
in the same format so results can be compared apples-to-apples.

---

## How to run

```bash
./tests/load/run.sh
```

Handles everything: build → start → config snapshot → collect stats → k6 → teardown → summary.
Total duration: ~11 minutes. Output lands in a timestamped folder:

```
tests/load/results/
└── 20260327T150853Z/
    ├── config.json       # parameters used in this run
    ├── k6_summary.json   # aggregated metrics — use this for comparisons
    └── stats.csv         # docker stats sampled every 2s (CPU, memory)
```

---

## How to extract comparable metrics

### From `k6_summary.json`

```bash
RUN="tests/load/results/<timestamp>"

# Latency per scenario
jq -r '
  .metrics["http_req_duration{scenario:health}",
           "http_req_duration{scenario:products}",
           "http_req_duration{scenario:orders}"]
  | to_entries[]
  | "\(.key): min=\(.value.min)ms avg=\(.value.avg)ms med=\(.value.med)ms p90=\(.value["p(90)"])ms p95=\(.value["p(95)"])ms p99=\(.value["p(99)"])ms max=\(.value.max)ms"
' "$RUN/k6_summary.json"

# Throughput
jq '.metrics.http_reqs | {total: .count, rate: .rate}' "$RUN/k6_summary.json"

# Error rates
jq '{
  health:   .metrics["http_req_failed{scenario:health}"].value,
  products: .metrics["http_req_failed{scenario:products}"].value,
  orders:   .metrics["http_req_failed{scenario:orders}"].value
}' "$RUN/k6_summary.json"
```

### From `stats.csv`

```bash
RUN="tests/load/results/<timestamp>"

# Peak CPU
tail -n +2 "$RUN/stats.csv" | awk -F',' '{gsub(/%/,"",$2); if($2+0>max) max=$2+0} END{print max"%"}'

# Peak Memory (MiB)
tail -n +2 "$RUN/stats.csv" | awk -F',' '{mem=$3; gsub(/MiB.*/,"",mem); gsub(/ /,"",mem); if(mem+0>max) max=mem+0} END{print max"MiB"}'
```

---

## Variables that affect results — always record

| Variable | Where | Canonical value |
|---|---|---|
| Sleep between requests | `tests/load/k6/scenarios.js` — `sleep(N)` | **0.01s (10ms)** |
| VU ramp profile | `tests/load/k6/scenarios.js` — `stages` | 0→50 (30s) → 50 (2m) → 50→200 (30s) → 0 (30s) |
| Binary build | always `--release` | ✓ |
| Host | document CPU model, cores, RAM | i7-11800H, 16 cores, 15 GiB |

> Use `sleep(0.01)` as the canonical value for all future comparisons.
> The `config.json` inside each run folder records the exact parameters automatically.

---

## Environment

| Field | Value |
|---|---|
| MarretaLang version | v0.4.0 (commit `68725ef`) |
| Binary | `--release` build (optimized) |
| Host CPU | 11th Gen Intel Core i7-11800H @ 2.30GHz (16 cores) |
| Host RAM | 15 GiB |
| Docker version | 28.2.2 |
| k6 image | grafana/k6:latest |
| Date | 2026-03-27 |

---

## Official Baseline — Run 20260327T150853Z

> This is the canonical run. Use it as the reference for all future version comparisons.

### Setup

| Parameter | Value |
|---|---|
| Sleep per iteration | 10ms |
| VU profile | 0→50 (30s) → 50 (2m) → 50→200 (30s) → 0 (30s) |
| Duration | 11m30s |
| Total requests | 3,670,453 |

### Latency (ms)

| Scenario | Route | min | avg | med (p50) | p90 | p95 | p99 | max |
|---|---|---|---|---|---|---|---|---|
| `health` | `GET /health` | 0.039 | 0.339 | 0.263 | 0.647 | 0.852 | 1.347 | 9.65 |
| `products` | `POST /products` | 0.046 | 0.391 | 0.318 | 0.728 | 0.938 | 1.411 | 7.83 |
| `orders` | `POST /orders` | 0.056 | 0.479 | 0.415 | 0.826 | 1.033 | 1.558 | 7.93 |

### Throughput & error rates

| Metric | Value |
|---|---|
| Average req/s | **5,319 req/s** |
| `health` error rate | 0.00% |
| `products` error rate | 0.00% |
| `orders` error rate | 10.00% (all intentional 422s — zero unexpected errors) |
| Checks passed | 100% |

### Thresholds (all passed ✓)

| Threshold | Limit | Result |
|---|---|---|
| `health` p95 | < 200ms | 0.852ms ✓ |
| `products` p95 | < 300ms | 0.938ms ✓ |
| `orders` p95 | < 400ms | 1.033ms ✓ |
| `health` error rate | < 1% | 0.00% ✓ |
| `products` error rate | < 1% | 0.00% ✓ |
| `orders` error rate | < 15% | 10.00% ✓ |

### Resource usage

| Metric | Value |
|---|---|
| Peak CPU | 160.82% (~1.6 cores) |
| Peak Memory | 19.5 MiB |
| Memory trend | Flat — no leak |

### Notes

- All latency numbers sub-millisecond at p95 across all three complexity tiers.
- Memory stable at ~19 MiB under 200 VUs — per-request isolation working correctly.
- CPU at ~1.6 cores under full load — tokio distributing across cores, headroom available.
- Delta `health` → `orders` at p95: **181µs** — total cost of nested schema validation + task contract + response serializer per request.
- Zero unexpected errors across 3.67M requests.
