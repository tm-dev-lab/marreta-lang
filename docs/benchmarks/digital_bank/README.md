# Digital Bank Benchmark (MongoDB)

Compares MarretaLang against equivalent **FastAPI** (Python, `motor`) and
**NestJS** (Node.js, `@nestjs/mongoose`) services on a doc-backed REST workload:
a small digital bank with accounts, balances, deposits, withdrawals, transfers,
and a transaction ledger, all persisted in **MongoDB** via MarretaLang's `doc`
API. It measures runtime/framework overhead on a realistic database-bound
workload.

Everything runs in containers. The three app containers are each capped at
**1 CPU / 1 GB**; MongoDB is a shared dependency (uncapped, identical for all)
and is **not** the subject under test. The load generator (k6) runs in its own
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

The FastAPI and NestJS images are built automatically by Docker Compose on first
run.

## Running

From this directory:

```bash
# One runtime at a time (marreta | fastapi | nest)
bash scripts/run_one.sh marreta

# All three sequentially, sharing one RUN_TS (tears MongoDB down at the end)
bash scripts/run_all.sh
```

Each run starts MongoDB, **drops the `bank` database for a clean slate**, then
seeds a pool of funded accounts (in the k6 `setup()` phase, before measurement)
and drives a read/write operation mix against them.

### Knobs (environment variables)

| Variable         | Default            | Meaning                                          |
|------------------|--------------------|--------------------------------------------------|
| `RATE`           | `500`              | Target requests/second (constant arrival rate).  |
| `DURATION`       | `60s`              | Load duration (k6 format).                        |
| `ACCOUNTS`       | `50`               | Funded accounts created before the load.          |
| `STATS_INTERVAL` | `2`                | Seconds between CPU/memory samples.               |
| `MARRETA_IMAGE`  | `marreta-lang:dev` | Runtime image to benchmark.                       |
| `RUN_TS`         | UTC timestamp      | Output folder name under `results/`.              |

Example:

```bash
RATE=1000 DURATION=90s bash scripts/run_one.sh nest
```

## Results

Each run prints a summary and writes raw artifacts to
`results/<RUN_TS>/<target>/`:

| File               | Contents                                                       |
|--------------------|----------------------------------------------------------------|
| `k6_summary.json`  | Full k6 metrics, including per-endpoint `http_req_duration`.    |
| `stats.csv`        | CPU% and memory time series sampled every `STATS_INTERVAL`.     |
| `config.json`      | Run parameters (rate, duration, accounts, caps, host info).    |
| `container.log`    | Stdout/stderr of the app container.                            |

The printed summary reports throughput, error rate, aggregate latency
(`avg`, `p50`, `p90`, `p95`, `p99`), and **both average and peak** CPU/memory
(the sustained average is a better footprint signal than a transient peak).

### Per-endpoint latency

The operation tags are `get_balance`, `get_account`, `list_transactions`,
`deposit`, `withdraw`, `transfer`, `create_account`:

```bash
jq -r '.metrics | to_entries[]
  | select(.key|startswith("http_req_duration{endpoint:"))
  | "\(.key|ltrimstr("http_req_duration{endpoint:")|rtrimstr("}"))\tavg=\(.value.avg|.*1000|round/1000)\tp95=\(.value["p(95)"])\tp99=\(.value["p(99)"])"' \
  results/<RUN_TS>/marreta/k6_summary.json
```

> `results/` is git-ignored — runs are local artifacts, not committed.
