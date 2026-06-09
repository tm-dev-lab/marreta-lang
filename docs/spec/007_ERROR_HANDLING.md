# MarretaLang — Implementation Plan: Error Handling (v0.6.0)

> Status: Delivered.

---

## Context and Design Rationale

This plan addresses three problems that form a coherent system. Designing them independently would produce an incoherent developer experience — they must ship together.

### The three problems

**1. No intentional domain error signal — `raise`**

Currently the only way to signal an error from inside a task is `fail CODE, MSG`, which carries HTTP semantics. This forces tasks to know what HTTP status code to return — a violation of separation of concerns. A domain validation task should not need to know whether its caller is a route that returns 422 or 409.

`raise MSG` signals that something went wrong without coupling to HTTP. The caller (route or pipeline `rescue`) decides the HTTP response.

**2. No structured error recovery in pipelines — `rescue`**

A pipeline step failure currently terminates the request as HTTP 500 with no developer control. There is no way to:
- Catch a DB failure and try a fallback
- Silently absorb a non-critical queue push failure
- Respond with a structured, tailored error message when infrastructure fails

`rescue` implements Railway Oriented Programming: any error at any preceding pipeline step diverts to the rescue handler, bypassing all intermediate steps.

**3. Rust internals leaking into error output — Marreta Error Identity**

Every external driver (sqlx, mongodb, redis, reqwest, tokio) can produce errors that reference Rust types, file paths, or driver-specific codes. These leak into developer-facing output: logged errors, HTTP response bodies, and `rescue` error values.

A MarretaLang developer should never see `sqlx::Error`, `src/interpreter.rs:247`, or `23505` (a Postgres constraint code). All errors must speak MarretaLang.

---

### Why `raise` and `fail` are distinct

`fail` is an HTTP response mechanism — it terminates the request immediately with a specific status code. It is not error handling; it is a deliberate response decision.

`raise` is a domain error mechanism — it signals that an operation failed, propagates up the call stack, and can be caught by `rescue`. If uncaught at the route level, it becomes HTTP 500.

The separation gives tasks the ability to signal errors without coupling to HTTP semantics. The route (or pipeline `rescue`) decides the HTTP response. This is the correct separation of concerns for a language that treats HTTP as a module, not a global assumption.

### Why `rescue` is a railway (not per-step)

The alternative considered was per-step inline rescue: `>> step_a rescue fail 500, "A"`. Explicitly rejected because it breaks the linear reading of the pipeline — the developer must track two levels of logic per line. This is the Go `if err != nil` pattern in pipeline clothing.

`rescue` is a terminal capture step. One `rescue` at the end of a pipeline catches any failure from any preceding step. For granular per-step recovery, the idiomatic approach is to isolate the risky step in a task with its own `rescue`. This preserves pipeline linearity.

### Why `error.code` uses semantic codes

Exposing HTTP or database error codes (like `23505` for Postgres unique violations) in the `error` Map would couple the developer's error handling logic to infrastructure internals. A unique constraint violation is `db_error`, not `23505`. The developer writes infrastructure-agnostic code; the engine translates.

---

## Scope

### Phase 1 — `raise` keyword ✅

- `TokenKind::Raise` added to lexer + `keyword_lookup()`
- `Statement::Raise { message: Expr, condition: Option<Expr>, line, column }` in AST
- `parse_raise()` in parser: `raise MSG` and `raise MSG if CONDITION` (suffix modifier, consistent with conditional assignment style)
- `require X else raise MSG` supported — extends existing `parse_require()` to accept `raise` in the `else` branch
- `MarretaError::RaiseError { message: String }` variant for propagation
- Interpreter: evaluates message expression, wraps in `RaiseError`, propagates as `Err`
- Uncaught `RaiseError` at route handler level → HTTP 500 with `{ "error": "<message>" }` in Marreta format — no Rust content

### Phase 2 — `rescue` pipeline step ✅

- `TokenKind::Rescue` added to lexer + `keyword_lookup()`
- `PipelineStage::Rescue { handler: RescueHandler }` in AST
- `RescueHandler` enum: `FailExpr { code: Expr, message: Expr }`, `TaskCall { name: String, args: Vec<Argument> }`, `Block { body: Vec<Statement> }`
- `>> rescue fail CODE, MSG` — one-liner `FailExpr` form
- `>> rescue task_name(args)` — `TaskCall` form
- `>> rescue` with indented block — `Block` form (consistent with `map` block syntax)
- Interpreter: preceding pipeline stages wrapped in error boundary (`match`); on any `Err(MarretaError)`, `error` Map is built and injected into scope, then handler is executed
- `error` Map automatically available in all rescue forms: `{ message, op, code }`
- `rescue` step is silently skipped when no error occurs

### Phase 3 — `rescue` as expression modifier ✅

**Design note:** `Expression::Rescue` uses `handler: Box<Expression>` (a free expression), while `PipelineStage::Rescue` uses a structured `RescueHandler` enum. These are intentionally separate — pipeline rescue is a stage-capture mechanism (different semantics from value substitution). Two code paths in the interpreter, but each is simple and clear. Do not unify them.

- `Expression::Rescue { expr: Box<Expression>, handler: Box<Expression> }` in AST
- Parser: `expr rescue handler` recognized as infix at lowest precedence (below pipeline `>>`)
- **Handler is any valid expression** — literals (`null`, `0`, `[]`, `{ key: val }`), task calls, `fail`, `reply`, or blocks
- Interpreter: evaluate `expr`; on error, evaluate `handler` with `error` injected into scope; if handler returns a value (not `fail`/`reply`), that value substitutes the failed expression result and execution continues
- `rescue null` and `rescue []` are valid: they silence the error and return the fallback value
- Same `error` Map available as in pipeline form

### Phase 4 — Marreta Error Identity ✅

- All `sqlx::Error` variants mapped in `postgres.rs` to named `MarretaError` variants with Marreta-style messages — no `sqlx::Error` propagates beyond the module boundary
- Global panic hook registered in `main.rs` via `std::panic::set_hook` — catches panics, emits `[marreta] Internal error` to stderr, returns HTTP 500 to the client — Rust panic output never visible
- All `MarretaError::Display` implementations audited: no Rust type names, no `src/` file paths, no crate names
- Log format standardized: `[marreta] <message>\n  → <file>:<line>` for all error output to stderr
- `error.code` semantic string codes defined — see table below
- Error log for uncaught runtime errors at the route handler includes the `.marreta` file and line number, not the Rust source location

### Phase 5 — Examples + E2E validation ✅

- `examples/functional_tests/app.marreta` updated with new routes covering all error handling scenarios
- `examples/ecommerce/routes/orders.marreta` updated to use `rescue` on the order save pipeline
- `test.sh` updated with E2E assertions for all new routes

---

## Architecture

### New AST nodes

```rust
// Statement
Statement::Raise {
    message: Expression,
    condition: Option<Expression>,
    line: usize,
    column: usize,
}

// Pipeline stage — structured RescueHandler (stage-capture semantics)
PipelineStage::Rescue {
    handler: RescueHandler,
}

enum RescueHandler {
    FailExpr { code: Expression, message: Expression },
    TaskCall  { name: String, args: Vec<Argument> },
    Block     { body: Vec<Statement> },
}

// Expression modifier — free Expression handler (value-substitution semantics)
Expression::Rescue {
    expr:    Box<Expression>,
    handler: Box<Expression>,  // any expression — literal, task call, fail, block
}
```

**Why two models for the same keyword:**
`PipelineStage::Rescue` captures a failure in a multi-step sequence and decides what the route does next (structured handler enum). `Expression::Rescue` substitutes a single failed value with a fallback (free expression). They share the `rescue` keyword but have different evaluation semantics — do not unify them into a single AST node.

### `error` Map in rescue scope

Injected into `Environment` whenever a rescue handler executes:

```rust
let error_map = Value::map_from(vec![
    ("message", Value::String(err.display_message())),  // no Rust details
    ("op",      Value::String(err.operation_name())),   // "db.users.save", "queue.push", etc.
    ("code",    Value::String(err.semantic_code())),    // see table below
]);
env.set("error", error_map);
```

**Semantic codes (`error.code`):**

| Code | Meaning | `MarretaError` variants |
|---|---|---|
| `raise_error` | Developer used `raise` keyword intentionally | `RaiseError` |
| `db_error` | Database operation failure | `DbError` |
| `type_error` | Type mismatch or wrong argument type | `TypeError` |
| `reference_error` | Undefined variable, task, property, or non-callable | `UndefinedVariable`, `UndefinedTask`, `PropertyNotFound`, `NotCallable` |
| `arity_error` | Wrong number of arguments to a task | `WrongArity` |
| `arithmetic_error` | Arithmetic fault (e.g. division by zero) | `DivisionByZero` |
| `io_error` | File system or I/O failure | `IoError`, `FileNotFound` |
| `config_error` | Startup-time conflict (routes, exports, schemas) | `RouteConflict`, `ExportConflict`, `CircularSchemaReference` |
| `infrastructure_error` | Queue, cache, or HTTP client failure | *(future driver modules)* |
| `runtime_error` | General interpreter or engine failure | all other variants |

`error.code` is never a Postgres error code, an HTTP status code, or a Rust error type name.

### Error translation layer (Marreta Error Identity)

Every external driver module boundary translates driver errors before propagation:

```rust
// postgres.rs — no sqlx::Error escapes this module
// Uses the driver's own message — no static text, no driver-specific code mapping.
fn translate_pg_error(err: sqlx::Error, table: &str, op: &str) -> MarretaError {
    let operation = format!("db.{}.{}", table, op);
    let message = match &err {
        sqlx::Error::Database(db_err) => db_err.message().to_string(),
        sqlx::Error::RowNotFound     => format!("record not found in '{}'", table),
        sqlx::Error::PoolTimedOut    => "database connection pool timed out".to_string(),
        sqlx::Error::PoolClosed      => "database connection pool is closed".to_string(),
        _                            => err.to_string(),
    };
    MarretaError::DbError { message, operation }
}
```

Same pattern applied to every future driver module (`mongodb.rs`, `redis.rs`, `http_client.rs`).

### Panic hook (main.rs)

```rust
std::panic::set_hook(Box::new(|info| {
    let msg = info.payload().downcast_ref::<&str>().unwrap_or(&"unexpected internal error");
    eprintln!("[marreta] Internal error: {}", msg);
    eprintln!("  → The engine encountered an unrecoverable condition.");
    eprintln!("  → Please report this at github.com/marreta-lang/marreta/issues");
}));
```

---

## Acceptance Criteria

### Phase 1 — `raise`

- [x] **AC-1.1:** `raise "message"` inside a task propagates as `RaiseError` with the evaluated message
- [x] **AC-1.2:** `raise "message" if condition` raises only when condition is truthy; no error when falsy
- [x] **AC-1.3:** `require X else raise MSG` raises when X is falsy — works identically to `require X else fail` but without HTTP code
- [x] **AC-1.4:** Uncaught `raise` reaching the route handler → HTTP 500 with `{ "error": "<message>" }` — body contains no Rust content
- [x] **AC-1.5:** `raise` inside a nested task propagates through the call stack to the nearest enclosing `rescue` or route handler
- [x] **AC-1.6:** Message expression supports string interpolation and variable references: `raise "Invalid total: #{order.total}"`
- [x] **AC-1.7:** `fail CODE, MSG` is unchanged — `raise` does not interfere with existing `fail` semantics

### Phase 2 — `rescue` pipeline step

- [x] **AC-2.1:** `>> rescue fail CODE, MSG` catches any error from any preceding pipeline step and terminates with the given HTTP response
- [x] **AC-2.2:** `>> rescue task_name` executes the named task when an error occurs; `error` Map is in scope
- [x] **AC-2.3:** `>> rescue` block executes the indented body on error; full language context available (DB, cache, tasks)
- [x] **AC-2.4:** `error.message`, `error.op`, `error.code` are available inside all rescue forms
- [x] **AC-2.5:** `error.code` is always one of the defined semantic codes — never a Postgres code or HTTP status
- [x] **AC-2.6:** `rescue` does NOT catch `fail CODE, MSG` — `fail` exits immediately and bypasses all `rescue` handlers
- [x] **AC-2.7:** No error in the pipeline → `rescue` step is silently skipped; output is the last pipeline value
- [x] **AC-2.8:** A `raise` inside a task called from a pipeline step is caught by the pipeline's `rescue`
- [x] **AC-2.9:** `rescue` block can call tasks, read/write cache, push to queue — full language context
- [x] **AC-2.10:** `fail` or `reply` inside a `rescue` block terminates the request immediately — it is NOT re-caught by any outer `rescue`

### Phase 3 — `rescue` expression modifier

- [x] **AC-3.1:** `expr rescue fail CODE, MSG` catches any error from `expr` evaluation
- [x] **AC-3.2:** `task_call(args) rescue task_name` calls the recovery task on failure; `error` in scope
- [x] **AC-3.3:** `error.message`, `error.op`, `error.code` available in expression rescue context
- [x] **AC-3.4:** Successful expression evaluation — rescue handler is not invoked
- [x] **AC-3.5:** `rescue` modifier has lowest precedence — does not conflict with arithmetic, comparison, or pipeline operators
- [x] **AC-3.6:** `rescue` handler accepts any expression as fallback value: `rescue null`, `rescue 0`, `rescue []`, `rescue { key: val }` — the fallback value substitutes the result and execution continues normally

### Phase 4 — Marreta Error Identity

- [x] **AC-4.1:** `sqlx::Error` never appears in any developer-visible output (HTTP body, log, `error` Map)
- [x] **AC-4.2:** Rust file paths and line numbers never appear in any output
- [x] **AC-4.3:** Database driver-specific codes (e.g. `23505`) never appear — translated to `error.message` in plain language
- [x] **AC-4.4:** A Rust panic produces `[marreta] Internal error` to stderr — no Rust panic output visible to the developer
- [x] **AC-4.5:** All log output to stderr follows `[marreta] <message>\n  → <file>:<line>` format with the `.marreta` source location
- [x] **AC-4.6:** Uncaught runtime error at route level logs in Marreta format and responds with `{ "error": "..." }` — no Rust leakage in the HTTP response body
- [x] **AC-4.7:** `error.code` values are restricted to the defined semantic code set

### Phase 5 — Examples + E2E

- [x] **AC-5.1:** `functional_tests` route raises uncaught → verified HTTP 500 body contains no Rust content
- [x] **AC-5.2:** `functional_tests` pipeline rescue routes → verified correct responses and `error.message` content
- [x] **AC-5.3:** `functional_tests` expression modifier rescue → verified
- [x] **AC-5.4:** `functional_tests` `require X else raise MSG` pattern → verified correct propagation and rescue
- [x] **AC-5.5:** `ecommerce` orders pipeline uses `rescue` → manual E2E test passes
- [x] **AC-5.6:** All existing tests continue to pass — zero regressions

---

## Implementation Steps

### Phase 1 — `raise`

1. Add `Raise` to `TokenKind` in `token.rs`; add `"raise"` to `keyword_lookup()`
2. Add `Statement::Raise { message, condition, line, column }` to `ast.rs`
3. Add `MarretaError::RaiseError { message: String }` to `error.rs`; Display: `"raise: {message}"`
4. Add `parse_raise()` to `parser.rs`: `raise EXPR` and `raise EXPR if EXPR`
5. Extend `parse_require()` to handle `else raise MSG` branch (produces `Statement::Raise`) — note: `require X else rescue handler` is NOT supported and must NOT be added; only `raise` is valid in the `else` branch (rescue is a pipeline/expression concept, not a require branch)
6. Add `Statement::Raise` arm to `execute_statement()` in `interpreter.rs` — evaluates message, returns `Err(RaiseError)`
7. In route handler (`server.rs`): `RaiseError` → HTTP 500 with Marreta-formatted body, no Rust content in response or log
8. 8+ unit tests covering all AC-1.x criteria

### Phase 2 — `rescue` pipeline step

9. Add `Rescue` to `TokenKind` in `token.rs`; add `"rescue"` to `keyword_lookup()`
10. Add `PipelineStage::Rescue { handler }` and `RescueHandler` enum to `ast.rs`
11. Extend `parse_pipeline()` to recognize `>> rescue` as a terminal error-capture stage
12. Add `marreta_error_to_map(e: &MarretaError) -> Value` helper — builds `error` Map with `message`, `op`, `code`
13. Implement `MarretaError::operation_name()` and `MarretaError::semantic_code()` methods on `error.rs`
14. In `interpreter.rs` `apply_pipeline_value()`: wrap preceding stage execution in Rust `match`; on `Err(e)`, build `error` Map, inject into scope, execute `RescueHandler`
15. 10+ unit tests covering all AC-2.x criteria

### Phase 3 — `rescue` expression modifier

16. Add `Expression::Rescue { expr, handler }` to `ast.rs`
17. Add parsing of `expr rescue handler` in `parser.rs` — lowest precedence infix operator
18. Add `Expression::Rescue` evaluation arm to `evaluate()` in `interpreter.rs`
19. 6+ unit tests covering all AC-3.x criteria

### Phase 4 — Marreta Error Identity

20. Audit `postgres.rs` — wrap every `sqlx::Error` branch in a `translate_sqlx_error()` function; no raw `sqlx::Error` propagates out
21. Register custom panic hook in `main.rs` via `std::panic::set_hook`
22. Audit all `MarretaError::Display` impls — remove any Rust type names, `src/` paths, crate references
23. Add `ErrorCode` enum or `&'static str` constants; implement `semantic_code()` on `MarretaError`
24. Standardize error log calls in `server.rs`: `[marreta] <message>\n  → <file>:<line>` format
25. 8+ unit tests covering all AC-4.x criteria

### Phase 5 — Examples + E2E

26. Update `examples/functional_tests/app.marreta` — sections for raise, rescue pipeline, rescue expression, require+raise
27. Update `examples/ecommerce/routes/orders.marreta` — add `rescue` to order save pipeline
28. Update `test.sh` — assertions for all new functional test routes
29. Run full test suite — 0 regressions required

---

## Dependencies

No new crate dependencies. All changes are within the existing interpreter, parser, and error modules. The panic hook uses `std::panic` from the Rust standard library.

---

## Structural Notes for Future Modules

Every future driver module (`doc/mongodb.rs`, `cache/redis.rs`, `http_client.rs`, `queue/rabbitmq.rs`) must follow the same error translation pattern established in Phase 4:

- Define a `translate_<driver>_error()` function at the module boundary
- Map all driver-specific error types to `MarretaError` variants
- No driver error type propagates beyond its module
- All `MarretaError` variants produced by the module must implement `operation_name()` and `semantic_code()`

This pattern must be enforced as a code review rule for every new driver PR.
