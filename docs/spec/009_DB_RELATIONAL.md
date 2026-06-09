# MarretaLang — Implementation Plan: DB Module (Relational, v0.5.0–v0.5.1)

> **Status: CLOSED** — All 6 phases complete. 130/130 functional tests passing. DB module is not accepting new scope; further relational enhancements go in a new plan.

## Context and Design Rationale

This plan was shaped by a deliberate conversation about what a good database API looks like for a keyword-driven DSL. The decisions below are not arbitrary — they reflect explicit trade-offs. Anyone picking this up in a new session should read this section first.

### Why three namespaces (`db`, `doc`, `cache`)?

The temptation is to unify everything under `db`. We rejected this because the three namespaces carry fundamentally different semantics:

- `db.*` — relational: tables, rows, foreign keys, transactions, SQL. Impedance mismatch between objects and rows is a known problem; hiding it produces surprises.
- `doc.*` — document: collections, flexible schema, no joins, no multi-collection transactions. MongoDB's model.
- `cache.*` — key-value: strings mapped to values, TTL, atomic counters. Redis's model.

Mixing them into one namespace would force the API to be the intersection of all three, which is the weakest subset. The separation makes the semantics explicit in the code — a developer reading `db.orders.find(id)` knows they are touching a relational table.

### Why two styles (direct + pipeline)?

A pure pipeline-only API is elegant but verbose for the 80% case. A pure direct-call API handles simple cases well but becomes awkward for multi-clause queries.

We resolved this by supporting both:

- **Direct**: `db.users.find(id)`, `db.users.save(payload)` — for single-step operations where the brevity matters.
- **Pipeline**: `db.orders >> where(...) >> join(...) >> fetch` — for queries with filters, joins, ordering, pagination.

Crucially, since we are building the language from scratch with no existing userbase, there is no reason to defer pipeline composition to a later version. We design it right from the beginning. This was a deliberate decision made in this session.

### Why expression-based `where` instead of filter suffixes?

Options considered:
1. A sub-DSL: `where("total > ? AND status = ?", 1000, "active")` — too close to raw SQL, leaks relational details.
2. Nested maps: `where({ total: { gt: 1000 } })` — verbose, breaks the flat-map style of the language.
3. Key suffixes: `where(total_gt: 1000, status: "active")` — consistent with map syntax and easy to parse, but the convention is not obvious to a first-time reader and the `_gt` suffix feels like a workaround.
4. **Boolean expressions: `where(total > 1000, status: "active")`** — reads like the language, uses operators already in the language, no new convention to learn.

We chose option 4. This was a deliberate refinement made after noticing that suffix-based filters felt "magical" and inconsistent with the keyword-driven identity of the language. The `>`, `>=`, `<`, `<=`, `!=` operators are already part of the language. `like` and `in` are added as context keywords — recognized as operators only inside `where(...)`, so they do not conflict with variable names elsewhere.

Key-value shorthand (`status: "active"`) is kept for equality because it is already idiomatic in the language and saves two characters in the most common case.

The parser challenge: inside `where(...)`, the argument list must accept both `key: value` pairs (map-style) and standalone boolean expressions (`total > 1000`). This is handled by parsing each comma-separated argument as either:
- An expression starting with an identifier followed by `:` → equality filter
- Any other expression → boolean filter clause extracted at evaluation time

The column name and value are extracted from the AST of the boolean expression (e.g. `BinaryOp { left: Identifier("total"), op: Gt, right: Integer(1000) }` → `FilterClause { column: "total", op: Gt, value: 1000 }`). If the expression is not a supported comparison (e.g. a complex arithmetic expression), the engine emits a descriptive error at startup.

### Why `on:` FK convention for joins instead of explicit `ON` SQL?

`>> join("users", on: "user_id")` — the `on:` value is the foreign key on the left table. The engine infers `orders.user_id = users.id`. This covers the 90% case (simple FK joins) cleanly. For anything more complex (multi-column FK, non-standard naming, multiple levels), the developer drops to `native_query` with SQL aliases.

Result columns are prefixed (`orders.id`, `users.name`) to prevent silent collision. For cleaner keys, the developer uses `native_query` with SQL aliases: `SELECT o.id AS order_id, u.name AS user_name`.

### Why does `native_query` use string interpolation instead of positional params?

The language already has `#{}` string interpolation as the standard way to embed variables in strings. Introducing a separate positional parameter syntax (`?` or `$1`) would be inconsistent. So `db.native_query("... WHERE email = '#{email}'")` extracts the interpolated variables at parse/evaluation time and binds them as `$1`, `$2`, ... prepared statement parameters internally. The developer writes idiomatic MarretaLang; the engine handles the SQL injection protection transparently.

### Why is `*>>` a prerequisite for the DB module?

The `*>>` broadcast operator is needed for running multiple DB aggregation tasks in parallel on a fetched result set. It was implemented in Phase 0 before the DB module to validate the parallel execution model independently.

Implementation approach chosen: **`Interpreter: Clone` + `std::thread::spawn`** — not async.

We originally planned to make the interpreter fully async (`async fn evaluate`) to match `sqlx`'s async model. During Phase 0 implementation, we discovered that making the entire evaluate call chain async is a large structural refactor with non-trivial risk. Instead:

- `Interpreter` derives `Clone` — each `*>>` branch gets a full independent interpreter fork
- `std::thread::spawn` per branch — true parallelism without async
- Results joined in declaration order — deterministic output

For the DB module, `sqlx` async calls will be handled via `tokio::runtime::Handle::current().block_on(...)` at the call site (the DB dispatch function), keeping the rest of the interpreter synchronous. This is a deliberate trade-off: avoids a full async refactor while still supporting DB operations. If the interpreter is eventually made async, this shim can be removed.

### Why is a `QueryBuilder` without a terminal a startup error?

A `QueryBuilder` is a lazy value — it holds accumulated clauses but has not executed anything. If it ends up in a variable that is never consumed by a terminal (`fetch`, `count`, etc.), the developer probably made a mistake (forgot `>> fetch`, for example). Detecting this at startup (during the two-pass load) rather than silently doing nothing at runtime is consistent with MarretaLang's philosophy of catching programmer errors early.

---

## Scope

### Phase 0 — Prerequisite: parallel `*>>` ✓ COMPLETE

- `Interpreter` derives `Clone` — fork-per-branch parallel execution via `std::thread::spawn`
- `apply_broadcast_value()` — passes full input value to each branch (no implicit iteration, distinct from `apply_pipeline_value()`)
- 6 new tests added; 479 tests passing
- Functionally validated with `examples/parallel/` (3 routes covering scalar, list, and chained broadcast)

### Phase 1 — Infrastructure ✓ COMPLETE

- `sqlx` with PostgreSQL driver, connection pool (`PgPool`), shared across requests
- `MARRETA_DB_PROVIDER` and `MARRETA_DB_URL` configuration keys
- Startup error on missing or invalid config
- `DbDriver` trait — abstract interface, structured to receive `DocDriver` and `CacheDriver` in later versions

### Phase 2 — Direct CRUD ✓ COMPLETE

- `db.TABLE.save(map)` → INSERT, returns full record via `RETURNING *`
- `db.TABLE.find(id)` → SELECT by PK, returns map or null
- `db.TABLE.find_all()` → SELECT all rows
- `db.TABLE.update(id, partial)` → UPDATE by PK, partial map
- `db.TABLE.delete(id)` → DELETE by PK

### Phase 3 — Pipeline composition ✓ COMPLETE

- `db.TABLE` returns a lazy `QueryBuilder` value
- Accumulating steps: `>> where(filters)`, `>> like("col", "pat")`, `>> in("col", list)`, `>> join(table, on: fk)`, `>> left_join(table, on: fk)`, `>> order_by(str)`, `>> limit(n)`, `>> offset(n)`
- Terminal operations: `>> fetch`, `>> fetch_one`, `>> count`, `>> exists`, `>> update({...})`, `>> delete`
- Filter operators via expression in `where()`: `>`, `>=`, `<`, `<=`, `!=`, `==`, plus key-value shorthand for equality

### Phase 4 — Native query + transactions ✓ COMPLETE

- `db.native_query("sql with #{expr}")` — `#{}` expressions extracted before interpolation, bound as prepared statement params (`$1`, `$2`, …)
- Result type mapping: PG types → `Value` (Integer, Float, String, Boolean, Null)
- `transaction` block — atomic, auto-rollback on failure, no nesting allowed
- Pipeline queries allowed inside `transaction`

### Phase 5 — Ecommerce example update ✓ COMPLETE

- [x] `routes/products.marreta` — full CRUD via `db.products.*` (GET list, GET by id, POST save, DELETE)
- [x] `routes/orders.marreta` — full CRUD via `db.orders.*`; `POST /orders` uses `transaction` block
- [x] `schemas/payloads.marreta` — `order_created` adds `order_id: integer`
- [x] `app.marreta` — bumped to `project_version = "2.0.0"`
- [x] `docker-compose.yml` — Postgres 16-alpine with healthcheck
- [x] `seed.sql` — DDL for `products` + `orders` tables; 3 sample rows
- [x] `marreta.env` — example env config pointing to local Postgres

### Phase 6 — Closing items (v0.5.1) ✅ Complete

Two items deferred from earlier phases, now resolved before closing the DB module:

**6a — `native_query` syntax: `#{}` interpolation (not positional `$1`)**

The original spec described `#{}` as the syntax for `native_query` parameters. The implementation shipped with explicit positional args (`$1`, `$2`, …) as a shortcut. This leaks PostgreSQL syntax into the language, violating the infrastructure-abstraction principle.

Correct syntax:
```marreta
# Variables/expressions interpolated — engine extracts as prepared params
rows = db.native_query("SELECT * FROM users WHERE email = #{email} AND active = #{is_active}")
rows = db.native_query("SELECT * FROM items WHERE name = #{params.name}")
```

The old positional form (`db.native_query("sql", param1, param2)`) is removed.

Implementation: at evaluation time, `dispatch_native_query` extracts `#{}` segments from the SQL string, evaluates each as an expression, replaces with `$1`, `$2`, … and passes values as the `params` vector to the driver. The driver implementation (`postgres.rs`) is unchanged — it already accepts `(sql, Vec<Value>)`.

**6b — `like` and `in` as pipeline steps**

Instead of context keywords inside `where()` (which require lexer/parser surgery), `like` and `in` are implemented as dedicated accumulating pipeline steps — consistent with `where`, `order_by`, `limit`, `offset`:

```marreta
db.users >> like("name", "João%") >> fetch
db.users >> in("status", ["active", "pending"]) >> fetch

# Fully composable with existing steps
db.orders
    >> where(active: true)
    >> like("description", "%urgent%")
    >> in("status", ["pending", "processing"])
    >> order_by("created_at desc")
    >> fetch
```

Implementation: two new cases in `apply_query_pipeline_stage` (`FunctionCall` arm), each producing a `FilterClause` with `FilterOp::Like` / `FilterOp::In` and appending to `next.filters`. No parser changes needed — `like(col, pattern)` and `in(col, list)` are parsed as regular function calls.

**Acceptance criteria — Phase 6:**

- [x] **AC-6.1:** `db.native_query("SELECT … WHERE col = #{var}")` extracts `var`, evaluates it, passes as prepared param
- [x] **AC-6.2:** Multiple `#{}` in one SQL string → multiple positional params in order
- [x] **AC-6.3:** Expression inside `#{}` (method call, arithmetic) is evaluated correctly
- [x] **AC-6.4:** SQL string with no `#{}` → executes with empty params list
- [x] **AC-6.5:** Old positional form `native_query("sql", param)` removed — clean error if extra positional args provided
- [x] **AC-6.6:** `>> like("col", "pattern")` adds `FilterOp::Like` clause to QueryBuilder
- [x] **AC-6.7:** `>> in("col", [v1, v2])` adds `FilterOp::In` clause with list expansion
- [x] **AC-6.8:** `like` and `in` compose with `where`, `order_by`, `limit`, `offset` and all terminals
- [x] **AC-6.9:** `like` and `in` work inside `transaction` blocks
- [x] **AC-6.10:** `examples/functional_tests/app.marreta` updated with `like`, `in`, and `#{}` native_query routes
- [x] **AC-6.11:** `test.sh` verifies all new routes pass end-to-end against live Postgres

**Out of scope (explicit deferral):**
- MySQL, SQLite (add new `DbDriver` impl when needed, no interpreter changes)
- `doc.*` — v0.6.0 (same pattern, `DocDriver` trait)
- `cache.*` — v0.8.0 (`CacheDriver` trait)
- Migrations, schema introspection
- OR filters, subqueries (→ `native_query`)
- Multi-level joins (→ `native_query`)

---

## Architecture

```
src/
├── db/
│   ├── mod.rs            # DbEngine enum, re-exports
│   ├── driver.rs         # DbDriver trait, FilterMap, JoinOptions, QueryBuilder state
│   ├── postgres.rs       # PostgresDriver : DbDriver  (sqlx::PgPool)
│   └── query_builder.rs  # parses filter maps → FilterClause list; builds SQL + param list
├── interpreter/
│   ├── mod.rs            # now async; holds Arc<dyn DbDriver>
│   └── builtins/
│       └── db.rs         # evaluates db.TABLE.OP and QueryBuilder pipeline steps
└── config.rs             # reads MARRETA_DB_* env vars
```

### `DbDriver` trait

```rust
#[async_trait]
pub trait DbDriver: Send + Sync {
    // Direct operations
    async fn save(&self, table: &str, data: Map) -> Result<Map>;
    async fn find(&self, table: &str, id: Value) -> Result<Option<Map>>;
    async fn find_all(&self, table: &str, filters: FilterMap) -> Result<Vec<Map>>;
    async fn update_by_id(&self, table: &str, id: Value, data: Map) -> Result<()>;
    async fn delete_by_id(&self, table: &str, id: Value) -> Result<()>;

    // Pipeline terminals (receive fully-built QueryBuilder state)
    async fn query_fetch(&self, q: &QueryState) -> Result<Vec<Map>>;
    async fn query_fetch_one(&self, q: &QueryState) -> Result<Option<Map>>;
    async fn query_count(&self, q: &QueryState) -> Result<i64>;
    async fn query_exists(&self, q: &QueryState) -> Result<bool>;
    async fn query_update(&self, q: &QueryState, data: Map) -> Result<()>;
    async fn query_delete(&self, q: &QueryState) -> Result<()>;

    // Native query
    async fn native_query(&self, sql: &str, params: Vec<Value>) -> Result<Vec<Map>>;

    // Transactions
    async fn begin_transaction(&self) -> Result<Box<dyn DbTransaction>>;
}
```

`DbEngine` holds `Arc<dyn DbDriver>`. `PostgresDriver` is the first impl. Future drivers (`MongoDriver`, etc.) implement their respective traits (`DocDriver`) in separate modules — the interpreter dispatches based on the namespace (`db` vs `doc`).

### `QueryState` — the lazy QueryBuilder

`Value::QueryBuilder(QueryState)` is a new `Value` variant holding accumulated pipeline clauses:

```rust
pub struct QueryState {
    pub table: String,
    pub namespace: Namespace,         // Db | Doc (for future doc.* pipeline)
    pub filters: Vec<FilterClause>,
    pub joins: Vec<JoinClause>,
    pub order_by: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub struct FilterClause {
    pub column: String,
    pub op: FilterOp,   // Eq, Gt, Gte, Lt, Lte, Ne, Like, In
    pub value: Value,
}

pub struct JoinClause {
    pub kind: JoinKind,  // Inner | Left
    pub table: String,
    pub on: String,      // FK column on left table
}
```

Pipeline step functions (`where`, `join`, `left_join`, `order_by`, `limit`, `offset`) receive a `Value::QueryBuilder`, clone-mutate the `QueryState`, and return the updated `Value::QueryBuilder`. They are pure — no I/O until a terminal is reached.

### `query_builder.rs` — filter parsing from expressions

`where(...)` arguments arrive as a `Vec<Expr>` from the parser. Each is walked to produce a `FilterClause`:

```rust
pub fn parse_where_args(args: &[Expr]) -> Result<FilterMap, MarretaError> {
    // Key-value pair (map syntax):
    //   Expr::MapPair("status", String("active"))
    //   → FilterClause { column: "status", op: Eq, value: "active" }
    //
    // Binary expression:
    //   BinaryOp(Identifier("total"), Gt, Integer(1000))
    //   → FilterClause { column: "total", op: Gt, value: 1000 }
    //
    //   BinaryOp(Identifier("name"), Like, String("João%"))
    //   → FilterClause { column: "name", op: Like, value: "João%" }
    //
    //   BinaryOp(Identifier("status"), In, List([...]))
    //   → FilterClause { column: "status", op: In, value: List }
    //
    // Anything else → startup error: "unsupported filter expression in where(): ..."
    // "order_by", "limit", "offset" as map-pair keys → extracted as reserved fields
}
```

`like` and `in` are added to the lexer/parser as **context keywords**: recognized as infix binary operators only when parsing `where(...)` argument lists. They are not added to the global reserved word list and do not conflict with identifiers named `like` or `in` elsewhere in the language.

### `native_query` — interpolation extraction

At evaluation time, the interpreter resolves `#{}` expressions in the SQL string before passing to the driver. The resolved values are separated from the SQL string and passed as a `Vec<Value>` parameter list. The driver binds them as `$1`, `$2`, ... in the prepared statement.

```
Input:  "SELECT * FROM users WHERE email = '#{email}' AND active = true"
After:  sql   = "SELECT * FROM users WHERE email = $1 AND active = true"
        params = [Value::String("ana@example.com")]
```

### `transaction` block — AST and execution

```rust
// AST
Statement::Transaction { body: Vec<Statement> }

// Execution
let mut tx = driver.begin_transaction().await?;
for stmt in &body {
    if let Err(e) = interpreter.execute_stmt_with_tx(stmt, &mut tx).await {
        tx.rollback().await.ok();
        return Err(e);
    }
}
tx.commit().await?;
```

Nesting detection happens at **parse time**: if a `transaction` block is encountered while already inside a `transaction`, the parser emits a startup error — the server does not start.

### Async interpreter refactor (Phase 0)

The current `fn evaluate` and `fn execute` are synchronous. The refactor:

1. Add `async` to all `evaluate` / `execute` signatures
2. Update all recursive calls to `.await`
3. Replace the `for` loop in `Expression::Broadcast` with:
   ```rust
   let futures: Vec<_> = targets.iter()
       .map(|t| self.apply_pipeline_value(&val, t))
       .collect();
   let results = futures::future::try_join_all(futures).await?;
   Ok(Value::List(results))
   ```
4. HTTP route handlers (already on tokio) call `.await` on the interpreter — no change needed there
5. REPL runs on `tokio::main` — add `.await`

Result order of `*>>` is preserved (same order as targets in source), even though execution is concurrent.

---

## Acceptance Criteria

### Phase 0 — Parallel `*>>` ✓ COMPLETE

- [x] **AC-0.1:** `Interpreter` is `Clone` — each `*>>` branch receives an independent fork; interpreter methods remain synchronous (threads, not async)
- [x] **AC-0.2:** `*>>` runs all targets concurrently via `std::thread::spawn` per branch, joined in declaration order
- [x] **AC-0.3:** `*>>` result is `Value::List` in declaration order regardless of completion order
- [x] **AC-0.4:** Panic in any `*>>` branch propagates as `MarretaError::TypeError` with descriptive message
- [x] **AC-0.5:** 473 → 479 tests passing; 6 new tests added
- [x] **AC-0.6:** 3 functional routes in `examples/parallel/` validate all combinations end-to-end
- [x] **AC-0.7 (discovered):** `*>>` does NOT implicitly iterate over List inputs — full value passed to each branch. `apply_broadcast_value()` added distinct from `apply_pipeline_value()`. Semantic distinction tested and documented.

### Phase 1 — Infrastructure ✓ COMPLETE

- [x] **AC-1.1:** `MARRETA_DB_PROVIDER=postgres` + valid `MARRETA_DB_URL` → pool connects at startup
- [x] **AC-1.2:** Missing `MARRETA_DB_URL` → descriptive startup error, server does not start
- [x] **AC-1.3:** Unsupported `MARRETA_DB_PROVIDER` value → descriptive startup error
- [x] **AC-1.4:** `db.*` calls when no DB is configured → `HTTP 500` with a clear message, no panic *(fulfilled in Phase 2 via AC-2.8)*
- [x] **AC-1.5:** Connection pool is shared across requests — `Arc<dyn DbDriver>` held in `DbEngine`, cloned into each request interpreter via `with_db()`
- [x] **AC-1.6 (new):** 22 unit tests for `query_builder` — all filter operators, joins, ORDER BY, LIMIT/OFFSET, IN list expansion, UPDATE/DELETE SQL generation; 501 tests passing

### Phase 2 — Direct CRUD ✓ COMPLETE

- [x] **AC-2.1:** `db.TABLE.save(map)` inserts a row, returns full record via `RETURNING *`
- [x] **AC-2.2:** `db.TABLE.find(id)` returns map when row exists, `null` when not found
- [x] **AC-2.3:** `db.TABLE.find_all()` returns all rows as `Value::List`
- [x] **AC-2.4:** `db.TABLE.find_all(key: val)` filters by equality (named args preserve key names)
- [x] **AC-2.5:** `db.TABLE.update(id, partial)` updates only listed fields via `RETURNING *`
- [x] **AC-2.6:** `db.TABLE.delete(id)` removes the row, returns `Boolean`
- [x] **AC-2.7 (new):** `db` evaluates to `Value::DbNamespace`; `db.TABLE` evaluates to `Value::DbTable` — no new AST nodes required (existing PropertyAccess + MethodCall intercepted in interpreter)
- [x] **AC-2.8 (new):** All operations return descriptive error when no DB engine is configured (AC-1.4 fulfilled)
- [x] **AC-2.9 (new):** 10 unit tests — DbNamespace/DbTable intermediate values, Display, all 5 ops return correct error without engine; 608 tests passing

### Phase 3 — Pipeline ✓ COMPLETE

- [x] **AC-3.0:** `map`/`keep` on a QueryBuilder without a terminal → descriptive error "did you forget >> fetch or >> fetch_one?" (runtime; startup static analysis deferred per Design Watch Points)
- [x] **AC-3.0b:** After any terminal, the returned `Value` (List, Map, Integer, Boolean) flows normally into the language pipeline — `map`, `keep`, tasks, `*>>` all work as usual
- [x] **AC-3.1:** `db.TABLE` evaluates to `Value::DbTable`; first `>>` promotes to `Value::QueryBuilder` automatically — no DB call at any accumulation step
- [x] **AC-3.2:** `>> where(key: val)` accumulates equality filter via named arg
- [x] **AC-3.3:** `>> where(col > val)` extracts column, operator, value from `BinaryOp` AST
- [x] **AC-3.4:** `>`, `>=`, `<`, `<=`, `!=`, `==` inside `where(...)` map to correct `FilterOp`
- [x] **AC-3.5:** `like` and `in` as pipeline steps (`>> like("col", "pat")`, `>> in("col", list)`) — cleaner than context keywords inside `where()`; no lexer surgery needed *(implemented in Phase 6)*
- [x] **AC-3.5b:** `like` and `in` are not reserved words — they are pipeline step names only *(fulfilled by pipeline-step approach)*
- [x] **AC-3.5c:** Unsupported operator (e.g. `+`) in `where()` → descriptive error; non-identifier LHS → descriptive error
- [x] **AC-3.5d:** `>> select("col1", "col2")` adds SELECT projection (computed alias with named args deferred to Phase 5)
- [x] **AC-3.6:** `>> join("table", on: "fk")` → INNER JOIN accumulation
- [x] **AC-3.7:** `>> left_join("table", on: "fk")` → LEFT JOIN accumulation
- [x] **AC-3.8:** Join results have table-prefixed keys *(query_builder.rs generates prefixed SQL; validated at DB integration test level in Phase 5)*
- [x] **AC-3.9:** `>> order_by("col desc")` accumulates ORDER BY clause
- [x] **AC-3.10:** `>> limit(n)` / `>> offset(n)` accumulate pagination clauses
- [x] **AC-3.11:** `>> fetch` executes and returns `List[Map]`
- [x] **AC-3.12:** `>> fetch_one` executes and returns `Map` or `null`
- [x] **AC-3.13:** `>> count` executes and returns `Integer`
- [x] **AC-3.14:** `>> exists` executes and returns `Boolean`
- [x] **AC-3.15:** `>> update({...})` executes bulk UPDATE on all matching rows, returns rows affected
- [x] **AC-3.16:** `>> delete` executes bulk DELETE on all matching rows, returns rows affected
- [~] **AC-3.17:** `Value::QueryBuilder` without terminal → startup static analysis *(explicitly deferred post-v0.5.0; runtime type error already raised — "pipeline returned a QueryBuilder … did you forget >> fetch?")*
- [x] **AC-3.18 (new):** 20 unit tests — all accumulation steps (pure, no DB), all terminal errors without engine, error hints; 627 tests passing

### Phase 4 — Native query + transactions ✓ COMPLETE

- [x] **AC-4.1:** `db.native_query("sql with #{expr}")` executes and returns `List[Map]`; `#{}` expressions are extracted from the raw AST string (before language interpolation), evaluated, and bound as `$1/$2` prepared params — positional style removed in Phase 6
- [x] **AC-4.2:** All PG types map correctly to `Value` variants (Integer, Float, String, Boolean, Null) *(validated in Phase 5 integration tests)*
- [x] **AC-4.3:** `#{}` interpolation in `native_query` produces `$1/$2` prepared statement params, not string concatenation
- [x] **AC-4.4:** `transaction` block issues `BEGIN` → body execution → `COMMIT`
- [x] **AC-4.5:** `transaction` block issues `ROLLBACK` when any statement fails, then re-raises the error
- [x] **AC-4.6:** Pipeline queries (`>> fetch`, `>> delete`, `>> update`) can appear inside `transaction` body (same interpreter, same connection pool — atomicity via explicit BEGIN/COMMIT/ROLLBACK)
- [x] **AC-4.7:** Nested `transaction` blocks are rejected at **parse time** via `inside_transaction: bool` on `Parser` struct — server does not start
- [x] **AC-4.8:** `*>>` inside a `transaction` block raises a runtime error ("*>> (broadcast) is not allowed inside a transaction block")
- [x] **AC-4.9:** `*>>` outside a `transaction` can freely combine parallel DB queries *(validated in Phase 5 — Section 19 functional tests, 130/130 passing)*
- [x] **AC-4.10 (new):** `TokenKind::Transaction` / `Statement::Transaction` — new keyword + AST node; no breaking changes to existing statements
- [x] **AC-4.11 (new):** `inside_transaction: bool` on `Interpreter` struct — guards `*>>` at runtime
- [x] **AC-4.12 (new):** `db.native_query(non_string)` → descriptive type error; missing first arg → descriptive error
- [x] **AC-4.13 (new):** 5 unit tests (no-engine guard, nested transaction parse error, broadcast-in-transaction); 536 lib tests passing

---

## Implementation Steps

### Phase 0 — Parallel `*>>` ✓ COMPLETE

1. ✓ `Interpreter` derives `Clone` — fork-per-branch chosen over async refactor (sync/CPU-bound interpreter; `sqlx` blocking wrappers available for Phase 1; no need to make entire call stack async)
2. ✓ `apply_broadcast_value()` added — passes input value as-is, no implicit list iteration (distinct from `apply_pipeline_value()`)
3. ✓ `Expression::Broadcast` handler: sequential `for` loop → `std::thread::spawn` per branch using `apply_broadcast_value`, handles joined in declaration order
4. ✓ 479 tests passing; 6 new tests covering all `*>>` + pipeline combinations
5. ✓ `examples/parallel/` — 3 functional HTTP routes validated end-to-end: scalar broadcast, list broadcast, broadcast→pipeline chain

### Phase 1 — Infrastructure ✓ COMPLETE

7. ✓ `Cargo.toml`: `sqlx = { version = "0.8", features = ["postgres", "runtime-tokio-rustls"] }`, `async-trait = "0.1"` (no `macros` feature needed — dynamic query binding used instead)
8. ✓ `src/db/driver.rs` — `DbDriver` trait, `QueryState`, `FilterClause`, `FilterOp`, `JoinClause`, `JoinKind` (`DbTransaction` deferred to Phase 4)
9. ✓ `src/db/query_builder.rs` — `build_select`, `build_update`, `build_delete`, `filters_from_equality_map`; 22 unit tests
10. ✓ `src/db/postgres.rs` — `PostgresDriver` wrapping `sqlx::PgPool`; PG type → `Value` mapping; `bind_and_fetch!` / `bind_and_execute!` macros for dynamic param binding
11. ✓ `src/db/mod.rs` — `DbEngine { driver: Arc<dyn DbDriver>, provider: DbProvider }`, `from_config()` async initializer
12. ✓ `src/config.rs` — `db_provider` + `db_url` fields; read from `marreta.env` or env vars
13. ✓ `src/main.rs` — `DbEngine::from_config()` called at startup on tokio runtime; startup error on invalid config; "DB connected (postgres)" log; engine passed to `ServerConfig`
14. ✓ `src/server.rs` — `ServerConfig.db_engine`; `Arc<Option<DbEngine>>` threaded into `register_route` → `execute_route`; interpreter receives engine via `with_db()`

### Phase 2 — Direct CRUD interpreter binding ✓ COMPLETE

15. ✓ `Value::DbNamespace`, `Value::DbTable(String)`, `Value::QueryBuilder(Box<QueryState>)` added to `src/value.rs` — no new AST nodes needed; existing PropertyAccess + MethodCall intercepted
16. ✓ `Identifier("db")` → `Value::DbNamespace`; `PropertyAccess(DbNamespace, table)` → `Value::DbTable`
17. ✓ `MethodCall(DbTable, method, args)` → `dispatch_db_direct()` — dispatches to driver via `tokio::Handle::block_on`
18. ✓ `args_to_equality_filters()` reads raw `Argument::Named` AST nodes (not pre-evaluated values) to preserve key names for `find_all` filters
19. ✓ Free helpers: `value_to_db_row`, `db_row_to_value`; 10 new unit tests

### Phase 3 — Pipeline interpreter binding ✓ COMPLETE

20. ✓ `evaluate_pipeline_stage` extended: `Value::DbTable` promoted to `Value::QueryBuilder` on first `>>` — no new AST/parser changes required
21. ✓ `apply_query_pipeline_stage` handles all accumulating steps and terminals via `FunctionCall` name dispatch + bare `Identifier` for terminals
22. ✓ `parse_where_args` — named args → equality filters; positional `Argument` → `extract_filter_from_expr` (BinaryOp AST walk)
23. ✓ `extract_filter_from_expr` — maps `BinaryOperator::Greater/GreaterEqual/Less/LessEqual/NotEqual/Equal` → `FilterOp`; rejects arithmetic ops and non-identifier LHS with descriptive errors
24. ✓ `extract_named_string_arg` — extracts `on: "fk"` from argument list for join calls
25. ✓ `require_db_engine` — shared helper used by all terminals
26. ✓ `like`/`in` context operators deferred (requires `BinaryOperator` enum extension + lexer changes)

### Phase 4 — Native query + transactions

23. Add `Expr::DbNativeQuery { sql_template: String, params: Vec<Expr> }` to AST — parser extracts `#{}` interpolations at parse time
24. Implement `native_query` in `PostgresDriver` — bind params as `$1/$2`, map `PgRow` → `Value::Map`
25. Add `Statement::Transaction { body: Vec<Statement> }` to AST
26. Parse `transaction\n    BODY` (indented block); detect nesting at parse time
27. Implement transaction evaluation with rollback-on-error

### Phase 5 — Ecommerce example update

28. Update `examples/ecommerce/app.marreta` to use `db.*` for orders and products
29. Update `examples/ecommerce/README.md` — Postgres setup, environment variables
30. Add `postgres` service to `tests/load/docker-compose.yml`; update `run.sh` health check to include Postgres

---

## Dependencies

| Crate | Version | Purpose |
|---|---|---|
| `sqlx` | 0.7 | Async PostgreSQL driver, `PgPool`, prepared statements, row mapping |
| `async-trait` | 0.1 | Async methods in traits (`DbDriver`) |
| `futures` | 0.3 | `try_join_all` for parallel `*>>` (likely already transitive) |
| `tokio` | 1 | Already a dependency — async runtime |

---

## Structural Notes for Future Drivers

This architecture is explicitly designed so that adding `doc.*` (v0.6.0) and `cache.*` (v0.8.0) requires **zero changes to the interpreter core**:

- **`doc.*` (MongoDB, v0.6.0):** Create `src/doc/` with `DocDriver` trait (same CRUD + filter suffix API, no `native_query`, no `transaction`). Add `Value::DocQueryBuilder` or reuse `QueryState` with `Namespace::Doc`. Register under `MARRETA_DOC_PROVIDER` / `MARRETA_DOC_URL`. The interpreter dispatches based on namespace prefix.

- **`cache.*` (Redis, v0.8.0):** Create `src/cache/` with `CacheDriver` trait: `get`, `set`, `set_ttl`, `delete`, `exists`, `incr`, `decr`. No query builder, no filters. Register under `MARRETA_CACHE_PROVIDER` / `MARRETA_CACHE_URL`.

- **Additional relational drivers (MySQL, SQLite):** Implement `DbDriver` in a new file under `src/db/`. Add a new variant to `DbEngine`. No parser or interpreter changes.

The three driver traits are fully independent — no shared base trait is needed or desirable. The separation in the language maps directly to the separation in the engine.
