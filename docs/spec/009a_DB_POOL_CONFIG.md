# Implementation Plan — DB Connection Pool Configuration (v0.6.1)

**Status**: ✅ Implemented
**Version**: v0.6.1
**Motivation**: Load test `LOAD_TEST_DB_20260330_2112.md` revealed that the hardcoded sqlx
default of 10 connections creates severe queuing at 200 VUs, inflating `products` p95 from
~1 ms (CPU-bound) to 36.9 ms (pool-wait dominated).

---

## 1. Problem

`PostgresDriver::connect()` called `PgPool::connect(url)` — sqlx's convenience constructor
which uses 100% defaults: `max_connections=10`, `acquire_timeout=30s`, etc.

At 200 VUs with ~2 ms Postgres round-trip, ~190 concurrent futures queue for a connection
slot at any moment. The queue wait, not the query, dominates latency.

Operators had no way to tune the pool without recompiling.

---

## 2. Solution

Expose all six sqlx `PgPoolOptions` parameters as `MARRETA_DB_POOL_*` environment variables
(and `marreta.env` file entries). All are optional — omitting them preserves the sqlx
defaults exactly.

---

## 3. New Environment Variables

| Variable | Type | Default | Description |
|---|---|---|---|
| `MARRETA_DB_POOL_MAX_CONNECTIONS` | `u32` | `10` | Maximum simultaneous connections |
| `MARRETA_DB_POOL_MIN_CONNECTIONS` | `u32` | `0` | Minimum connections kept open when idle |
| `MARRETA_DB_POOL_ACQUIRE_TIMEOUT_SECS` | `u64` | `30` | Seconds to wait before returning `DbError: pool timed out` |
| `MARRETA_DB_POOL_IDLE_TIMEOUT_SECS` | `u64` | `600` | Seconds idle before a connection is closed |
| `MARRETA_DB_POOL_MAX_LIFETIME_SECS` | `u64` | `1800` | Max connection lifetime before recycling |
| `MARRETA_DB_POOL_TEST_BEFORE_ACQUIRE` | `bool` | `true` | Ping connection before handing it out |

Boolean values: `false` or `0` → disabled; any other string → enabled.

---

## 4. Files Changed

### `src/config.rs`
- Added 6 new `Option<T>` fields to `MarretaConfig` struct.
- Added reading of all 6 from `marreta.env` file vars + environment variables in `load()`.
- Updated test struct instantiations to include `None` for all new fields.

### `src/db/postgres.rs`
- Added `use sqlx::postgres::PgPoolOptions` and `use std::time::Duration`.
- Added `pub struct PoolConfig` carrying the 6 optional parameters.
- Changed `connect(url: &str)` → `connect(url: &str, cfg: PoolConfig)`.
- Replaced `PgPool::connect(url)` with `PgPoolOptions::new()` builder chain.
- Each option: `cfg.field.unwrap_or(sqlx_default)`.

### `src/db/mod.rs`
- Added `use crate::config::MarretaConfig`.
- Changed `from_config(provider, url)` → `from_config(config: &MarretaConfig)`.
- Extracts `db_url`, `db_provider` from config directly.
- Constructs `PoolConfig` from config fields and passes to `PostgresDriver::connect()`.
- Fixed `TypeError` → `DbError` for missing URL and unsupported provider errors.

### `src/main.rs`
- Both `DbEngine::from_config(...)` call sites updated to `from_config(&config)` / `from_config(&marreta_config)`.

### `src/interpreter.rs`
- Fixed pre-existing test failure: `interpolate_string` now coerces undefined variables
  to `Value::Null` instead of propagating `UndefinedVariable` error. This matches the
  design intent (Ruby-style `nil` for undefined refs in interpolated strings).

### `src/openapi.rs` + `src/parser.rs`
- Fixed pre-existing compile errors in test helpers: `Statement::Reply.status_code` is
  `Expression`, not `i64` — test helpers updated to wrap with `Expression::Integer(...)`.

---

## 5. Sizing Guide

Formula: `MARRETA_DB_POOL_MAX_CONNECTIONS ≈ 1.5 × expected_peak_concurrent_db_calls`

Where `expected_peak_concurrent_db_calls = peak_VUs × (db_time / iteration_time)`.

Example — 200 VUs, 2 ms DB, 10 ms sleep:
- Active ratio: 2 / (2 + 10) = 16.7%
- Peak concurrent: 200 × 0.167 ≈ 34
- Recommended: `MARRETA_DB_POOL_MAX_CONNECTIONS=50`

---

## 6. Load Test Validation

See `docs/performance/LOAD_TEST_POOL_CONFIG_20260331_0030.md`.

Summary with `MARRETA_DB_POOL_MAX_CONNECTIONS=50` vs default 10:

| Metric | pool=10 | pool=50 | Delta |
|---|---|---|---|
| products p95 | 36.9 ms | 4.9 ms | **−87%** |
| orders p95 | 21.1 ms | 3.6 ms | **−83%** |
| Throughput | 4,039 req/s | 4,826 req/s | **+19.5%** |
