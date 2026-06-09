# 051 - AST Hot Path Optimization (Single Engine)

> Status: Approved
> Type: Runtime performance / execution engine
> Scope: Apply Spec 050's findings to the single AST interpreter and retire the
> route execution template fast path entirely (one engine, no flag, no fallback)

---

## 1. Purpose

Spec 050 proved, with measurements, that a private fast path makes simple and
route-local-composition routes ~1.66x faster on p95 (~1.68x p99) and ~40% cheaper
on CPU. Crucially, it also proved **where** the wins come from: removing
per-request overhead that the tree-walking interpreter pays unnecessarily — not a
fundamentally better execution model (Spec 050, §16.2).

The fast path delivered that as a *second* execution engine behind a default-off
flag, with mandatory hand-maintained parity against the interpreter. That dual
engine is a maintenance liability and is not the project's intended end state.

This spec folds the 050 findings into the **single AST interpreter** so that the
one engine — the semantic source of truth — is also the fast one, and removes the
fast-path concept and every trace of it. The outcome is one approach, no toggle,
no fallback, no duplicated semantics.

---

## 2. Motivation

Marreta's identity includes good performance with a small, maintainable runtime.
Two engines that must agree (the §16.3 hazard, realized twice during 050) work
against maintainability and against language evolution: every new construct would
have to be implemented and kept in parity in two places.

The single-engine path is preferred because:

- there is exactly one definition of each construct's semantics;
- no eligibility analysis, no fallback, no parity tests between engines;
- the measured wins are overhead removal, which applies directly to the
  interpreter — we do not need a separate representation to get them.

This spec stays tree-walking. It does not pursue a bytecode VM / IR / JIT; that
remains a possible, separate future direction if tree-walking's ceiling ever
becomes the bottleneck.

---

## 3. Optimizations To Apply

Each item targets one of the overheads identified in Spec 050 §16.2. Each must be
behavior-preserving relative to the current `main` interpreter and validated
against the functional suite and the benchmark.

1. **Avoid per-request environment cloning.** Reuse or pool the request-local
   environment instead of cloning the module/global environment per request.
   The request scope must remain isolated per request; only the construction cost
   is removed.

2. **Slot-indexed variable access.** Resolve route-local variable names to stable
   indices once (at project load / route preparation), so request execution reads
   and writes by index instead of hashing names on every access. Name-based
   lookup remains the fallback only where indices cannot be resolved statically.

3. **Direct JSON response serialization.** Serialize supported response values
   straight to bytes, skipping the intermediate `Value -> serde_json::Value`
   construction, for the response-safe value set (null, boolean, integer, float,
   decimal-as-string, string with correct escaping, list, map). Anything outside
   that set uses the existing serialization path. This must produce byte-identical
   responses to today.

4. **Lower-overhead route-local composition.** Execute `task`, `pipeline`, and
   `broadcast` without spinning up a fresh general interpreter context per step.
   For broadcast specifically, statically-pure branches may run sequentially
   (Spec 050 showed OS-thread spawn dominated cost for tiny in-memory branches and
   that sequential execution is observably identical for pure branches);
   genuinely concurrent/IO-bearing broadcasts retain parallel execution. Any
   sequential strategy must preserve declaration-ordered results and identical
   error/cancellation semantics.

These are incremental and independently measurable. Land and benchmark them one
at a time (Spec 050 discipline: bench after each change, attribute the delta).

---

## 4. Mandatory Removal Of The Template Fast Path

This spec must leave **no trace** of the route execution template. Removal is part
of the deliverable, not a follow-up.

Remove from the runtime:

- the `template/` module (eligibility, ops, frame, route);
- the route execution template build at project load and the
  `RouteDefinition` template field;
- the request-time dispatch switch between template and AST execution;
- the `MARRETA_ROUTE_TEMPLATE_MODE` environment variable and its mode plumbing;
- the profiler's template-specific phase (`template_execute`) and any
  template/fallback execution-mode reporting added for 050;
- template-specific parity test scaffolding.

Remove from the harness (left in place during 050 for measurement):

- `MARRETA_ROUTE_TEMPLATE_MODE` pass-through in the example suites'
  `docker-compose.yml` (e.g. `functional_tests`);
- `MARRETA_ROUTE_TEMPLATE_MODE` pass-through in the benchmark `docker-compose.yml`
  and any `INCLUDE`/mode toggles that only existed to compare template on/off.

After this spec, searching the runtime and harness for `template_mode`,
`MARRETA_ROUTE_TEMPLATE_MODE`, `RouteExecutionTemplate`, or an execution-mode flag
must return nothing.

---

## 5. Semantic Contract

There is now one engine, so cross-engine parity is no longer a concern. The
contract is instead **no observable change versus the current `main` interpreter**:

- same HTTP status, body (byte-identical JSON), and headers;
- same request binding and schema coercion behavior;
- same error kind and the same line/column on failures;
- same runtime stack shape and logs.

The optimizations are internal data-flow changes. If any optimization cannot be
made byte/behavior-identical for some case, that case keeps the existing path.

---

## 6. Non-Goals

- No bytecode, IR, VM, or JIT.
- No new language syntax or semantics.
- No reintroduction of a second execution path, eligibility analysis, fallback,
  or on/off flag.
- No `Value` representation overhaul (a lighter internal value is out of scope
  unless a later spec justifies it).
- No change to db/doc/cache/queue/http-client/auth semantics.
- No offloading of interpreter execution to a blocking thread pool — measured a
  net regression under a fixed CPU quota (§10.8); worker sizing (opt 5) is the
  chosen answer to head-of-line blocking.

---

## 7. Validation

Per change (Spec 049/050 discipline):

- `cargo fmt --check`, `cargo check`, `cargo test`, `cargo build --release`;
- functional suites under `examples/` (`functional_tests`,
  `migrations_functional`, and the others) run green against the rebuilt runtime
  image;
- benchmark run after each change, comparing against the current `main` baseline
  (not against a template on/off toggle, which no longer exists), reporting the
  per-change delta and attributing it.

Because there is a single engine, validation is "did behavior stay identical to
`main` while latency/CPU improved", measured with the hot-path profiler (049) and
the containerized benchmark.

---

## 8. Acceptance

- All four optimizations landed (or any deferred one explicitly justified by
  profiler data).
- The template fast path and every `MARRETA_ROUTE_TEMPLATE_MODE` / execution-mode
  trace are gone from runtime and harness.
- No functional or examples regression; responses byte-identical to `main`.
- The benchmark shows the single engine reaching, in aggregate, latency/CPU
  comparable to what Spec 050 measured for the fast path (target: most of the
  ~1.66x p95 / ~40% CPU, acknowledging tree-walking has some ceiling), with the
  per-change deltas recorded.

---

## 9. Relationship To Other Specs

- **Supersedes the delivery of Spec 050:** 050 is concluded as a successful
  experiment whose implementation is not merged (050 §16); this spec realizes its
  findings in the single engine and removes its code.
- **Builds on Spec 049:** the hot-path profiler remains the measurement tool and
  stays in place.
- **Reference:** the 050 experiment branch/tag preserves how each construct was
  lowered and the parity reasoning, which informs the equivalent in-interpreter
  optimizations here.

---

## 10. Measurements

In-memory benchmark, 1000 rps, 30s, 1 CPU / 1 GiB, broadcast included. Each
optimization below records the aggregate and the affected endpoints versus the
previous step, so the spec carries baseline -> per-change deltas as the work
progresses.

### 10.0 Baseline (single AST engine, before any 051 optimization)

| Metric | Value |
|---|---:|
| aggregate avg | 0.565 ms |
| aggregate p50 | 0.520 ms |
| aggregate p95 | 0.998 ms |
| aggregate p99 | 1.267 ms |
| CPU avg / peak | 38.3% / 41.4% |
| memory avg | 59.5 MiB |

Slowest endpoints (p95): broadcast_chain 1.311, broadcast_list 1.206,
broadcast_scalar 1.140, pipeline 1.118, direct_chain 0.983, direct_list 0.929,
tasks 0.910, operators 0.769, health 0.717.

For reference, Spec 050's fast path reached aggregate p95 ~0.628 / CPU ~24.4% on
the same workload; that is the target envelope to approach with the single engine.

### 10.1 Optimization 3 — direct JSON response serialization

`Value` now implements `serde::Serialize` (mirroring `value_to_json` exactly,
byte-for-byte, verified by a parity test), and the reply/fail paths serialize via
`serde_json::to_string(&value)` instead of building an intermediate
`serde_json::Value` tree and stringifying it.

| Metric | Baseline | Direct JSON | Delta |
|---|---:|---:|---:|
| aggregate avg | 0.565 ms | 0.554 ms | -2.0% |
| aggregate p95 | 0.998 ms | 0.960 ms | -3.8% |
| aggregate p99 | 1.267 ms | 1.233 ms | -2.7% |
| CPU avg | 38.3% | 38.8% | flat (noise) |
| memory avg | 59.5 MiB | 59.3 MiB | flat |

A small, real win, as expected: JSON serialization was a minor component of total
request cost (a few microseconds). It removes one allocation/walk and is fully
parity-preserving. Functional suite green (548) and `migrations_functional` green.
The larger levers (per-request environment cloning, composition re-entry) follow.

### 10.2 Optimization 1 — shared, read-only environment base

`Environment` now holds a shared `Arc<HashMap>` base (global/module definitions)
plus request/block-local scopes. The base is frozen once after project load
(`ProjectRuntime::freeze_envs`, before serving) and never mutated; all writes go
to local scopes (shadowing the base, observably identical and isolated per
request). Cloning an environment — which happens per request **and per task call
and per broadcast branch** — now only bumps the `Arc` instead of deep-copying
every global definition (task ASTs included). Thread-safe because the base is
read-only and `Value` is `Send + Sync`.

| Metric | Baseline | After opt 3 | After opt 1 | Δ vs prev | Δ vs baseline |
|---|---:|---:|---:|---:|---:|
| aggregate p95 | 0.998 ms | 0.960 ms | 0.870 ms | -9.4% | -12.8% |
| aggregate p99 | 1.267 ms | 1.233 ms | 1.112 ms | -9.8% | -12.2% |
| aggregate avg | 0.565 ms | 0.554 ms | 0.535 ms | -3.4% | -5.3% |
| CPU avg | 38.3% | 38.8% | 34.5% | -4.3pp | -3.8pp |

A larger win than opt 3, as expected: the deep environment clone was paid on
every request and again on every task call and broadcast branch, so it weighed
on the composition routes most. CPU dropped meaningfully (less per-request work).
Parity preserved: functional suite green (548), `migrations_functional` green.
The remaining tail is dominated by composition routes (broadcast ~1.0-1.1 ms,
pipeline ~0.98 ms), addressed next.

### 10.3 Optimization 4a — task-call caller scope (composition)

A cross-module task call was injecting the caller's **entire** variable set
(`all_variables()`, which after opt 1 includes the shared base) into the task
scope on every call — deep-cloning every global definition (task ASTs included)
per call. That dominated the composition routes (every `task`/`pipeline`/
`broadcast`/`direct/*` call paid it).

Now only the caller's **local** variables are propagated (`local_variables()` —
route-local task definitions and route vars, which must remain callable down a
task chain). Globals are not re-injected: `base_env` already provides them via
the shared `Arc`.

| Metric | After opt 1 | After opt 4a | Δ vs prev | Δ vs baseline |
|---|---:|---:|---:|---:|
| aggregate p95 | 0.870 ms | 0.671 ms | -22.9% | -32.8% |
| aggregate p99 | 1.112 ms | 0.864 ms | -22.3% | -31.8% |
| aggregate avg | 0.535 ms | 0.383 ms | -28.4% | -32.2% |
| CPU avg | 34.5% | 23.3% | -11.2pp | -15.0pp |

Per-endpoint p95: tasks 0.597, pipeline 0.656, direct/* 0.58-0.63, broadcast
0.79-0.86. The biggest single win, because the all-globals clone was paid on
every composition call. Parity preserved: functional suite green (548),
`migrations_functional` green.

With opts 3 + 1 + 4a the single AST engine reaches aggregate p95 ~0.671 / CPU
~23.3%, essentially matching Spec 050's fast-path envelope (~0.628 / ~24.4%) —
confirming the 051 hypothesis that those wins were overhead removal, not a
better execution model.

**Open semantic note (for review, not a blocker):** propagating the caller's
local definitions down a task chain is what lets a route-local task call another
route-local task (covered by the `db_uncaught_chain` trace test). This is
dynamic-ish and sits in tension with Spec 017's lexical "not accidental caller
context" rule. The current behavior is preserved here for parity; reconciling the
scoping model (e.g. proper lexical closures for route-local tasks) is separate
work.

### 10.4 Optimization 4b — sequential broadcast for pure branches

`*>>` broadcast spawned an OS thread per branch. For branches that are provably
side-effect-free (a task whose body — inline or a no-statement block — touches no
db/doc/cache/queue/http_client, makes no calls, and runs no nested broadcast), the
thread spawn costs far more than the work, and sequential execution yields
identical results in declaration order. The interpreter now runs such broadcasts
sequentially; anything not provably pure keeps the parallel path (so real I/O
branches retain parallelism and identical error/side-effect semantics). The purity
check is conservative — a false "impure" only forgoes the fast path, and a misread
can never run an I/O branch sequentially, so it cannot change semantics.

Broadcast internal cost dropped from ~200-470 us to ~50-72 us. Per-endpoint p95
in the three-runtime scoreboard:

| Broadcast endpoint | Before | After | Node |
|---|---:|---:|---:|
| broadcast_scalar | 0.856 ms | 0.665 ms | 0.541 ms |
| broadcast_list | 0.919 ms | 0.678 ms | 0.547 ms |
| broadcast_chain | 0.909 ms | 0.670 ms | 0.566 ms |

The broadcast routes are no longer outliers — they now sit with Marreta's other
routes and beat FastAPI. Parity preserved: functional suite green (548),
`migrations_functional` green.

### 10.5 Standing vs Node and FastAPI (after opts 3, 1, 4a, 4b)

Three-runtime scoreboard (1000 rps, 30s, broadcast included):

| Runtime | avg | p95 | p99 | CPU | Mem |
|---|---:|---:|---:|---:|---:|
| Marreta | 0.444 ms | 0.669 ms | 0.812 ms | 25.0% | 56 MiB |
| Node/Fastify | 0.354 ms | 0.585 ms | 0.860 ms | 24.4% | 92 MiB |
| FastAPI | 0.734 ms | 1.190 ms | 1.746 ms | 48.6% | 66 MiB |

The single AST engine: beats FastAPI ~2x; beats Node on p99 (tail) and on memory
(~1.6x lower); ties Node on CPU. It trails Node on p95 by ~14%, now a **uniform**
~50-90 us/route gap (the broadcast outlier is gone) — the residual cost of a
tree-walking interpreter versus V8-compiled handlers. Closing that uniform gap is
beyond overhead removal (it would need slot-indexed access — opt 2 — for a modest
gain, or a compiled execution tier, explicitly out of this spec's scope).

### 10.6 Optimization 4c — skip the fork for same-module task calls

A breadth check (new `/feat/*` endpoints: recursion, reduce, while, list stats,
conversions, json) showed Marreta ~1.5-2x Node on most, and ~5.5x on recursion
(100 task calls/request, ~1.7ms internal). Profiling pinned it on `fork_with_env`:
a cross-module task call cloned the whole interpreter per call, including the
`trace_frames` stack — which grows with recursion depth, making the clone
**O(depth^2)**.

A fork is only needed to switch to *another* module's environment base and
`current_module`. A same-module call — recursion and route-local composition —
already runs against the right base in `self`, so it now takes the cheap
in-place path (push a scope, bind params, run, pop) with no per-call interpreter
clone. Cross-module calls still fork. Behaviour preserved: functional suite green
(548), `migrations_functional` green.

| Endpoint (internal avg) | Before | After |
|---|---:|---:|
| feat_recursion | 1738 us | 737 us (~2.4x) |
| tasks | ~62 us | 45 us |
| pipeline | — | 53 us |
| broadcast_scalar | ~50-72 us | 43 us |
| direct_chain | — | 42 us |

This is a broad composition win (every same-module task call benefits), not just
recursion, and it removes the quadratic blow-up. recursion's residual cost is now
linear in depth (~7 us/call: trace-frame push + scope alloc + param bind), the
inherent per-call cost of a tree-walking interpreter.

### 10.7 Optimization 5 — size tokio worker threads to the CPU quota

By default tokio sizes its worker pool to the host's logical CPUs. Inside a
container with a CPU limit (`--cpus`, enforced via the CFS quota — which throttles
bandwidth, not affinity, so `available_parallelism` still reports all host cores),
this over-subscribes worker threads onto a throttled CPU (16 threads on 1 CPU).
The HTTP server runtime now detects the cgroup CPU quota (v2 `cpu.max`, then v1
`cpu.cfs_quota_us`) and sizes its worker pool to it, with a floor of 2 so a
CPU-bound handler cannot stall all I/O on a single worker. Fully transparent (no
developer configuration); `MARRETA_WORKER_THREADS`/`TOKIO_WORKER_THREADS` override.

Worker-count sweep (1000 rps, 30s, 1 CPU limit), aggregate p95:

| workers | 16 (host default) | 8 | 4 | 2 | 1 |
|---|---:|---:|---:|---:|---:|
| p95 | 0.834 ms | 0.746 | 0.822 | 0.785 | 0.855 |

The default 16 is the worst end; 1 is worst for the fast routes (single worker
blocked by a slow handler — head-of-line blocking confirmed); sizing to the quota
(2 here) lands at ~0.79 ms, ~5% better than the host default and free/correct for
any CPU-limited deployment.

### 10.8 Rejected — CPU-bound handler offload to a blocking pool

Hypothesis: running each request's synchronous (potentially CPU-bound)
interpreter on a tokio blocking-pool thread (`spawn_blocking`) instead of an
async worker would stop a slow route body from stalling a worker (head-of-line
blocking), improving the tail under load. Prototyped end to end: a thread-local
guard marks blocking-pool interpreter threads so `run_async` drives in-handler
I/O with `Handle::block_on` there (and keeps `block_in_place` on async workers,
leaving queue consumers / startup / scenarios untouched); broadcast gained a
matching blocking-pool fan-out. Full unit + functional suites passed.

Same-host A/B (marreta only, in-memory HTTP, 1 CPU limit), offload vs the
pre-offload build at this section's state:

| load | metric | pre-offload | offload |
|---|---|---:|---:|
| 150 rps (~10% CPU) | p50 / p95 / p99 | 0.67 / 1.03 / 1.65 | 0.84 / 1.24 / 1.83 |
| 3000 rps (~65% CPU) | p50 / p95 / p99 | 0.31 / 0.78 / 1.23 | 0.35 / 0.70 / 1.29 |
| 3000 rps | peak CPU | 64.8% | 88.8% |
| 4500 rps (saturation) | p50 / p95 / p99 | 0.23 / 0.78 / 11.5 | 0.27 / 17.3 / 51.8 |
| 4500 rps | peak CPU | 70.0% | 102.8% |

Verdict: net regression for the target deployment, reverted. Under a fixed CPU
quota (the container norm) the bottleneck is CPU, not worker availability, so
offloading does not relieve the real constraint — it adds per-request handoff,
extra threads, and context switches that consume the CPU budget. At low load it
is pure overhead (~+0.2 ms everywhere); at mid load it burns ~37% more CPU for
the same throughput; at saturation it caps throughput ~1.5x earlier and its tail
collapses (p95 22x worse). Head-of-line blocking on an async worker only matters
when there is spare CPU but blocked workers — which does not occur when CPU is
the cap, the case opt 5 already addresses by sizing workers to the quota. The
prototype is preserved on tag `experiment-handler-offload` for reference.

### 10.9 Final standing (delivered configuration)

Delivered config: all three runtimes containerized, each capped at 1 CPU / 1 GB,
k6 in a container on the same docker network, 1000 rps, 60s, broadcast included.
The benchmark app disables request logging (`MARRETA_REQUEST_LOG=false`), which
`serve` defaults to on; per-request log formatting cost ~18% CPU and the
reference Node/FastAPI apps do not log per request, so this levels the field.

Aggregate latency (ms):

| runtime | avg | p50 | p95 | p99 |
|---|---:|---:|---:|---:|
| node | 0.38 | 0.38 | 0.55 | 0.73 |
| marreta | 0.43 | 0.40 | 0.71–0.74 | 1.29 |
| fastapi | 0.72 | 0.66 | 1.18 | 1.65 |

Resource use (sustained average over the steady window, not peak — peak is a
transient and overstated Node's cost via a GC/JIT spike):

| runtime | CPU avg | CPU peak | mem avg |
|---|---:|---:|---:|
| marreta | ~25–27% | 27% | ~69 MiB |
| node | ~23% | ~40% (spike) | ~42 MiB |
| fastapi | ~47% | ~49% | ~44 MiB |

Per-endpoint (avg/p95/p99) shows marreta matching Node on 23 of 24 endpoints
(within ~0.02–0.04 ms; marreta is slightly faster on `echo`). The lone outlier is
`feat_recursion` (marreta 1.13/1.43/1.78 vs Node 0.36/0.54/0.67), which by itself
lifts marreta's aggregate p99 — recursive task-call cost in the tree-walker. It is
the clearest remaining latency target.

Reading: marreta went from worst and erratic on `main` (p95 8 ms, p99 12 ms under
the 1-CPU cap, from tokio over-subscribing 16 workers — opt 5 alone fixes most of
it) to **beating FastAPI across the board and matching Node per-endpoint at
comparable sustained CPU**, with recursion the single exception. Memory remains
the one axis where marreta trails both (~69 vs ~43 MiB).

### 10.10 Measurement-fidelity finding (why latency stops here)

The hot-path profiler shows marreta's whole in-handler time for a simple route is
~0 µs median, ~50 µs p99, while k6 reports ~300 µs `http_req_waiting` (TTFB) for
the same requests. The gap is not the interpreter and not the load generator
(k6's client send+receive is a constant ~70 µs and its open model is honest): it
is kernel networking + the docker bridge + hyper/axum request framing + the
round trip — most of it outside marreta's code, and in production dominated by
real network latency. Running marreta natively on the host over loopback did not
lower the floor (p95 ≈ container), confirming the container is not the cost.

Consequence: interpretation is a rounding error in the request budget, so neither
remaining micro-optimization nor a bytecode/JIT engine would move HTTP-level
latency; they would only reduce CPU-per-request (throughput ceiling / footprint).
This is why **opt 2 (slot-indexed variable access) is deferred**: name lookup is a
tiny fraction of an already-negligible interpretation cost, so its marginal gain
does not justify the added complexity now. It remains available if a future,
compute-heavier workload makes interpreter CPU the proven bottleneck.
