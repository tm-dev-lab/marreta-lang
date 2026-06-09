# 022b — Trace Performance and API Ergonomics

Status: Delivered

## Delivery notes

- `e695e87` — introduced structured trace frame labels and borrowed operation
  labels; changed table-less DB operation labels from `db.unknown.query` to
  `db.query`.
- `0d7c765` — introduced scoped trace frame guards for route, task, and
  consumer execution.
- `891c07b` — removed the old manual trace push/pop/clear API, added TTY-only
  error-code color, documented spec phase-taxonomy guidance, fixed the load-test
  runtime image, and recorded the ecommerce load-test validation in
  `docs/performance/LOAD_TEST_TRACE_022B_20260417.md`.
- documentation follow-up — added comparison between the 022b load-test run
  and the previous DB load-test run.

## Motivation

`022_RUNTIME_ERROR_HARDENING.md` delivered the runtime trace model, the
uncaught-error envelope, and the rescue/panic guarantees. A post-landing review
surfaced four follow-ups that would compromise the language's "excellent DX +
performance + low footprint" promise if left unresolved before 1.0:

1. The trace model allocates on the hot path even when no trace will ever be
   read.
2. The trace frame stack is managed through five manual public methods on
   `Interpreter`, with a push/pop invariant that is re-enforced at every call
   site. This rots as new frame kinds land in the planned expression-level
   precision work.
3. The fork-path task call transfers the child trace stack back to the parent
   on error. This remains isolated to the module-fork path because the child
   interpreter owns a separate environment; the 022b guard work removes manual
   push/pop leakage but does not yet replace fork trace transfer with a shared
   trace-store model.
4. The 022 spec used three overlapping phase taxonomies (Phase 1/2, Phase A–E,
   Phase A–H). Future specs should not inherit that pattern.

None of these are user-visible bugs today. They are maintainability and
runtime-cost regressions that get worse as the trace model grows.

## Goal

Keep the runtime trace model zero-cost on the success path and make the
push/pop invariant impossible to violate, so that future frame kinds (pipeline
stages, expression-level frames) can be added without touching every call site
or introducing new allocations.

## Non-goals

- changing the user-visible trace output format
- changing the spec 022 exit criteria
- adding new frame kinds (deferred to the expression-precision spec)
- restructuring spec numbering for already-landed specs

## Scope

Four changes, in priority order.

### 1. Zero-allocation frame labels

Replace `MarretaFrame { label: String, ... }` with a structured label enum:

```rust
pub enum FrameLabel {
    Route { verb: HttpVerb, path: Arc<str> },
    Task(Arc<str>),
    Consumer { kind: ConsumerKind, target: Arc<str> },
    Op(Arc<str>),
}
```

Guidance:

- `Arc<str>` (not `String`) for path/task/target — the AST already owns these
  strings, so frame construction should reuse them via `Arc::clone`, not
  re-allocate.
- `format!` stays out of the success path entirely. `FrameLabel::render()`
  produces the human-readable label only when an uncaught trace is being built.
- `trace_operation_label()` on `MarretaError` should return a borrowed view
  (`&str` or `Cow<'_, str>`) rather than cloning the `operation` field.

Acceptance:

- no `format!` or `String` allocation in the trace path on a successful route,
  task call, or infra operation
- benchmark or timing comparison shows no regression vs pre-022 on a
  nested-task success loop

### 2. RAII frame guard

Collapse the five manual trace methods (`push_route_frame`,
`push_consumer_frame`, `pop_trace_frame`, `clear_trace`, and the paired
`call_depth` bookkeeping) into a single guard pattern:

```rust
// inside server.rs
let _frame = interp.enter_route(&verb, path, line, col);
```

Guidance:

- one `enter_*` method per frame kind, returning a guard whose `Drop` impl pops
  the frame and (where applicable) decrements `call_depth`
- the guard must be `#[must_use]` so forgetting to bind it is a warning
- internal frame push/pop methods become private; only `enter_*` is public
- `clear_trace` is removed — the server-side boundary no longer needs it,
  because the guard drop handles it
- `uncaught_trace_lines(&err)` stays public, unchanged

Acceptance:

- `Interpreter`'s public surface loses `push_*_frame`, `pop_trace_frame`, and
  `clear_trace`; gains `enter_route`, `enter_task`, `enter_consumer`,
  `enter_op`
- no call site manually pairs push + pop
- the fork-path task call preserves the child trace on error without requiring
  manual push/pop cleanup at the call site
- `debug_assert!` on guard drop verifies the popped frame matches the expected
  kind, catching mispaired guards in tests

### 3. DX polish on error output

Two small DX wins surfaced by the review:

- color the error code (`db_error`, `raise_error`, …) when `stderr` is a TTY,
  using a lightweight dependency-free ANSI helper; plain output when piped to a
  file or CI log
- replace the `db.unknown.query` operation label with `db.query` when the
  driver cannot resolve a table name. The "unknown" token is an implementation
  detail that leaks into user-facing stderr today

Acceptance:

- functional test asserts `db.query` (not `db.unknown.query`) appears in trace
  output for `db.native_query` failures
- TTY-only color output verified manually; piped output remains plain

### 4. Spec taxonomy hygiene

Document a single-taxonomy convention for new specs in `docs/spec/SPEC.md` (or
equivalent template):

- one sequence of phase labels per spec (prefer `Phase 1`, `Phase 2`, … over
  letter/number mixing)
- `Test Plan` sections reference the same phase labels as the `Implementation
  Plan`, never introducing a parallel A–H track
- `Delivery notes` block at the top of delivered specs maps phases to commits,
  as `022_RUNTIME_ERROR_HARDENING.md` now does

Acceptance:

- `docs/spec/SPEC.md` gains a short "Phase taxonomy" subsection
- no behavior change; this is a documentation convention only

## Implementation plan

### Phase 1 — zero-allocation frame labels

1. introduce `FrameLabel` enum and `Arc<str>` fields on `MarretaFrame`
2. update `push_route_frame` / `push_consumer_frame` / task-call sites to
   construct `FrameLabel` variants directly (no `format!`)
3. update `MarretaFrame::render()` to format the label lazily
4. change `MarretaError::trace_operation_label()` to return a borrowed view
5. add a success-path allocation test (or manual `cargo flamegraph` snapshot)

### Phase 2 — RAII frame guard

1. introduce `FrameGuard` with `Drop` pop
2. add `enter_route`, `enter_task`, `enter_consumer`, `enter_op` methods
   returning `FrameGuard`
3. migrate `server.rs`, `interpreter.rs` task call sites, and the consumer
   runner to the new API
4. make the old `push_*` / `pop_*` / `clear_trace` methods private or remove
   them
5. keep fork-path trace transfer contained to the child-error branch while
   ensuring guard cleanup handles the success path
6. update unit tests that reach into `push_route_frame` directly to use
   `enter_route`

### Phase 3 — DX polish

1. add a minimal TTY detection + ANSI color helper (or adopt `anstream`
   if already in tree)
2. update `log_uncaught_runtime_error` in `server.rs` to use the helper
3. adjust the Postgres driver's error translation to emit `db.query` when no
   table is known
4. update the Phase C functional assertion in
   `examples/functional_tests/test.sh` to expect `db.query`

### Phase 4 — spec taxonomy

1. edit `docs/spec/SPEC.md` to document single-taxonomy guidance
2. no code changes

## Test plan

### Phase 1

- unit test: building a frame for a successful route + task + op path makes
  zero `String` allocations beyond the `Arc::clone` path (assert via a custom
  allocator or by inspection)
- existing `uncaught_trace_lines` render tests keep passing with identical
  output strings
- functional suite: unchanged output, all 363 tests still green
- load test: run a representative success-path load scenario against
  `examples/ecommerce` and compare it with the pre-022b baseline
- documentation: record the load-test setup, commands, baseline numbers, and
  post-change numbers under `docs/performance`

### Phase 2

- unit test: an early-return error inside `enter_route` scope leaves
  `trace_frames.len()` unchanged after the guard drops
- unit test: nested `enter_route` → `enter_task` → `enter_op` with a bubbling
  error still renders the correct trace
- `cargo test --lib` and `cargo test --bin marreta` green
- functional suite green
- grep assertion: no occurrence of `push_route_frame`, `push_consumer_frame`,
  `pop_trace_frame`, or `clear_trace` in `src/server.rs` or `src/interpreter.rs`
  outside the guard implementation

### Phase 3

- functional test updated and green
- manual verification of color output on a TTY and plain output when piped
  (`2>/tmp/x; cat /tmp/x` shows no escape codes)

### Phase 4

- `docs/spec/SPEC.md` diff review only

## Acceptance criteria

- no `format!` on the success path for trace frames
- `MarretaFrame::label` is not a `String`
- `examples/ecommerce` load-test results are documented under
  `docs/performance`
- `Interpreter`'s public API no longer exposes manual push/pop/clear for trace
  frames
- fork-path task call preserves child trace context on error and uses guard
  cleanup on success
- `db.query` replaces `db.unknown.query` in user-facing trace output
- `docs/spec/SPEC.md` documents the single-taxonomy convention
- all unit tests + 022 functional tests pass unchanged

## Out of scope (deferred)

- expression-level frames and pipeline-stage frames (separate spec)
- structured JSON log output for traces (follow-up from 022)
- editor integration linking trace frames to source files (follow-up from 022)
