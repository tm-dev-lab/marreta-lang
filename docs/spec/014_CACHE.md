# Implementation Plan — v0.9 Cache Module

**Status:** ✅ Complete — all 5 phases implemented for v0.9.0.

## Overview

v0.9 introduces a key-value cache integration layer to MarretaLang. The goal is
to let routes and tasks read/write cached values, use atomic counters for rate
limiting, and express idempotency/locking primitives — all using the same
ergonomics established by `db.*`, `doc.*`, and `queue.*`.

The initial provider target is **Redis**. The architecture mirrors the existing
infrastructure modules: provider and URL are declared via environment variables,
and application code never references Redis-specific concepts.

**Scope discipline.** Cache is intentionally minimal. If an operation does not
exist across Redis, Memcached, Valkey, KeyDB, Dragonfly, and DynamoDB TTL, it
is out of scope. That excludes hashes, lists, sorted sets, pub/sub, Lua scripts,
pattern scans, and global flush. Those are either different namespaces (`db.*`,
`doc.*`, `queue.*`) or anti-patterns at scale.

---

## Syntax Reference

### Core Operations

```marreta
# SET — returns the value stored (pipeline-friendly, same as db.*.save)
cache.set("key", value)
cache.set("session:#{token}", user, ttl: 3600)

# SET if absent — atomic "store only if no key exists"
# Returns the value on success, null if the key already existed.
stored = cache.set("idempotency:#{key}", result, ttl: 86400, only_if_absent: true)
require stored else fail 409, "duplicate request"

# GET — returns the value or null on miss / expiry
value = cache.get("key")

# DELETE — returns true if the key existed, false otherwise
cache.delete("key")

# EXISTS — boolean, no payload fetch
alive = cache.exists("session:#{token}")

# TTL — seconds remaining; null if the key has no TTL or does not exist
remaining = cache.ttl("session:#{token}")

# EXPIRE — refresh TTL without re-writing the value (sliding sessions)
cache.expire("session:#{token}", ttl: 3600)

# INCREMENT / DECREMENT — atomic, return the new value
cache.incr("page_views:#{slug}")
cache.incr("rate_limit:#{ip}", by: 1, ttl: 60)
count = cache.decr("credits:#{user_id}")
```

### Bulk Operations

```marreta
# GET many — map keyed by the requested keys; misses are null entries
users = cache.get_many(["user:1", "user:2", "user:3"])

# SET many — shared TTL for the whole batch
cache.set_many({ "user:1": user1, "user:2": user2 }, ttl: 300)
```

### Schema Contracts (optional)

```marreta
# Write path — strip fields not declared in the schema before serializing
cache.set("user:#{id}" as user_schema, payload, ttl: 300)

# Read path — validate cached payload on load
# Mismatch → treated as a cache miss (returns null). Stale values written by
# a previous deploy are self-healing: they miss, fall through to the source,
# and get rewritten in the new shape.
user = cache.get("user:#{id}") as user_schema
```

### Read-through Idiom (no dedicated syntax)

Read-through is expressed with `or` + pipeline — no new keyword:

```marreta
user = cache.get("user:#{id}") or db.users.find(id) >> cache.set("user:#{id}", ttl: 300)
```

### Schema Behavior Summary

| Context | Without `as` | With `as` |
|---|---|---|
| `cache.set` | stores payload as-is (JSON) | strips fields not in schema before serializing |
| `cache.get` | returns payload as-is | validates on read; mismatch → `null` (treated as miss) |
| `cache.set_many` | same as `cache.set`, per entry | not supported in v0.9 (revisit if needed) |

---

## Environment Variables

```
MARRETA_CACHE_PROVIDER=redis                       # required if cache.* is used
MARRETA_CACHE_HOST=cache.internal                  # required
MARRETA_CACHE_PORT=6379                            # optional, defaults to 6379
MARRETA_CACHE_USER=default                         # optional, advanced (Redis ACL)
MARRETA_CACHE_PASSWORD=secret                      # optional unless cache auth is enabled
MARRETA_CACHE_DB=0                                 # optional, advanced; default 0
MARRETA_CACHE_PREFIX=myapp:prod:                   # optional, default ""
MARRETA_CACHE_DEFAULT_TTL=3600                     # optional, default null (no TTL)
MARRETA_CACHE_POOL_SIZE=10                         # optional, default 10
MARRETA_CACHE_CONNECT_TIMEOUT_MS=2000              # optional, default 2000
MARRETA_CACHE_OPERATION_TIMEOUT_MS=1000            # optional, default 1000
MARRETA_CACHE_RECONNECT_MAX_RETRIES=10             # optional, default 10
```

Same pattern as `MARRETA_DB_*`, `MARRETA_DOC_*`, and `MARRETA_QUEUE_*`:

- `PROVIDER` is **never defaulted**. If the `.marreta` code calls `cache.*` and
  `MARRETA_CACHE_PROVIDER` is not set, startup fails fast with:
  ```
  cache.* called but no cache is configured (set MARRETA_CACHE_PROVIDER,
  MARRETA_CACHE_HOST, and MARRETA_CACHE_PORT)
  ```
- `HOST` and `PORT` have no localhost fallback — explicit is safer than magical
  (lesson from v0.8 queue driver).
- `USER` is advanced because many Redis deployments only require a password; it
  exists to support ACL-backed production clusters without forcing that shape on
  the common local case.
- `PREFIX` is transparently prepended to every key at the driver boundary. The
  `.marreta` code never sees it; it enables multi-tenant / multi-env isolation
  on a shared cache cluster without code changes.
- `DEFAULT_TTL` is a safety net against "forgot TTL → key lives forever → memory
  leak". When set, any `cache.set` / `cache.set_many` without an explicit `ttl:`
  uses this value. Default is `null` (no implicit TTL) for backward safety.
- `OPERATION_TIMEOUT_MS` is important: a slow cache must not stall the request
  path. Per-op timeout fails fast; pair with `rescue null` for soft-fail reads.

---

## Error Semantics

| Failure | Behavior |
|---|---|
| Key miss on `cache.get` | Returns `null` (not an error) |
| Expired key on `cache.get` | Returns `null` (not an error) |
| Schema mismatch on `cache.get ... as schema` | Returns `null` (treated as miss) |
| Connection failure, timeout, protocol error | **Raises** (same as `db.*` / `queue.*`) |
| Serialization error on write | Raises |
| `only_if_absent: true` and key exists | Returns `null` (not an error) |

Soft-fail is opt-in per call:

```marreta
# "cache is a hint" — survive cache outages
user = cache.get("user:#{id}") rescue null
```

---

## AST & Parser

**Implementation note:** No new AST expression variants were added. `cache` is
a `TokenKind::Cache` that produces `Expression::Identifier("cache")`. Method
calls like `cache.set(...)` are parsed as standard `Expression::MethodCall`,
and dispatch happens at the interpreter level via `dispatch_cache()` — the same
pattern used by `db.*` and `doc.*`. Named parameters (`ttl:`, `only_if_absent:`,
`by:`) reuse the existing `Argument::Named` AST node.

Pipeline support (`value >> cache.set("key")`) required a parser fix: pipeline
stage precedence was changed from `PREC_CALL` to `PREC_CALL - 1` so that
dot-access is included in the stage expression.

---

## Cache Module (`src/cache/`)

```
src/cache/
├── mod.rs        # CacheEngine, from_env() wiring
├── driver.rs     # CacheDriver trait, CacheError, MockCacheDriver (#[cfg(test)])
└── redis.rs      # RedisDriver via `redis` crate (async, connection pool)
```

### `CacheDriver` trait

```rust
#[async_trait]
pub trait CacheDriver: Send + Sync {
    async fn get(&self, key: &str) -> CacheResult<Option<Value>>;
    async fn set(&self, key: &str, value: &Value, ttl: Option<Duration>, only_if_absent: bool) -> CacheResult<Option<Value>>;
    async fn delete(&self, key: &str) -> CacheResult<bool>;
    async fn exists(&self, key: &str) -> CacheResult<bool>;
    async fn ttl(&self, key: &str) -> CacheResult<Option<Duration>>;
    async fn expire(&self, key: &str, ttl: Duration) -> CacheResult<bool>;
    async fn incr(&self, key: &str, by: i64, ttl: Option<Duration>) -> CacheResult<i64>;
    async fn decr(&self, key: &str, by: i64, ttl: Option<Duration>) -> CacheResult<i64>;
    async fn get_many(&self, keys: &[String]) -> CacheResult<HashMap<String, Option<Value>>>;
    async fn set_many(&self, entries: &HashMap<String, Value>, ttl: Option<Duration>) -> CacheResult<()>;
}
```

Key prefixing is applied inside the driver — callers pass unprefixed keys.

### `CacheError`

```rust
pub enum CacheDriverError {
    ConnectionFailed(String),
    OperationTimeout(String),
    SerializationError(String),
    InvalidKey(String),
}
```

Mapped to `MarretaError::CacheError` in the interpreter so error messages stay
in Marreta Identity (no `redis::` leakage).

### Crate choice

`redis = "1.2"` with `tokio-comp` and `connection-manager` features. Uses
`ConnectionManager` for automatic reconnection — no separate pool crate needed.

---

## Interpreter Integration

- `Interpreter` extended with `cache_driver: Option<Arc<dyn CacheDriver>>` and
  `cache_config: Option<CacheConfig>`, mirroring the existing `queue` / `db` / `doc` fields.
- `dispatch_cache()` handles all 10 operations via `MethodCall` dispatch on
  `Value::CacheNamespace` (no new AST expression variants).
- Named parameter extraction via `resolve_ttl()`, `resolve_named_bool()`,
  `resolve_named_i64()` helpers.
- `DEFAULT_TTL` fallback resolved at evaluator level, not driver level — keeps
  the driver dumb and makes the fallback observable in tests.
- Pipeline injection for `cache.set` handled in `apply_pipeline_value()`.
- Schema contracts (`as schema_name`) deferred to a future version.
- Runtime error when `cache.*` is called without a configured driver:
  ```
  cache.* called but no cache is configured (set MARRETA_CACHE_PROVIDER,
  MARRETA_CACHE_HOST, and MARRETA_CACHE_PORT)
  ```

---

## Health Endpoint

Extend `GET /_health` with a `cache` field, mirroring `db` / `doc` / `queue`:

```json
{
  "ok": true,
  "api": "marreta-app",
  "version": "0.9.0",
  "db":    { "ok": true },
  "doc":   null,
  "queue": { "ok": true },
  "cache": { "ok": true }
}
```

`cache` is `null` when `MARRETA_CACHE_PROVIDER` is not configured, consistent
with the existing convention.

Cache driver exposes a lightweight `ping()` used by the health check with a
short timeout (reuse `MARRETA_CACHE_OPERATION_TIMEOUT_MS`).

---

## Phases

### Phase 1 — AST & Tokens ✅
- `TokenKind::Cache`; `Value::CacheNamespace`; `as_integer()` helper on `Value`
- No new AST expression variants — reuses `MethodCall` dispatch (same pattern as db/doc)

### Phase 2 — Cache Driver ✅
- `src/cache/driver.rs` — `CacheDriver` trait (10 async operations), `CacheDriverError`, `MockCacheDriver`
- `src/cache/redis.rs` — `RedisDriver` with `ConnectionManager`, transparent key prefixing, per-op timeout, JSON ser/de
- `src/cache/mod.rs` — `CacheConfig::from_env()`, `CacheEngine::from_env()`
- `Cargo.toml` — `redis = { version = "1.2", features = ["tokio-comp", "connection-manager"] }`

### Phase 3 — Interpreter Integration ✅
- `dispatch_cache()` handles all 10 operations with named param extraction (`ttl:`, `only_if_absent:`, `by:`)
- `resolve_ttl()` respects `MARRETA_CACHE_DEFAULT_TTL`; `require_cache_driver()` fail-fast
- Pipeline injection for `cache.set` in `apply_pipeline_value()`
- 16 new unit tests

### Phase 4 — Health & Server Wiring ✅
- `cache_engine` field on `ServerConfig`; `/_health` gains `cache` field
- `main.rs` initialization; threaded driver+config through serve/register_route/execute_route/start_consumers
- (OpenAPI: no new extension — cache is internal to route execution)

### Phase 5 — Functional Tests & Docs ✅
- `examples/functional_tests/routes/cache.marreta` — 15 routes (Section 29)
- `examples/functional_tests/docker-compose.yml` — Redis 7 Alpine on port 6380
- `examples/functional_tests/test.sh` — 18 cache test cases
- Fixed parser bug: pipeline stage precedence `PREC_CALL` → `PREC_CALL - 1` for dot-access
- `docs/spec/SPEC.md` §8 updated; `CHANGELOG.md` updated

### Test Coverage Requirement

All phases must maintain the **80% unit test coverage** floor across `src/`:

- `src/cache/driver.rs`, `src/cache/mod.rs` — trait, config, error types: ≥ 80%
- `src/cache/redis.rs` — via `MockCacheDriver` for unit tests; integration tests
  against a real Redis are separate
- `src/parser.rs` — new grammar rules: happy path + error cases + edge cases
- `src/interpreter.rs` — all `Cache*` branches, schema read/write, default TTL,
  driver-absent error

---

## Design Watch Points

### 1. `only_if_absent:` as flag vs. dedicated `cache.add`

Decision: **flag on `cache.set`**. Dedicated `cache.add` would work (and matches
memcached vocabulary), but it bloats the namespace for a single orthogonal
concern. The flag makes the intent explicit at the call site and keeps the API
surface small.

### 2. Distributed locks

Not a first-class API in v0.9. `cache.set(key, token, ttl: N, only_if_absent: true)`
composed with `cache.delete(key)` is enough to express a basic lock pattern. A
future version may add `cache.lock(key, ttl:)` as syntactic sugar if usage
warrants, but it is not core.

### 3. Schema mismatch on `get` — null vs. raise

Decision: **null (treated as a miss)**. The reasoning:

- Cache is expected to be ephemeral. A value written by a previous deploy with
  an older schema is a normal operational condition, not an error.
- Treating it as a miss makes the read-through idiom self-heal: miss → source
  of truth → re-cache in the new shape.
- If strict validation is needed, the caller can compose: read from `db.*`
  directly and skip cache, or inspect the cached value manually.

This is **different from** the queue consumer behavior (schema mismatch → nack),
because queue deliveries come from a publisher that should be in-sync, while
cache reads commonly cross deploy boundaries.

### 4. Key prefix scope

`MARRETA_CACHE_PREFIX` applies to all keys, including `get_many` / `set_many`
entries. The prefix is stripped from keys returned by `get_many` so the caller
sees the unprefixed keys it asked for.

### 5. `DEFAULT_TTL` and counters

`cache.incr` / `cache.decr` with no explicit `ttl:` do **not** apply
`DEFAULT_TTL`. Counters are commonly expected to persist (e.g. lifetime page
views) — an implicit TTL would silently reset them. Explicit is safer than
magical: when a counter needs a TTL, it must be stated.

### 6. Value size limits

No enforcement in MarretaLang. Redis has a 512 MB per-value limit; Memcached
defaults to 1 MB. Documented as provider-dependent. If a cached value exceeds
the provider limit, the driver's `SerializationError` surfaces as a runtime
error — fix in application code, not in the language.

### 7. Bulk atomicity

`set_many` is **not** guaranteed to be atomic across keys — Redis `MSET` is
atomic but pipelined batches over cluster slots may not be. Document as
best-effort. Atomic multi-key writes are out of scope; use `db.*` transactions
if atomicity is required.
