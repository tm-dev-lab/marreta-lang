# MarretaLang — Post-046 Load Test Regression Check

This document records the ecommerce load tests executed after the post-046
runtime/tooling work. The goal was to check whether features delivered after
the previous performance report introduced measurable overhead in the main
runtime path.

This is a regression signal, not a new official baseline. Two runs were
executed: one with unrelated containers still running on the host, and one
after cleaning those containers.

## Run

| Field | Value |
|---|---|
| First run ID | `20260519T213256Z` |
| Clean run ID | `20260519T214850Z` |
| Branch | `main` |
| Source snapshot | `8a68fc2` |
| Binary | release build inside Docker |
| Command | `./tests/load/run.sh` |
| Scenario script | `tests/load/k6/scenarios.js` |
| Thresholds script | `tests/load/k6/thresholds.js` |
| Host CPU | 11th Gen Intel Core i7-11800H @ 2.30GHz, 16 cores |
| Host RAM | 15 GiB |
| Docker | 28.2.2 |
| k6 image | `grafana/k6:latest` |

## Load Profile

| Parameter | Value |
|---|---|
| Sleep per iteration | 10ms |
| VU profile | 0->50 (30s) -> 50 (2m) -> 50->200 (30s) -> 0 (30s) |
| Scenarios | `health`, `products`, `orders` |
| Duration | 11m30s |
| Error injection | ~10% of `orders` requests intentionally send invalid payloads and expect 422 |

## Results

All k6 thresholds passed in both runs.

### First Run: `20260519T213256Z`

This run was executed while unrelated containers from previous manual checks
were still running.

| Scenario | min | avg | med | p90 | p95 | p99 | max |
|---|---:|---:|---:|---:|---:|---:|---:|
| `health` | 0.054ms | 0.675ms | 0.551ms | 1.227ms | 1.527ms | 2.597ms | 21.907ms |
| `products` | 1.025ms | 3.348ms | 3.024ms | 4.387ms | 5.102ms | 9.123ms | 186.361ms |
| `orders` | 0.134ms | 3.926ms | 3.452ms | 5.905ms | 7.877ms | 18.002ms | 216.371ms |

| Metric | Value |
|---|---:|
| Total requests | 3,118,949 |
| Average throughput | 4,520 req/s |
| `health` error rate | 0.00% |
| `products` error rate | 0.00% |
| `orders` error rate | 10.02% |
| Peak CPU | 694.83% |
| Peak memory | 93.15 MiB |

### Clean Run: `20260519T214850Z`

Before this run, all unrelated containers were stopped. The load-test script
then started only its own ecommerce, PostgreSQL, MongoDB, and k6 workload.

| Scenario | min | avg | med | p90 | p95 | p99 | max |
|---|---:|---:|---:|---:|---:|---:|---:|
| `health` | 0.057ms | 0.736ms | 0.605ms | 1.305ms | 1.605ms | 2.772ms | 16.807ms |
| `products` | 0.860ms | 2.767ms | 2.583ms | 3.726ms | 4.196ms | 6.599ms | 146.456ms |
| `orders` | 0.134ms | 3.891ms | 3.295ms | 5.952ms | 7.582ms | 20.475ms | 142.378ms |

| Metric | Value |
|---|---:|
| Total requests | 3,164,557 |
| Average throughput | 4,586 req/s |
| `health` error rate | 0.00% |
| `products` error rate | 0.00% |
| `orders` error rate | 9.98% |
| Peak CPU | 696.67% |
| Peak memory | 97.92 MiB |

## Thresholds

All configured k6 thresholds passed in the clean run.

| Threshold | Limit | Clean run result |
|---|---:|---:|
| `health` p95 | < 200ms | 1.605ms |
| `products` p95 | < 300ms | 4.196ms |
| `orders` p95 | < 400ms | 7.582ms |
| `health` error rate | < 1% | 0.00% |
| `products` error rate | < 1% | 0.00% |
| `orders` error rate | < 15% | 9.98% |

## Comparison Between Today's Runs

| Metric | First run | Clean run | Delta |
|---|---:|---:|---:|
| Total requests | 3,118,949 | 3,164,557 | +1.46% |
| Average throughput | 4,520 req/s | 4,586 req/s | +1.46% |
| `health` p95 | 1.527ms | 1.605ms | +5.1% |
| `products` p95 | 5.102ms | 4.196ms | -17.8% |
| `orders` p95 | 7.877ms | 7.582ms | -3.8% |
| Peak CPU | 694.83% | 696.67% | +0.3% |
| Peak memory | 93.15 MiB | 97.92 MiB | +5.1% |

Cleaning unrelated containers improved the write-heavy `products` path and
slightly improved `orders`, but did not materially change CPU usage or the
overall regression signal. Host contention was not the primary cause.

## Comparison With Previous Modern Run

**Previous run**: `20260417T144602Z`  
**Previous report**: `docs/performance/LOAD_TEST_TRACE_022B_20260417.md`

Both runs used the same load-test orchestrator (`./tests/load/run.sh`), the
same k6 script, and the same high-level scenario profile. The comparison is
useful as a regression signal, but not a perfectly controlled benchmark: the
runtime and ecommerce example evolved between the runs.

### Topline Delta

| Metric | 20260417 run | 20260519 clean run | Delta |
|---|---:|---:|---:|
| Total requests | 3,316,314 | 3,164,557 | -4.58% |
| Average throughput | 4,806 req/s | 4,586 req/s | -4.58% |
| Peak CPU | 588.55% | 696.67% | +18.4% |
| Peak memory | 84.12 MiB | 97.92 MiB | +15.7% |
| `health` error rate | 0.00% | 0.00% | stable |
| `products` error rate | 0.00% | 0.00% | stable |
| `orders` error rate | 10.02% | 9.98% | stable, intentional 422s |

### Latency Delta

| Scenario | 20260417 p95 | 20260519 clean p95 | Delta | Reading |
|---|---:|---:|---:|---|
| `health` | 0.843ms | 1.605ms | +90.4% | Hot-path overhead increased, but remains low |
| `products` | 3.297ms | 4.196ms | +27.3% | Moderate regression |
| `orders` | 4.998ms | 7.582ms | +51.7% | Moderate regression on the heavier path |

## Interpretation

The result is not a release blocker by itself. The service completed the full
load test, all k6 thresholds passed with large margins, checks were 100%
successful, and the only errors were the intentionally injected 422 responses
in the `orders` scenario.

However, the clean run confirms a measurable regression against the previous
modern run:

- throughput is down by roughly 4.6%;
- peak CPU is up by roughly 18.4%;
- peak memory is up by roughly 15.7%;
- p95 latency increased in all three scenarios.

The strongest signal is the `health` route. Because it does not exercise DB
I/O, schema-heavy request bodies, or document persistence, its p95 increase
points to overhead in the common request/runtime path rather than only in
business logic or database work.

Recent features unlikely to affect this workload directly include OpenAPI docs,
formatter, and linter work: the load script does not call `/docs`, `marreta fmt`,
or `marreta lint`. More plausible areas to inspect are runtime hot-path changes
introduced after the previous report, such as validation/coercion behavior,
`Value` handling, contract types (`enum`/`decimal`), error/trace/log context,
or any request pipeline work that now runs for every request.

## Recommendation

Treat this as a regression signal that should be tracked, not as proof of a
specific bottleneck.

Recommended next steps:

1. Keep `20260519T214850Z` as the clean comparison artifact.
2. Run a targeted profiler or lightweight timing probes on the common request
   path, starting with `health`.
3. Compare commits between `891c07b` and `8a68fc2` for runtime changes that
   execute on every request.
4. If the overhead is not obvious, create a reduced load scenario with only
   `GET /health` to isolate router/runtime overhead from DB and schema work.
5. Do not update the official baseline until the cause is understood or the
   regression is consciously accepted.

## Artifacts

```text
tests/load/results/20260519T213256Z/config.json
tests/load/results/20260519T213256Z/k6_summary.json
tests/load/results/20260519T213256Z/stats.csv

tests/load/results/20260519T214850Z/config.json
tests/load/results/20260519T214850Z/k6_summary.json
tests/load/results/20260519T214850Z/stats.csv
```
