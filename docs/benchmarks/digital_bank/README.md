# Digital Bank Benchmark (MongoDB)

Compares MarretaLang against equivalent **FastAPI** (Python, `motor`),
**NestJS** (Node.js, `@nestjs/mongoose`), and **Spring Boot** (Java, `spring-data-mongodb`)
services on a doc-backed REST workload: a small digital bank with accounts, balances, deposits,
withdrawals, transfers, and a transaction ledger, all persisted in **MongoDB** via MarretaLang's
`doc` API. It measures runtime/framework overhead on a realistic database-bound workload. The
pre-registered protocol and the result record live in `METHODOLOGY.md` and `RESULTS.md`.

The four apps are kept feature-identical: `scripts/contract_parity.py` runs the same request
sequence against each and asserts the same status codes, success-body shapes, and deterministic
values, so no app is a strawman or skips validation or the funds check.

Everything runs in containers. The four app containers are each capped at
**1 CPU / 1 GB**; MongoDB is a shared dependency (identical for all) and is
**not** the subject under test. The load generator (k6) runs in its own
container, and one runtime is exercised at a time.

The MarretaLang app was scaffolded with `marreta init --with doc` and lives in
`apps/marreta/` (`routes/`, `schemas/`, `tasks/` per convention).

## Domain

| Method | Route                          | Operation                                  |
|--------|--------------------------------|--------------------------------------------|
| GET    | `/health`                      | liveness                                   |
| POST   | `/accounts`                    | open an account (balance 0)                |
| GET    | `/accounts/:id`                | fetch an account                           |
| GET    | `/accounts/:id/balance`        | current balance                            |
| POST   | `/accounts/:id/deposit`        | credit + ledger entry                      |
| POST   | `/accounts/:id/withdraw`       | debit (funds check) + ledger entry         |
| POST   | `/transfers`                   | move funds between two accounts            |
| GET    | `/accounts/:id/transactions`   | recent transaction history                 |

Money is stored as integer minor units (cents).

## Prerequisites

- Docker and Docker Compose (with the `mongosh`-capable `mongo` image pulled on
  first run).
- `jq` on the host (used to print the summary).
- The MarretaLang runtime image **`marreta-lang:dev`** must already exist. The
  benchmark only *consumes* it — it never builds the runtime. Build it from the
  `marreta-lang` repository root:

  ```bash
  cargo build --release
  docker build -t marreta-lang:dev .
  ```

  Verify the image carries the binary you intend to test:

  ```bash
  docker run --rm --entrypoint sha256sum marreta-lang:dev /usr/local/bin/marreta
  sha256sum target/release/marreta   # must match
  ```

The FastAPI, NestJS, and Spring Boot images are built automatically by Docker Compose on
first run.

## Running

The full pre-registered study (protocol, metrics, and a reproducibility runbook) is in
[`METHODOLOGY.md`](METHODOLOGY.md). In short, from this directory:

```bash
# Fixed-load comparison: levels x 3 interleaved reps, MongoDB capped, then aggregation.
LEVELS="200 500 1000" REPS=3 WARMUP=120s DURATION=300s bash scripts/run_study.sh
# Saturation: max sustainable throughput per stack, MongoDB headroom.
bash scripts/run_saturation.sh
# One single cell (marreta | fastapi | nest | spring), for a quick check:
RATE=500 WARMUP=30s DURATION=60s bash scripts/run_one.sh marreta
```

Each run starts MongoDB, **drops the `bank` database for a clean slate**, seeds a pool of
funded accounts (in the k6 `setup()` phase, before measurement), warms up, then drives a
read/write operation mix during the measurement window.

### Knobs (environment variables)

| Variable | Default | Meaning |
|---|---|---|
| `LEVELS` | `200 500 1000` | Fixed arrival rates (req/s) for the study. |
| `REPS` | `3` | Interleaved repetitions per level. |
| `WARMUP` | `120s` | Warmup duration, discarded (lets the JVM JIT settle). |
| `DURATION` | `300s` | Steady-state measurement window. |
| `RATE` | `500` | Arrival rate for a single `run_one.sh`. |
| `ACCOUNTS` | `50` | Funded accounts created before the load. |
| `MARRETA_IMAGE` | `marreta-lang:dev` | Runtime image to benchmark. |
| `RUN_TS` | UTC timestamp | Output folder name under `results/`. |

## Results

A study run writes, per cell, to `results/<RUN_TS>/rate_<level>/rep_<n>/<target>/`, plus the
aggregate at the run root `results/<RUN_TS>/`:

| File | Contents |
|---|---|
| `summary.json` (per cell) | Compact metrics: throughput, error rate, latency avg/p50/p90/p95/p99, CPU and memory avg+peak, idle, startup, MongoDB peak. |
| `config.json` (per cell) | Run parameters: target, rate, warmup, window, accounts, caps, mongo cap mode, image, host, timestamp. |
| `summary.json` + `summary.csv` (run root) | Aggregated median and CV per metric across reps, with the consistency gate. |
| `saturation_summary.json` | Per stack: max sustainable throughput and the `app` vs `whole-system` limiter. |
| `k6_summary.json` | Full k6 metrics, including per-endpoint `http_req_duration`. |
| `stats.csv`, `mongo_stats.csv` | CPU/memory time series for the app and MongoDB. |
| `container.log` | Stdout/stderr of the app container. |

The whole `results/` tree is **git-ignored and regenerates on a re-run**. The study's committed data
of record is [`RESULTS.md`](RESULTS.md) (and [`DX.md`](DX.md)), not a tree of per-run files, so the
repo carries the lean study rather than the pile of run outputs.

### Per-endpoint latency

The operation tags are `get_balance`, `get_account`, `list_transactions`,
`deposit`, `withdraw`, `transfer`, `create_account`:

```bash
jq -r '.metrics | to_entries[]
  | select(.key|startswith("http_req_duration{endpoint:"))
  | "\(.key|ltrimstr("http_req_duration{endpoint:")|rtrimstr("}"))\tavg=\(.value.avg|.*1000|round/1000)\tp95=\(.value["p(95)"])\tp99=\(.value["p(99)"])"' \
  results/<RUN_TS>/rate_500/rep_1/marreta/k6_summary.json
```

> The entire `results/` tree is git-ignored and regenerable; the committed record of the study is
> `RESULTS.md` (and `DX.md`), not the per-run files.
