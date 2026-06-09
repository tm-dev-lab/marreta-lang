# MarretaLang — 022b Trace Performance Validation

This document records the ecommerce load test used to validate
`docs/spec/022b_TRACE_PERF_AND_ERGONOMICS.md`.

## Run

| Field | Value |
|---|---|
| Run ID | `20260417T144602Z` |
| Branch | `feature/trace-perf-ergonomics-022b` |
| Source snapshot | `891c07b` plus the documentation comparison update |
| Binary | release build inside Docker |
| Command | `./tests/load/run.sh` |
| Scenario script | `tests/load/k6/scenarios.js` |
| Thresholds script | `tests/load/k6/thresholds.js` |
| Host CPU | 11th Gen Intel Core i7-11800H @ 2.30GHz, 16 cores |
| Host RAM | 15 GiB |
| Docker | 28.2.2 |
| k6 image | `grafana/k6:latest` |

The first attempt exposed a load-test container issue unrelated to trace
runtime behavior: the builder image produced a binary requiring `GLIBC_2.38`
while the runtime image used `debian:bookworm-slim`. The runtime image was
aligned to `debian:trixie-slim`, matching the working example Dockerfiles, and
the test then completed successfully.

## Load Profile

| Parameter | Value |
|---|---|
| Sleep per iteration | 10ms |
| VU profile | 0->50 (30s) -> 50 (2m) -> 50->200 (30s) -> 0 (30s) |
| Scenarios | `health`, `products`, `orders` |
| Duration | 11m30s |
| Error injection | 10% of `orders` requests intentionally send invalid payloads and expect 422 |

## Results

All k6 thresholds passed.

| Scenario | min | avg | med | p90 | p95 | p99 | max |
|---|---:|---:|---:|---:|---:|---:|---:|
| `health` | 0.032ms | 0.322ms | 0.231ms | 0.656ms | 0.843ms | 1.317ms | 9.051ms |
| `products` | 0.775ms | 2.348ms | 2.030ms | 2.851ms | 3.297ms | 13.767ms | 230.057ms |
| `orders` | 0.072ms | 2.776ms | 1.985ms | 3.689ms | 4.998ms | 21.433ms | 195.386ms |

| Metric | Value |
|---|---:|
| Total requests | 3,316,314 |
| Average throughput | 4,806 req/s |
| `health` error rate | 0.00% |
| `products` error rate | 0.00% |
| `orders` error rate | 10.02% |
| Peak CPU | 588.55% |
| Peak memory | 84.12 MiB |

## Thresholds

| Threshold | Limit | Result |
|---|---:|---:|
| `health` p95 | < 200ms | 0.843ms |
| `products` p95 | < 300ms | 3.297ms |
| `orders` p95 | < 400ms | 4.998ms |
| `health` error rate | < 1% | 0.00% |
| `products` error rate | < 1% | 0.00% |
| `orders` error rate | < 15% | 10.02% |

## Interpretation

The 022b trace changes did not introduce obvious pathological overhead under
the ecommerce workload. The success path remains silent, the service completed
3.3M requests without unexpected failures, and memory remained bounded at a
peak of 84.12 MiB during the 200 VU stress phase.

The run is not an apples-to-apples comparison against the older v0.4.0 baseline
because the ecommerce example now includes database and document providers.
For 022b, the relevant signal is regression safety: thresholds passed with
large margin, no trace output appeared on successful requests, and resource
usage stayed bounded.

## Comparison With Previous DB Load Run

**Previous run**: `20260330T235451Z`  
**Previous report**: `docs/performance/LOAD_TEST_DB_20260330_2112.md`

Both runs used the same load-test orchestrator (`./tests/load/run.sh`) and the
same high-level k6 profile: 10ms sleep, 0->50->200 VUs, and the three ecommerce
scenarios (`health`, `products`, `orders`). The comparison is useful as a
regression signal, but not a perfectly controlled apples-to-apples benchmark:
the ecommerce example and runtime configuration evolved between the DB run and
the 022b run.

### Topline Delta

| Metric | Previous DB run | 022b run | Delta |
|---|---:|---:|---:|
| Total requests | 2,787,305 | 3,316,314 | +19.0% |
| Average throughput | 4,039 req/s | 4,806 req/s | +19.0% |
| Peak CPU | 217.89% | 588.55% | +170.1% |
| Peak memory | 61.34 MiB | 84.12 MiB | +37.1% |
| `health` error rate | 0.00% | 0.00% | stable |
| `products` error rate | 0.00% | 0.00% | stable |
| `orders` error rate | 10.01% | 10.02% | stable, intentional 422s |

### Latency Delta

| Scenario | Previous DB p95 | 022b p95 | Delta | Reading |
|---|---:|---:|---:|---|
| `health` | 0.809ms | 0.843ms | +4.2% | Stable; still sub-millisecond p95 |
| `products` | 36.933ms | 3.297ms | -91.1% | Major improvement in observed write-path latency |
| `orders` | 21.080ms | 4.998ms | -76.3% | Major improvement; still includes the intentional fast 422 path |

### Updated Reading

The previous DB run showed a large p95 penalty on write-heavy routes,
especially `products` at 36.933ms. This 022b run keeps the same ecommerce
tiers below 5ms p95 while increasing throughput from 4,039 req/s to
4,806 req/s.

This reduces the concern that Marreta's interpreter or trace bookkeeping is a
success-path bottleneck. The 022b trace implementation stayed silent on
successful requests, passed all load thresholds with significant headroom, and
did not show obvious unbounded trace-memory growth.

The resource profile did increase:

- CPU peak rose from 217.89% to 588.55%, meaning the current run used more
  parallel CPU capacity during the stress phase.
- Memory peak rose from 61.34 MiB to 84.12 MiB, but remained bounded during the
  run.
- The previous memory-bloat concern should remain tracked, but this run did
  not show an obvious runaway leak pattern.

### Follow-up Recommendation

To isolate whether the latency improvement came from runtime changes, Docker
image changes, pool behavior, or ecommerce changes, run a controlled DB-only
comparison with:

1. the same Docker runtime image,
2. the same Postgres image and pool settings,
3. doc/cache/queue disabled,
4. one run before 022b and one run after 022b,
5. memory sampling kept for at least 10 minutes after k6 finishes to distinguish
   a stable pool plateau from an actual leak.

Until that controlled run exists, this comparison should be treated as a
positive regression signal for 022b, not as proof that a specific DB bottleneck
was fixed.

## Artifacts

```text
tests/load/results/20260417T144602Z/config.json
tests/load/results/20260417T144602Z/k6_summary.json
tests/load/results/20260417T144602Z/stats.csv
```
