# MarretaLang Benchmarks

Comparative benchmarks for MarretaLang and equivalent implementations in other runtimes.

Each benchmark lives in its own subdirectory under `docs/benchmarks/`, with its own README and
runner.

- [`docs/benchmarks/digital_bank`](digital_bank/README.md) — a MongoDB-backed digital-bank REST
  workload (accounts, balances, deposits, withdrawals, transfers) comparing MarretaLang against
  FastAPI (Python) and NestJS (Node.js). See its README for prerequisites, how to run, and the
  benchmark contract.
