# 022 — Runtime Error Hardening

Status: Delivered

## Delivery notes

Landed on branch `feature/runtime-error-hardening-022`:

- `df05d91` — initial Marreta runtime trace support (`MarretaFrame`, frame stack)
- `74faf75` — preserve Marreta trace context in runtime errors
- `97b1378` — normalize CLI runtime error surfaces (Rule 6, startup paths)
- `8bb1479` — normalize consumer bootstrap runtime errors
- `aa54739` — preserve expression statement source locations (Phase 2 precision)
- `5d57bb9` — harden rescue frame bookkeeping; Phase C/D/H test coverage
- `1874fef` — Phase G panic-hook format coverage

Exit-criteria map:

| Criterion | Covered by |
|---|---|
| authored `reply`/`fail` unchanged | `5d57bb9` (unit + functional body-purity asserts) |
| uncaught engine-originated semantic envelope | `74faf75` |
| stderr trace for uncaught failures | `df05d91`, `74faf75`, `5d57bb9` (Phase C chain) |
| `.marreta` file/line in trace | `df05d91`, `aa54739` |
| rescued errors emit no top-level trace | `5d57bb9` (frame-depth truncate) |
| startup failures use Marreta-formatted stderr only | `97b1378`, `8bb1479` |
| panic fallback Marreta-formatted, no Rust leakage | `1874fef` |
| success path silent | `5d57bb9` (Phase H silence assertion); overhead validation is tracked in `022b_TRACE_PERF_AND_ERGONOMICS.md` |
| core runtime stops relying on panic | audited: remaining `panic!()` calls live only in AST downcast helpers (`src/ast.rs`), which are unreachable from user input and treated as legitimate internal invariants caught by the panic hook |


## Goal

Strengthen MarretaLang runtime error handling so that:

- Rust internals never leak into developer-facing or client-facing error surfaces
- runtime-generated errors remain diagnostically useful
- `reply` and `fail` stay fully user-authored and are never wrapped or mutated by the engine
- uncaught runtime errors include a Marreta-native execution trace instead of a Rust stack trace

## Problem

Marreta already has the right error philosophy:

- semantic `error.code`
- translated infrastructure/runtime errors
- panic hook to avoid raw Rust panic output

But the current model still has two risks:

1. Some runtime failures can still be overly generic after translation
2. Developers may lose execution context when an uncaught runtime error happens

If the engine only translates Rust/driver errors into generic messages, the language becomes safe but opaque. If it exposes raw internals, it becomes noisy and leaks implementation details. Marreta needs a third layer: a language-native execution trace.

## What counts as an engine-originated failure

This plan uses **engine-originated failure** to mean failures produced by the
runtime rather than explicitly authored by the user.

Examples:

- interpreter evaluation errors
- uncaught `raise`
- runtime type errors
- infrastructure driver errors (`db`, `doc`, `cache`, `queue`, `http_client`)
- uncaught task/route/consumer execution failures
- unexpected internal failures that reach the panic hook

Not included:

- explicit `fail CODE, BODY`
- explicit `reply CODE, BODY`
- explicit `reply CODE as schema, BODY`
- `raise` that is successfully handled by `rescue`

## Principles

### 1. User-authored responses are authoritative

These constructs are response-authoring mechanisms, not runtime error envelopes:

- `reply CODE, BODY`
- `reply CODE as schema, BODY`
- `fail CODE, BODY`

When the user writes a response body explicitly, the engine must not inject extra fields such as:

- `code`
- `trace`
- `runtime`
- `details`

Examples:

```marreta
fail 404, { error: "user not found" }
reply 400, { reason: "invalid payload" }
```

The HTTP body must remain exactly what the user authored.

### 2. Only engine-authored failures get the default runtime envelope

When a runtime/infrastructure/interpreter error is not handled by user code, the engine may build a standard HTTP error response.

Example shape:

```json
{
  "error": "database operation failed",
  "code": "db_error"
}
```

This envelope applies only to uncaught engine-originated failures.

### 3. Rust stack traces must never be shown

Rust panic output, driver type names, crate paths, and raw stack traces must never be emitted to:

- HTTP response bodies
- `rescue` maps
- normal developer-facing stderr output

### 4. Marreta-native execution trace must exist

For uncaught runtime errors, the engine must print a Marreta-native trace that helps the developer understand the evaluation path without exposing Rust internals.

## Marreta Execution Trace

The runtime should maintain a lightweight execution stack of Marreta frames.

Possible frame kinds:

- route
- consumer
- task
- pipeline stage
- db operation
- doc operation
- cache operation
- queue operation
- http client operation

Example stderr output:

```text
[marreta] db_error: database operation failed
[marreta] trace:
  at route POST /checkout (routes/checkout.marreta:3)
  at task create_order (tasks/orders.marreta:12)
  at task persist_order (tasks/orders.marreta:27)
  at db.orders.save (tasks/orders.marreta:31)
```

The trace must be language-native, compact, and readable. It is not a Rust backtrace.
File and line information should be a first-class goal of the trace model.
If some specific runtime frame cannot yet provide source metadata, the engine
may temporarily fall back to a frame without location, but the direction of the
feature is to include `.marreta` file and line information wherever possible.

## Surfaces

### HTTP response for uncaught engine errors

Allowed:

```json
{
  "error": "database operation failed",
  "code": "db_error"
}
```

Not allowed:

- raw Rust panic message
- raw SQLx/MongoDB/Lapin/Redis error strings as-is
- Rust type names
- crate/module paths
- Marreta execution trace inside the HTTP body by default

### stderr / runtime logs

Allowed:

- semantic message
- semantic error code
- operation name when applicable
- Marreta execution trace

Not allowed:

- raw Rust panic backtrace
- unfiltered driver internals that expose implementation-level noise
- secrets or credentials from provider configuration

Marreta trace output belongs in stderr/runtime logs, not in the default HTTP
response body.

### `rescue`

Inside `rescue`, the error surface remains semantic and structured:

- `error.code`
- `error.message`
- `error.op`

The execution trace is not automatically injected into the `error` map for now.
If an error is successfully handled by `rescue`, it is not considered uncaught
and should not be emitted as a top-level uncaught runtime trace.

An uncaught `raise`, however, is still an engine-originated failure at the
top-level boundary and therefore should participate in Marreta trace emission.

### Startup errors

Startup/load/validation failures are not HTTP responses and therefore must not
use the default HTTP error envelope.

They should be reported only through Marreta-formatted stderr/runtime output.

## Scope

This plan covers:

- hardening uncaught runtime errors
- Marreta-native execution trace
- stricter distinction between authored responses and engine-generated error responses
- normalization of stderr/runtime logging for failures

This plan does not cover:

- new `raise` / `rescue` syntax
- exposing traces to HTTP clients by default
- full structured logging redesign
- security/auth semantics

## Delivery Strategy

This feature should be implemented as a single `022`, but in internal phases.

### Phase 1 — Useful and safe Marreta trace

Required:

- preserve authored `reply` and `fail` responses exactly
- stable engine-generated error envelope for uncaught failures
- Marreta-native trace at the main execution boundaries:
  - route
  - consumer
  - task
  - infrastructure operation
- `.marreta` file and line information wherever source metadata is already practical
- no Rust leakage in HTTP or normal developer-facing stderr

This phase is enough to consider `022` functionally delivered if the resulting
trace is already useful in real debugging.

### Phase 2 — Finer-grained source precision

Refinements:

- better source metadata propagation through runtime values
- more precise expression/pipeline call-site attribution
- richer frame coverage where the extra precision is worth the complexity

This phase should happen inside `022` if it remains tractable. If source-span
work grows disproportionately, it can be split later, but that is not the
default plan.

## Performance and Runtime Cost

Marreta trace introduces runtime bookkeeping, so this feature must be
implemented conservatively.

Expected risk areas:

- push/pop overhead for trace frames on normal successful execution
- avoidable string/path cloning
- frame copying in concurrent or nested execution paths
- over-instrumentation of fine-grained expression nodes

Implementation guidance:

- use a lightweight frame stack
- keep frames minimal
- avoid formatting full trace strings on the success path
- instrument only major execution boundaries in phase 1
- defer expression-level granularity until phase 2

## Proposed Runtime Rules

### Rule 1 — `reply` and `fail` are immutable response surfaces

If execution terminates through explicit `reply` or `fail`, the engine sends exactly the user-authored body.

### Rule 2 — uncaught runtime failures become engine-generated responses

If execution terminates because of an uncaught interpreter/runtime/infrastructure failure, the engine generates the standard error response body.

### Rule 3 — every engine-originated error must have semantic identity

Every uncaught runtime failure must resolve to:

- `error.code`
- `error.message`
- `error.op` when applicable

### Rule 4 — every uncaught runtime failure should carry Marreta frames

The runtime should accumulate Marreta frames as evaluation enters:

- routes
- tasks
- consumers
- infrastructure operations

When the error reaches the top-level HTTP/consumer boundary, the engine prints
that trace. The order should be stable and readable: from the outermost frame to
the innermost frame. Each frame should include `.marreta` file and line
information whenever the runtime can resolve it.

### Rule 5 — panic hook remains last-resort safety net

The panic hook remains, but it should be the outermost guard. Expected runtime failures should be modeled as `MarretaError`, not left to panic handling.

### Rule 6 — startup failures use Marreta-formatted stderr only

If project loading, startup validation, route registration, or runtime boot
fails before request handling begins, the engine prints a Marreta-formatted
error to stderr and exits without emitting any HTTP envelope.

## Implementation Plan

### Phase A — Audit and classify current panic/error surfaces

1. Audit `panic!` usage outside tests
2. Classify each case:
   - legitimate internal invariant
   - expected runtime failure that should become `MarretaError`
3. Reduce panic-based control flow in interpreter/runtime boundaries

### Phase B — Introduce Marreta trace frames

Add a small runtime trace model, for example:

- `MarretaFrame`
- `MarretaTrace`

Capture frames when entering:

- route execution
- task execution
- queue/topic consumer handlers
- infrastructure operations

Each frame should carry source location metadata when available:

- file path
- line
- optional column later if useful

Phase-1 guidance:

- start with route/consumer/task/infra frames
- avoid frame-per-expression instrumentation initially

### Phase C — Attach trace to uncaught runtime errors

Ensure top-level request/consumer boundaries can print:

- semantic message
- semantic code
- semantic operation name when applicable
- Marreta-native trace

without leaking Rust internals.

### Phase D — Preserve authored responses

Review the HTTP response pipeline and guarantee:

- `reply` bodies are never wrapped
- `fail` bodies are never wrapped
- only uncaught runtime errors use the engine-generated default envelope

### Phase E — Tests and docs

Add explicit tests for:

- authored `fail` body remains unchanged
- authored `reply` body remains unchanged
- uncaught `db/doc/cache/queue/http_client` errors use semantic envelope
- stderr contains Marreta trace for uncaught runtime failure
- startup failures use Marreta-formatted stderr, not HTTP envelope semantics
- no Rust panic text leaks to HTTP body

## Validation Plan

### Unit tests

- verify runtime-generated HTTP error envelope shape
- verify `reply` and `fail` do not receive injected metadata
- verify trace frame push/pop behavior
- verify translated errors keep semantic `error.code`
- verify trace bookkeeping is zero-output on successful execution

### Integration tests

- route calling nested tasks that trigger a DB error
- route calling nested tasks that trigger a cache/queue/doc/http_client error
- consumer path that fails with uncaught infrastructure error
- ensure HTTP output stays semantic while stderr shows Marreta trace
- ensure rescued errors do not emit top-level uncaught trace output
- ensure startup/load failures emit only Marreta-formatted stderr output

### Functional validation

Use `examples/functional_tests` with dedicated routes/tasks that:

1. return explicit `fail` bodies
2. return explicit `reply` bodies
3. trigger uncaught DB/runtime errors inside nested task chains
4. prove that:
   - authored response body is untouched
   - engine-generated error body is semantic
   - runtime logs show Marreta trace

### Lightweight performance validation

The goal is not a full benchmarking suite, only a regression guard against
obvious overhead explosions.

Suggested checks:

1. success-path route benchmark or timing comparison
   - a route with nested task calls but no failure
   - compare runtime before/after the trace implementation in local development
   - ensure there is no obvious order-of-magnitude regression

2. repeated task-call micro test
   - run a task chain many times in a test harness
   - ensure trace bookkeeping does not allocate or format output on success

3. concurrent path sanity check
   - exercise broadcast or consumer execution
   - confirm trace context stays isolated and does not grow unbounded

Acceptance intent:
- no obvious pathological slowdown
- no unbounded trace growth
- no success-path trace output

## Test Plan

### Phase A — Authored HTTP responses remain untouched

Goal:
- prove that `reply` and `fail` bodies are never wrapped or enriched by the engine

Suggested fixtures:
- route returning `reply 400, { reason: "invalid payload" }`
- route returning `fail 404, { error: "user not found" }`
- route returning `fail 422, "invalid order"`

Checks:
- response body matches exactly what the Marreta code authored
- no injected fields such as `code`, `trace`, `runtime`, or `details`

### Phase B — Uncaught runtime errors use semantic engine envelope

Goal:
- prove that uncaught runtime/infrastructure failures return the engine-generated envelope

Suggested fixtures:
- route that triggers DB failure
- route that triggers doc failure
- route that triggers cache failure
- route that triggers queue failure
- route that triggers http_client failure

Checks:
- HTTP response is not the raw Rust/driver error
- HTTP response includes semantic `error` and `code`
- `code` is stable and Marreta-level (`db_error`, `cache_error`, `queue_error`, `infrastructure_error`, etc.)

### Phase C — Nested task chains produce Marreta trace

Goal:
- prove that the runtime captures execution flow across route -> task -> task -> infra operation

Suggested fixture:
- route calls task A
- task A calls task B
- task B triggers infra/runtime failure

Checks:
- stderr/log contains Marreta-native trace
- trace shows route frame
- trace shows nested task frames
- trace shows operation frame when applicable
- trace shows `.marreta` file and line for frames that have source metadata
- no Rust backtrace or crate path leaks

### Phase D — `raise` semantics in trace handling

Goal:
- prove the distinction between handled and unhandled `raise`

Suggested fixtures:
- route/task with `raise "boom"` and no `rescue`
- route/task with `raise "boom"` captured by `rescue`

Checks:
- uncaught `raise` returns engine-generated error envelope and prints Marreta trace
- handled `raise` does not emit top-level uncaught runtime trace
- `rescue` still receives semantic `error.message`, `error.code`, and `error.op`

### Phase E — Consumer failures

Goal:
- prove that queue/topic consumers follow the same hardening rules

Suggested fixtures:
- `on queue` consumer that triggers uncaught DB/cache/doc failure
- `on topic` consumer that triggers uncaught runtime failure

Checks:
- runtime logs show Marreta-native consumer trace
- no Rust panic or raw driver details leak
- handled failures inside `rescue` do not emit uncaught trace

### Phase F — Startup failures

Goal:
- prove that startup/load/validation failures use Marreta-formatted stderr only

Suggested fixtures:
- invalid project metadata
- route registration conflict
- invalid provider config
- project load failure

Checks:
- process exits with failure
- stderr uses Marreta-formatted error output
- no HTTP envelope is involved
- no Rust panic output is shown

### Phase G — Panic fallback safety net

Goal:
- prove that unexpected internal panic still remains contained

Suggested approach:
- targeted test hook or controlled internal panic path in test-only fixture

Checks:
- panic hook emits Marreta-formatted internal error
- no raw Rust panic output reaches normal developer-facing output
- process/request failure remains bounded to the request or startup boundary being tested

### Phase H — Overhead sanity checks

Goal:
- prove that trace support does not introduce disproportionate overhead

Suggested fixtures:
- route with nested task chain that succeeds
- repeated task execution loop
- concurrent broadcast path

Checks:
- no trace output on success path
- no obvious order-of-magnitude slowdown in local timing comparison
- no unbounded growth of in-memory trace context across repeated calls

### Functional runner recommendation

Primary functional workspace:
- `examples/functional_tests`

Recommended additions:
- dedicated routes/tasks for:
  - authored `reply`
  - authored `fail`
  - uncaught nested DB/doc/cache/queue/http_client failures
  - handled vs unhandled `raise`
- assertions on:
  - HTTP body
  - stderr/log content
  - `.marreta` file and line presence in trace output when source metadata exists
  - absence of Rust leakage patterns such as `panicked at`, crate paths, or raw driver type names

### Exit criteria for implementation

The implementation should not be considered complete until all of the following are demonstrated:

- authored responses remain byte-for-byte or structurally unchanged
- uncaught engine-originated failures return semantic HTTP envelopes
- stderr/runtime logs show Marreta-native traces for uncaught failures
- Marreta traces include `.marreta` file and line information wherever source metadata is available
- handled `raise` / `rescue` flows do not emit top-level uncaught trace output
- startup failures use stderr-only Marreta formatting
- panic fallback still prevents raw Rust panic leakage
- success-path execution does not emit trace output or show obvious pathological overhead

## Acceptance Criteria

- `reply` and `fail` responses remain exactly as authored
- uncaught engine-originated failures use a stable Marreta error envelope
- Rust panic output never appears in HTTP response bodies
- stderr/runtime logs show Marreta-native trace for uncaught runtime failures
- Marreta traces include `.marreta` file and line information wherever the runtime has source metadata
- handled `rescue` flows do not emit top-level uncaught runtime traces
- startup failures use Marreta-formatted stderr output only
- unexpected internal panic still produces Marreta-formatted internal error output, not raw Rust panic output
- core runtime boundaries stop relying on panic for expected failures
- success-path execution remains free of trace output and avoids obvious pathological overhead

## Follow-up Considerations

- optional debug mode to include richer trace context locally
- structured JSON logs for trace output
- editor/runtime integration that links trace frames back to source files
