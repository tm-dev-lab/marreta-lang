# 049 - Runtime Hot Path Profiling

> Status: Approved
> Type: Runtime performance / profiling
> Scope: Add measured hot-path profiling without changing runtime behavior

---

## 1. Purpose

Recent comparative benchmarks show Marreta still has strong throughput and low
resource consumption, but its in-memory HTTP latency is no longer clearly ahead
of a minimal Node/Fastify implementation.

At 1000 requests per second, using the same in-memory endpoints and 1 CPU / 1 GB
container limits:

| Runtime | avg | p50 | p95 | p99 | CPU avg | Memory avg |
|---|---:|---:|---:|---:|---:|---:|
| Marreta | 0.479 ms | 0.361 ms | 0.588 ms | 0.799 ms | 25.27% | 32.70 MiB |
| FastAPI | 0.717 ms | 0.630 ms | 1.200 ms | 1.783 ms | 55.29% | 69.10 MiB |
| Node/Fastify | 0.340 ms | 0.335 ms | 0.549 ms | 0.705 ms | 39.63% | 99.50 MiB |

An additional Marreta-only run with request logging, trace context, and docs
disabled produced:

| Runtime | avg | p50 | p95 | p99 | CPU avg | Memory avg |
|---|---:|---:|---:|---:|---:|---:|
| Marreta, observability off | 0.467 ms | 0.456 ms | 0.698 ms | 0.814 ms | 19.18% | 43.86 MiB |

This means observability explains part of CPU cost, but does not explain the
latency gap. The likely bottleneck is the request execution hot path: route
dispatch, interpreter setup, environment cloning, dynamic expression evaluation,
map representation, string interpolation, and JSON serialization.

This spec delivers only the measurement layer required to optimize those paths
with evidence. It does not implement runtime optimizations.

---

## 2. Motivation

Performance is part of Marreta's product identity. The language is implemented
in Rust and should offer:

- predictable low latency for simple in-memory HTTP routes;
- low CPU usage under moderate load;
- low memory footprint compared with general-purpose web frameworks;
- no hidden runtime cost from language features that are not used by a route.

The current runtime is intentionally simple and tree-walking. That is acceptable
for language maturity, but the hot path now carries enough generality that simple
routes may pay for features they do not use.

Before optimizing, Marreta needs a first-party way to answer concrete questions:

- how much time is spent before the interpreter runs;
- how much time is spent building request bindings;
- how much time is spent in schema coercion;
- how much time is spent walking route AST;
- how much time is spent on interpolation and JSON serialization;
- whether auth, response building, or route metadata handling matter for a route.

Without this profiler, optimization work becomes intuition-driven and hard to
review.

---

## 3. Non-Goals

This spec does not introduce:

- new language syntax;
- new user-visible runtime behavior;
- route execution templates;
- bytecode;
- a JIT compiler;
- native code generation;
- direct JSON serialization changes;
- map storage changes;
- route cloning changes;
- interpolation precompilation;
- benchmark-specific shortcuts that bypass language semantics;
- changes to route, schema, task, auth, queue, cache, db, doc, or http-client
  semantics.

Optimization work belongs to later specs. In particular, Route Execution
Templates are separated into Spec 050 so this spec can remain an auditable,
behavior-preserving profiling delivery.

---

## 4. Baseline Evidence

### 4.1 Historical Marreta baselines

Historical performance reports show Marreta previously operated with very low
latency in self-comparison load tests:

- `BASELINE_040.md`: around 5,319 req/s average, with p95 sub-ms for the simple
  health route.
- `LOAD_TEST_TRACE_022B_20260417.md`: around 4,806 req/s average, health p95
  around 0.843 ms.
- `LOAD_TEST_POST_046_20260519.md`: around 4,586 req/s average, health p95
  around 1.605 ms.

Those tests are not directly equivalent to the new FastAPI/Node benchmark. They
used different applications and workload shape. Still, they show that Marreta's
own latency trend needs attention.

### 4.2 Comparative in-memory benchmark

The new `marreta-lang-bench` in-memory HTTP benchmark is intentionally small:

- no db;
- no cache;
- no queue;
- no doc db;
- no auth;
- no migrations;
- no external IO;
- only HTTP routing, request body parsing, expression evaluation, and JSON
  response generation.

This makes it suitable for isolating runtime execution overhead.

### 4.3 Observability-off control run

Disabling request logging, W3C trace context, and generated docs did not close
the latency gap against Node/Fastify.

Conclusion: observability should still be optimized where needed, but it is not
the primary explanation for the current latency profile.

---

## 5. Profiling Mode

Introduce a runtime profiling mode controlled by environment variable:

```text
MARRETA_RUNTIME_PROFILE=hot_path
```

When disabled, profiling must have no meaningful hot-path cost.

When enabled, profiling records aggregated timings, not per-request logs.

Disabled profiling must not allocate, log, capture timestamps, or build profile
events. A single predictable branch based on startup configuration is acceptable;
the implementation must not perform per-request environment reads.

Allowed implementation approaches:

- scoped timers with atomics/histograms;
- per-thread counters aggregated at shutdown;
- low-cardinality route-level aggregation.

Disallowed:

- logging every request timing line;
- allocating a new profile event object per request;
- writing to stdout on each request;
- changing route behavior.

---

## 6. Timed Phases

The profiler must measure at least:

| Phase | Meaning |
|---|---|
| `http_total` | full Axum handler future from request entry to response completion |
| `handler_total` | Marreta handler work after route metadata acquisition |
| `route_clone` | cost of cloning or acquiring route execution metadata |
| `auth_eval` | authentication and authorization work before route execution |
| `env_setup` | interpreter creation and environment/frame setup |
| `request_binding` | params/query/headers/body binding |
| `schema_coercion` | payload/response schema coercion if present |
| `ast_execute` | route body statement/expression execution |
| `interpolation` | string interpolation evaluation |
| `json_serialize` | conversion to HTTP response body |
| `response_build` | final Axum response construction |
| `total_execute_route` | full Marreta route execution time excluding network |

If a phase is not applicable for a route, it records zero or is omitted from that
route summary.

A phase count is the number of phase observations, not necessarily the number of
HTTP requests. For example, a route with multiple `take` bindings may record
multiple `request_binding` observations for one request. Request-level counts are
available through `http_total` and `total_execute_route`.

---

## 7. Output Format

Profiling output must be emitted once at shutdown or on explicit summary trigger.
For the first cut, process shutdown output is enough.

Output should be machine-readable JSON lines or a single JSON document.

Example:

```json
{
  "kind": "marreta.runtime_profile",
  "mode": "hot_path",
  "routes": [
    {
      "route": "GET /item/:id",
      "phases": {
        "http_total": { "count": 30000, "avg_us": 470, "p95_us": 620 },
        "env_setup": { "count": 30000, "avg_us": 80, "p95_us": 120 },
        "ast_execute": { "count": 30000, "avg_us": 170, "p95_us": 260 },
        "json_serialize": { "count": 30000, "avg_us": 70, "p95_us": 110 },
        "interpolation": { "count": 30000, "avg_us": 35, "p95_us": 55 }
      }
    }
  ]
}
```

The exact histogram implementation is an implementation detail. The reported
metrics must be enough to rank bottlenecks.

---

## 8. Benchmark Contract

The first profiling report must be produced using:

- the in-memory HTTP benchmark in `marreta-lang-bench`;
- 1 CPU / 1 GB limit;
- 1000 rps;
- at least 10 seconds of warm-up before recording final metrics;
- request logging disabled;
- trace context disabled;
- docs disabled;
- one run for Marreta only.

Results must not be committed under `results/`. A summarized markdown report may
be committed to the performance repository if needed.

For official comparative reports, k6 should run isolated from the runtime
container where practical. Acceptable approaches include CPU pinning, running
the load generator on a separate host, or documenting that the run was local and
therefore subject to scheduler noise. Local development runs may skip pinning.

Official reports should also include a saturation run whose purpose is to find
the runtime's practical throughput ceiling and latency behavior under overload.
This is separate from the fixed-rate `constant-arrival-rate` runs, which measure
latency at a controlled request rate.

---

## 9. Safety Rules

1. Do not optimize based on intuition alone when profiling data is available.
2. Do not change language semantics as part of profiling.
3. Do not skip functional tests because a change is "only instrumentation".
4. Do not commit raw benchmark result directories unless explicitly requested.
5. Profiling must preserve error span quality and runtime stack behavior.
6. Profiling output must be disabled by default.
7. Profiling must not emit per-request logs.
8. Profiling must not expose a public HTTP endpoint in this first cut.

---

## 10. Required Final Validation

The implementation is not complete until all of the following pass:

```bash
# marreta-lang runtime repository
cargo fmt --check
cargo check
cargo test
cargo build

# marreta-lang-examples repository
./functional_tests/test.sh
./migrations_functional/test.sh
./smart_inventory/test.sh
./ecommerce/test.sh
./init_functional/test.sh
```

If an examples test requires local services, its script is responsible for
starting and stopping them. A runtime profiling change must not be merged with
"unit tests pass" as the only validation.

If a specific examples suite is unavailable in the local environment, the final
implementation note must explicitly state what was not run and why.

`cargo clippy --all-targets -- -D warnings` is desirable, but this repository is
not currently clippy-clean. Until that is fixed globally, Spec 049 cannot use
clippy as a hard delivery gate unless the implementation note shows that no new
clippy debt was introduced by the profiler.

---

## 11. Implementation Notes

The first implementation should add:

- a dedicated `runtime_profile` module;
- startup initialization from `MARRETA_RUNTIME_PROFILE`;
- a cheap disabled fast path;
- route-level aggregate registry;
- scoped timers around the phases listed in Section 6;
- shutdown emission of one JSON/JSONL profile summary;
- unit tests for histogram/stat aggregation and disabled timer behavior;
- a smoke validation showing profile output on shutdown.

The first implementation must not add any optimization from the original hot path
hypothesis list. In particular, it must not:

- remove route definition cloning;
- precompile string interpolation;
- change `Value::Map` storage;
- add direct JSON serialization;
- add execution templates.

Those changes need separate specs or separate implementation phases after the
profiler produces evidence.

---

## 12. Decisions From Review

1. Profiling output starts as shutdown output in JSON or JSONL form. A profiling
   HTTP endpoint is out of scope for the first cut because it creates routing,
   security, and exposure concerns.
2. The first target is HTTP route execution. Tasks called from routes are
   measured through route execution first; standalone task profiling is deferred
   until evidence proves it is needed.
3. Benchmark acceptance for this spec is not "beat Node/Fastify". The acceptance
   condition is reliable profiler output with disabled-by-default behavior and
   no semantic regression.
4. Route Execution Templates are explicitly not part of this spec. They move to
   Spec 050 so their design can be reviewed without contaminating the profiling
   delivery.

---

## 13. Open Questions

1. Should profiling summaries be written only to stderr/stdout, or optionally to
   a file path such as `MARRETA_RUNTIME_PROFILE_OUTPUT` in a future spec?
2. Should future profiling include tasks, consumers, migrations, or startup
   phases as separate profile modes?
3. Should the profiler eventually support route filters, endpoint inclusion, or
   sampling if production profiling becomes necessary?

---

## 14. Initial Recommendation

Deliver Spec 049 as profiling only.

After this lands, use the profiler output to decide the next performance spec.
The strongest current candidate is Spec 050, focused on Route Execution
Templates as a private runtime fast path with mandatory fallback to the existing
AST interpreter.
