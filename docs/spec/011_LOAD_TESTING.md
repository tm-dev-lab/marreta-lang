# MarretaLang — Load Testing & Performance Baseline

> Status: Delivered.

> **Meta:** Establish a reproducible load testing setup for MarretaLang using the ecommerce
> example as the target workload. Measure throughput, latency distribution, and resource
> consumption under realistic and stress conditions — producing a performance baseline for
> the current interpreter before any optimization work begins.

---

## 1. Motivation

MarretaLang's runtime is written in Rust, but the interpreter is tree-walking and dynamically
typed. Before optimizing anything, we need hard numbers:

- What is the steady-state throughput (req/s) for each route complexity tier?
- Where does latency degrade (p95, p99) as concurrency increases?
- How does memory and CPU behave over time — does anything grow unbounded?
- Which feature has the highest per-request cost: schema validation, task contracts, response serialization?

The ecommerce example is the ideal target: it contains three distinct complexity tiers
(`GET /health`, `POST /products`, `POST /orders`) in a single realistic multi-file project.

---

## 2. Architecture

```
tests/load/
├── Dockerfile                  # multi-stage: cargo build → minimal runtime image
├── docker-compose.yml          # marreta service + k6 service
├── k6/
│   ├── scenarios.js            # main k6 script — 3 scenarios, 4 load stages
│   ├── thresholds.js           # pass/fail criteria (imported by scenarios.js)
│   └── payloads/
│       ├── order_valid.json     # POST /orders — valid nested payload
│       ├── order_missing.json   # POST /orders — triggers 422 (stress validation path)
│       └── product_valid.json   # POST /products — flat schema payload
├── collect_stats.sh            # docker stats sampler (2s interval → stats.csv)
└── run.sh                      # orchestrates: build → start → collect → k6 → teardown
```

---

## 3. Load Scenarios

### 3.1 Complexity Tiers

| Tier | Route | Features exercised |
|---|---|---|
| Baseline | `GET /health` | No schema, no task, pure reply |
| Medium | `POST /products` | Flat schema validation, response serializer |
| Heavy | `POST /orders` | Nested schema + typed list + task contract + response serializer |

Running all three tiers in the same test reveals the cost of each additional feature layer.

### 3.2 Load Stages

```
Stage 1 — Ramp-up:   0 → 50 VUs over 30s   (warm up the interpreter and OS network stack)
Stage 2 — Sustained: 50 VUs for 2 minutes   (steady-state baseline)
Stage 3 — Stress:    50 → 200 VUs over 30s  (find the degradation point)
Stage 4 — Ramp-down: 200 → 0 VUs over 30s  (observe recovery behavior)
```

Total test duration: ~3m30s per scenario. All three scenarios run sequentially to avoid
interference; the full suite takes ~11 minutes.

### 3.3 Error Injection

10% of `POST /orders` requests use `order_missing.json` (missing `billing.city`) to exercise
the 422 validation path under load — this is a real workload pattern and stresses the
recursive validator separately from the happy path.

---

## 4. Metrics

### 4.1 k6 Metrics (per scenario)

| Metric | Description |
|---|---|
| `http_req_duration` p50/p95/p99 | Latency distribution |
| `http_reqs` rate | Throughput (req/s) |
| `http_req_failed` rate | Error rate (non-2xx/expected-4xx) |
| `http_req_waiting` | Time-to-first-byte (server processing time) |
| `vus` over time | Actual concurrency profile |

### 4.2 Docker Stats (sampled every 2s)

| Metric | Description |
|---|---|
| `CPUPerc` | CPU usage % of the marreta process |
| `MemUsage` | Absolute memory (RSS) |
| `MemPerc` | Memory % of host |

Collected to `results/stats_<timestamp>.csv` for later correlation with k6 timeline.

### 4.3 Output Files

```
results/
├── k6_health_<timestamp>.json      # k6 JSON summary for /health scenario
├── k6_products_<timestamp>.json    # k6 JSON summary for /products scenario
├── k6_orders_<timestamp>.json      # k6 JSON summary for /orders scenario
├── stats_<timestamp>.csv           # docker stats time series
└── summary_<timestamp>.txt         # human-readable summary printed by run.sh
```

---

## 5. Implementation Phases

### Phase 1 — Dockerfile & Docker Compose

Build a minimal production image for the marreta binary:

- Multi-stage build: `rust:slim` for `cargo build --release` → `debian:slim` for runtime
- Image only contains the `marreta` binary + the `examples/ecommerce/` directory
- Exposes port `3000`
- `docker-compose.yml` defines:
  - `marreta` service: built from local source, port `3000:3000`, healthcheck on `GET /health`
  - `k6` service: `grafana/k6` official image, mounts `tests/load/k6/` as volume
- No external dependencies (no database, no cache — ecommerce example is self-contained)

**Deliverable:** `docker compose up` starts the ecommerce server and k6 can reach it at `http://marreta:3000`.

### Phase 2 — k6 Scripts

Write `scenarios.js` with the three-tier test:

- Parameterized base URL (`__ENV.BASE_URL`, default `http://marreta:3000`)
- Each scenario defined as a k6 `scenario` with its own executor and stage profile
- Payload files loaded via `open()` from `payloads/`
- 10% error injection on orders via `Math.random()`
- `thresholds.js` defines initial pass/fail gates:
  - `http_req_duration['p(95)'] < 200` (200ms p95 is a reasonable starting bar — adjust after first run)
  - `http_req_failed rate < 0.01` (excluding expected 422s)

**Deliverable:** `k6 run k6/scenarios.js` produces a full summary with all three scenarios.

### Phase 3 — Docker Stats Collector

Write `collect_stats.sh`:

- Loops `docker stats marreta --no-stream` every 2 seconds
- Appends ISO timestamp + CPU% + MemUsage + MemPerc to `results/stats_<timestamp>.csv`
- Runs in background; killed by `run.sh` after k6 finishes
- Header row written on first sample for easy CSV parsing

**Deliverable:** `stats.csv` with a row every 2s for the full test duration.

### Phase 4 — Orchestration Script

Write `run.sh`:

```
1. cargo build --release (if not already built)
2. docker compose build
3. docker compose up -d marreta
4. Wait for healthcheck to pass (poll /health with timeout)
5. mkdir -p results/
6. Start collect_stats.sh in background → stats_<timestamp>.csv
7. docker compose run k6 → k6_<scenario>_<timestamp>.json (one per scenario)
8. Kill collect_stats.sh
9. docker compose down
10. Print summary_<timestamp>.txt: key metrics from all three JSON files + peak mem/cpu from CSV
```

**Deliverable:** `./tests/load/run.sh` runs the full suite end-to-end with a single command.

### Phase 5 — Documentation & Baseline Record

- Add `tests/load/README.md` with:
  - Prerequisites (Docker, k6 if running outside Docker)
  - How to run: `./tests/load/run.sh`
  - How to interpret results
  - Description of each output file
- Record the first run results as the official v0.4.0 baseline in `docs/performance/BASELINE_040.md`
- Baseline doc format: table with p50/p95/p99/rps per scenario + peak mem/cpu + test environment (host specs)

---

## 6. Acceptance Criteria

- [x] **(Docker)** `docker compose up` in `tests/load/` builds and starts the ecommerce server from local source. Server is reachable at `http://localhost:8080/health`.

- [x] **(k6 — scenarios)** All three scenarios run without script errors. Each scenario exercises the correct route with the correct payload shape.

- [x] **(k6 — error injection)** ~10% of `/orders` requests use the missing-field payload and receive 422. These do not count as failures in the error rate threshold.

- [x] **(k6 — output)** `k6_summary.json` produced inside a timestamped folder under `results/`, with `http_req_duration` and `http_reqs` populated for all three scenarios.

- [x] **(stats collector)** `stats.csv` contains one row per 2-second sample for the full test duration. CPU and memory columns are populated for the `marreta` container.

- [x] **(run.sh)** A single `./tests/load/run.sh` call executes the full pipeline: build → start → collect → test → teardown → summary. No manual steps required.

- [x] **(baseline)** `docs/performance/BASELINE_040.md` records the official v0.4.0 baseline: min/avg/med/p90/p95/p99/max latency and req/s for each scenario, peak memory (19.5 MiB), peak CPU (160.82%), and host environment. Run `20260327T150853Z` is the canonical reference.
