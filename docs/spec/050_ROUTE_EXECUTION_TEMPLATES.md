# 050 - Route Execution Templates

> Status: Approved
> Type: Runtime performance / execution engine
> Scope: Private route fast path for simple HTTP routes, with mandatory AST interpreter fallback

---

## 1. Purpose

Spec 049 adds profiling. This spec uses that profiling foundation to explore a
private runtime fast path for simple HTTP routes: Route Execution Templates.

A Route Execution Template is an internal, pre-resolved execution shape derived
from the already parsed Marreta AST. It is not new user syntax, not public
bytecode, not a serialized artifact, and not a JIT.

The goal is to reduce repeated request-time interpreter work:

- repeated symbolic lookup;
- repeated request binding layout decisions;
- repeated dispatch over common statement/expression shapes;
- repeated response construction decisions for simple routes.

The existing AST interpreter remains the semantic source of truth.

After the post-049 saturation runs, this work is no longer framed as "catching
up to Node" for the in-memory benchmark. Marreta already sustains comparable or
better latency under higher pressure. The purpose of this spec is narrower and
more precise: reduce request-time overhead and p99 tail latency for simple
routes without changing language behavior.

The first deliverable is route-only by design. However, the implementation
should avoid hard-coding the internal architecture so narrowly that future
`TaskExecutionTemplate` or `ConsumerExecutionTemplate` work would require a full
rewrite. Shared internal pieces such as eligibility analysis, lowered operation
representation, source-span preservation, and request-local/frame slot handling
should be designed as reusable runtime infrastructure where that does not add
complexity to the route first cut.

---

## 2. Motivation

The current Marreta runtime walks the route AST on every HTTP request. This keeps
the engine simple, but makes very small routes pay for generality they may not
use.

For simple in-memory endpoints, the benchmark goal is aggressive low latency and
stable tail behavior. Profiling and saturation tests show Marreta is already
competitive, but p99 still grows under very high arrival rates. The remaining
optimization target is therefore not throughput first; it is predictable request
execution cost.

Recent saturation results from `marreta-lang-performance`:

| Load | Runtime | Throughput | Avg | P95 | P99 | Error rate | Dropped |
|---|---|---:|---:|---:|---:|---:|---:|
| 6000 rps | Marreta | 5999 rps | 0.320 ms | 0.570 ms | 1.322 ms | 0% | 0 |
| 6000 rps | Node | 5999 rps | 2.056 ms | 0.569 ms | 76.885 ms | 0% | 0 |
| 6000 rps | FastAPI | 1058 rps | 1936.766 ms | 6142.602 ms | 14874.028 ms | 5.26% | 105794 |
| 10000 rps | Marreta | 9998 rps | 1.079 ms | 0.924 ms | 37.095 ms | 0% | 0 |
| 10000 rps | Node | 9999 rps | 3.283 ms | 0.869 ms | 74.359 ms | 0% | 0 |

Report:

```text
/home/thiago/Dev/Git/marreta-lang-performance/LOAD_TEST_SATURATION_IN_MEMORY_HTTP_20260524.md
```

The template hypothesis is that some remaining tail pressure comes from repeated
dynamic execution work in the interpreter path: symbolic lookup, generic
statement dispatch, environment shape setup, and response construction through
general-purpose values.

This is inspired by the same broad principle behind template interpreters in
mature runtimes: reduce repeated dispatch and repeated symbolic work in hot
paths. Marreta does not need a bytecode VM or JIT to apply that principle.

Important: this is not a promise to replace the interpreter. It is an experiment
to add a private fast path for route shapes that are provably safe to execute
without repeatedly rediscovering the same structure on every request.

---

## 3. Definition

An execution template is a runtime-owned structure built at project load:

```text
AST route body
  -> validated project load
  -> optional RouteExecutionTemplate
  -> request-local frame execution
```

It is only an optimization of data flow. It does not change:

- parsing;
- linting;
- formatting;
- OpenAPI generation;
- source spans;
- error stack construction;
- public runtime semantics.

A developer writing Marreta code must not know or care whether a route uses a
template internally. The only observable difference allowed is lower latency in
profiling and benchmarks.

### 3.1 What "Template" Means Here

In this spec, "template" means a precomputed execution layout, not a textual
template and not bytecode.

For example, the runtime may transform a route body like:

```marreta
route GET "/item/:id"
    id = params.id
    reply 200, { id: id, ok: true }
```

Into an internal shape equivalent to:

```text
RouteExecutionTemplate {
  params_needed: ["id"],
  slots: {
    params: 0,
    id: 1
  },
  steps: [
    read_param("id") -> slot 1,
    build_map([("id", slot 1), ("ok", true)]),
    reply_static_status(200)
  ]
}
```

This structure is private to the runtime. It must retain source spans for every
step that can fail. If the runtime cannot build this shape safely, it does not
warn and does not error; it simply uses the AST interpreter.

The template must never be serialized, exposed to tooling, exposed to user code,
or treated as a stable artifact.

---

## 4. Non-Goals

This spec does not introduce:

- new language syntax;
- public bytecode;
- bytecode files;
- a VM instruction set exposed to tooling;
- JIT compilation;
- native code generation;
- benchmark-only shortcuts by route name;
- changes to route/schema/task/auth/db/doc/cache/queue/http-client semantics;
- template support for all language constructs in the first cut;
- template support for consumers or standalone tasks in the first cut;
- changing the public semantics of route-local tasks, pipelines, or broadcast.

The template engine must not special-case `/health`, `/item/:id`, `/echo`, or
`/summary` by route name. It may support the language constructs those routes
use.

Although consumers and standalone tasks are explicitly out of scope for this
spec, route-local composition is in scope when it appears inside HTTP route hot
paths. That includes direct task calls, simple pipelines, and broadcast
expressions used by routes. Optimizing those constructs does not make standalone
tasks or consumers templated artifacts; it only reduces the cost paid while a
route is executing.

The internal implementation should keep the route-specific layer separate from
reusable template primitives. For example:

```text
template/
  eligibility.rs      # shared shape/support analysis primitives
  ops.rs              # shared lowered operation representation
  frame.rs            # shared slot/frame primitives
  route.rs            # route-specific build/execution glue
```

This structure is illustrative, not mandatory. The architectural requirement is
separation of concerns: route templates are the only behavior delivered here,
but the lower-level machinery should not be unnecessarily route-shaped.

---

## 5. Semantic Contract

Execution templates must preserve the public behavior of the current runtime:

- same HTTP status;
- same response body;
- same response headers;
- same request binding behavior;
- same schema coercion behavior;
- same error kind;
- same line and column whenever an error is reported;
- same runtime stack shape when stack frames are produced;
- same logs, except for explicit profiling metadata.

The template engine must not bypass validation. If the current interpreter would
parse JSON, coerce a schema, validate a `require`, serialize a decimal as a
string, or reject a payload, the template path must do the same.

Error span parity is mandatory. A fast path that loses diagnostic precision is
not acceptable.

---

## 6. Mandatory Fallback

Fallback is mandatory. A route must execute through the existing AST interpreter
when it uses any construct the template engine does not explicitly support.

Unsupported constructs must not produce partial or approximate behavior.

Fallback rules:

- unsupported statement kind -> AST interpreter;
- unsupported expression kind -> AST interpreter;
- unsupported builtin or namespace operation -> AST interpreter;
- dynamic behavior that requires normal interpreter environment semantics -> AST
  interpreter;
- any template build error during project load -> discard the template and use
  AST interpreter for that route;
- any ambiguity about source span preservation -> AST interpreter.

Fallback is not an error. It is the safety mechanism that lets the template
engine grow incrementally without breaking existing projects.

Fallback must happen at template build time whenever possible. Runtime fallback
is allowed only for situations that cannot be decided at project load without
changing semantics.

The implementation should prefer this rule:

```text
if route can be fully templated:
    execute template
else:
    execute AST interpreter
```

It should avoid mixed execution in the first cut. Executing half a route through
the template and half through the interpreter increases semantic risk and makes
error stack parity harder to prove.

### 6.1 Maintenance Guardrails

The current AST/template integration is intentionally simple: a route owns an
optional private template built during project load, and request execution makes
a single switch between the template path and the normal AST interpreter. This
shape is acceptable and should be preserved.

Future work must avoid these failure modes:

- do not introduce mid-route execution mixing unless there is a complete and
  tested state-transfer model between template frames and interpreter
  environments;
- keep `execute_route_profiled` from becoming the permanent home for every fast
  path decision. If template routing grows, extract the dispatch boundary into a
  small helper so the server request path remains readable;
- move parity test helpers out of `server.rs` if the supported template surface
  grows substantially. The test policy is right, but the file should not become
  a catch-all template test module;
- define `MARRETA_ROUTE_TEMPLATE_MODE=auto` before enabling templates by default.
  Today, `auto` is effectively equivalent to enabled execution for eligible
  templates; default-on behavior requires clearer rollout semantics and enough
  examples coverage;
- keep template internals private. Linter, formatter, OpenAPI, doctor, and user
  source semantics must not depend on whether a route is templated.

These guardrails are not blockers for the first delivery. They are boundaries to
keep the fast path from turning into a second, divergent runtime.

---

## 7. First Experiment Scope

The first implementation must be deliberately narrow. It should target only the
simple in-memory HTTP benchmark route shapes.

Supported in the first experiment:

- literal `reply`;
- terminal `fail`;
- `require ... else fail` guards;
- literal maps and lists;
- path parameter reads;
- query/header reads when explicitly taken by the route;
- local assignment;
- simple identifier reads;
- string interpolation if already represented in a pre-parsed internal form;
- direct JSON serialization for template responses;
- direct calls to a small allowlist of pure builtin namespace operations if
  needed by the benchmark.

Recommended first supported statement/expression set:

- `reply STATUS, BODY`;
- `fail STATUS, BODY` only as a terminal statement;
- `require EXPR else fail STATUS, MESSAGE`;
- assignment from literal, identifier, path/query/payload read, or supported
  pure expression;
- literal string/integer/float/boolean/null;
- literal map/list;
- identifier read from template slot;
- params/query/headers field read;
- pre-parsed string interpolation segments.

Not supported in the first experiment:

- payload/form/raw binding;
- schema-coerced payload routes;
- db/doc/cache/queue/http-client operations;
- transactions;
- auth and allow rules;
- rescue/error control flow beyond existing fallback;
- dynamic task dispatch;
- consumers;
- scenario-only behavior;
- mutations whose semantics depend on shared `Value::Map` locking.

The purpose is to validate the execution model, not replace the interpreter in
one PR.

Routes with unsupported constructs must keep working through the existing AST
path. That includes real-world application routes using db/doc/cache/queue/auth.

---

## 8. Template Frame Model

The template engine should avoid cloning the full interpreter environment for
supported routes. It should use a request-local frame with stable slots:

```text
slot 0 = params
slot 1 = payload
slot 2 = local variable declared by route
slot 3 = temporary expression result
```

Exact slot layout is an implementation detail. The contract is:

- slot assignment happens once during project load;
- request execution uses numeric slots where safe, not repeated string lookup;
- global/module values remain accessible through the existing interpreter when
  fallback is needed;
- source spans remain attached to lowered operations for diagnostics.

The frame is request-local. It must not be shared across requests, stored in
global runtime state, or exposed as `Value` to user code. This keeps the
optimization separate from the language's visible map/list/value model.

The first implementation may still store slot values as existing `Value`
instances. It does not need to introduce a new visible value representation.
Changing `Value::Map` storage remains outside the minimum template experiment.

Measured note: direct raw request lookup was evaluated during implementation
and rejected for this cut. The experiment avoided materializing selected
request maps and resolved params/query/headers lazily from raw request data
instead. Under the current benchmark this regressed latency versus the
selective-map approach because the same request fields can be read multiple
times and repeatedly converted. The first delivery therefore keeps selective
request map materialization and slot reuse. A future attempt may revisit this
only with caching or a different request-local representation.

Direct JSON serialization is allowed in the first route-template experiment, but
only inside the template response path. The AST interpreter response path remains
unchanged. The goal is to remove the avoidable intermediate
`Value -> serde_json::Value -> response` conversion for template-eligible
responses without changing public semantics.

The direct serializer must support only normal response-safe values:

- null;
- boolean;
- integer;
- float;
- decimal as JSON string;
- string with correct JSON escaping;
- list;
- map.

If a template response contains a value that cannot be serialized by the direct
serializer with exact parity, the implementation must fall back to the existing
response serialization path or make the route ineligible for templating.

If the template frame introduces a lighter internal representation later, that
representation must remain private and must convert back to normal `Value`
semantics at language boundaries.

The frame abstraction should be named and modeled generically enough to support
future execution contexts. Route execution may use route-specific slots such as
params/query/payload, while future task or consumer templates may use argument or
message slots. This spec only requires route slots, but it should not encode
"HTTP route" into the shared frame primitives.

---

## 9. Profiling Requirements

Spec 049 profiling is a prerequisite for this work.

Profiling output must make execution mode visible for routes where templates are
attempted:

```json
{
  "route": "GET /item/:id",
  "execution_mode": "template",
  "template_fallbacks": 0,
  "ast_execute": { "avg_us": 4 },
  "template_execute": { "avg_us": 2 }
}
```

If the route falls back:

```json
{
  "route": "GET /orders/:id",
  "execution_mode": "ast_fallback",
  "fallback_reason": "db_operation_not_supported"
}
```

The exact JSON shape may differ, but benchmark analysis must distinguish template
execution from AST fallback.

Minimum profiling additions:

- count routes that have a template;
- count routes that use AST fallback;
- record fallback reason when profiling is enabled;
- record `template_execute` timing separately from `ast_execute`.

Profiling must not emit fallback metadata when profiling is disabled.

---

## 10. Tooling Contract

Static tooling continues to operate on the AST and project model:

- `marreta lint` must not require templates;
- `marreta fmt` must not read templates;
- generated OpenAPI docs must not depend on templates;
- VS Code tooling must not expose templates;
- scenario tests must not have template-specific syntax.

Templates are runtime internals only. If a future tool needs to inspect whether
a route can be templated, that inspection must be diagnostic metadata, not a new
language contract.

---

## 11. Validation Contract

Every implementation step must preserve correctness before measuring speed.

Required runtime validation after each structural template change:

```bash
cargo fmt --check
cargo check
cargo test
cargo build
```

New tests must be added as the engine grows:

- unit tests for template build success/fallback decisions;
- unit tests for frame slot assignment;
- unit tests proving unsupported constructs fall back;
- functional tests proving template and AST paths return identical results for
  supported route shapes;
- error tests proving source span parity when a templated operation fails.

No implementation step may skip tests to speed up delivery.

In addition to runtime tests, every meaningful implementation milestone must run
the examples suite from `marreta-lang-examples` before merge. This is mandatory
because templates touch runtime execution semantics:

```bash
cd /home/thiago/Dev/Git/marreta-lang-examples/functional_tests && ./test.sh
cd /home/thiago/Dev/Git/marreta-lang-examples/migrations_functional && ./test.sh
cd /home/thiago/Dev/Git/marreta-lang-examples/smart_inventory && ./test.sh
cd /home/thiago/Dev/Git/marreta-lang-examples/ecommerce && ./test.sh
cd /home/thiago/Dev/Git/marreta-lang-examples/init_functional && ./test.sh
```

For route templates specifically, functional validation must run with templates
disabled and enabled:

```bash
cd /home/thiago/Dev/Git/marreta-lang-examples/functional_tests && ./test.sh
cd /home/thiago/Dev/Git/marreta-lang-examples/functional_tests && MARRETA_ROUTE_TEMPLATE_MODE=on ./test.sh
```

The runtime repository must keep container-based functional tests out of its own
test suite. Runtime tests should use Rust unit tests and lightweight generated
Marreta projects where possible. Full container orchestration belongs in the
`marreta-lang-examples` repository.

Template parity tests are required for each newly supported construct. At
minimum, the first route-template delivery must compare AST and template output
for:

- map responses;
- list responses;
- string, number, boolean, and null values;
- string escaping;
- path parameter reads;
- interpolated strings;
- supported pure value methods.

The comparison must cover status and JSON body equivalence. Header parity must
be added when template response headers become supported.

If an optimization touches shared runtime behavior rather than only template
build/execution, run the full runtime validation:

```bash
cargo fmt --check
cargo check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build
```

### 11.1 Template Parity Tests

The implementation must include a way to force execution modes in tests:

```text
MARRETA_ROUTE_TEMPLATE_MODE=off
MARRETA_ROUTE_TEMPLATE_MODE=on
```

Exact variable name may change, but tests need deterministic control. Relying on
auto-selection alone makes parity tests fragile.

Required parity strategy:

- run the same supported routes with templates disabled;
- run the same supported routes with templates enabled;
- assert identical status/body/headers;
- assert identical error kind and source span for supported failures;
- assert unsupported routes still execute through AST fallback.

This mode flag is internal/runtime diagnostic surface. It is not language
syntax.

---

## 12. Acceptance

The template work is acceptable only if:

- route behavior is unchanged;
- unsupported routes continue through the AST interpreter;
- linter, formatter, OpenAPI, scenario tests and editor tooling are unaffected;
- profiling identifies which routes used templates and which fell back;
- simple in-memory benchmark routes show a measurable drop in route execution
  overhead;
- route-local composition hot paths (`task`, `pipeline`, `broadcast`) are either
  materially improved or explicitly explained by profiler data;
- any remaining gap against Node/Fastify is explained by profiler data rather
  than speculation.

Beating Node/Fastify remains a strategic target. The initial route-template cut
improved simple routes, but the expanded in-memory benchmark shows that real
route bodies can still spend most of their incremental cost in composition
constructs. Therefore this spec should not be considered delivered until the
composition gap is addressed or measured as intentionally out of scope with a
clear rationale.

Minimum delivery target:

- supported routes execute through template mode when enabled;
- unsupported routes fall back cleanly;
- no functional or examples regression;
- profiling clearly distinguishes `template` vs `ast_fallback`;
- in-memory HTTP benchmark shows measurable latency improvement for simple route
  shapes and for route-local composition shapes;
- endpoint-level benchmark data identifies where Marreta wins, where it loses,
  and what runtime subsystem explains the delta.

---

## 13. Open Questions

Resolved for the first cut:

1. Template execution starts behind an internal runtime flag. It must not be
   enabled by default until parity tests and examples validation have run across
   enough route shapes.
2. Fallback reason is exposed only in profiling output for now. `marreta doctor`
   should not grow template diagnostics in the first cut because templates are
   private runtime internals, not user-facing project health.
3. The first template frame should use existing `Value` semantics where possible
   and avoid introducing a new visible value variant.
4. String interpolation pre-parsing may be consumed by this spec if already
   present, but this spec must not depend on a broad interpolation refactor.

Open:

1. Should template build happen during project load for every route, or lazily on
   first request after route matching? Current implementation builds during
   project load, which preserves request-time determinism.
2. Should the first implementation support schema-coerced payload routes, or only
   routes without schema binding? Current implementation keeps schema/body
   coercion out of the first delivery.

---

## 14. Implementation Attack Plan

Spec 049 profiling is merged. Saturation data is recorded. The next step is a
small experimental branch that implements route templates incrementally while
preserving AST parity.

Recommended order:

1. Build the route-template scaffold: eligibility analysis, frame slots, lowered
   route steps, mandatory AST fallback, and execution-mode profiling.
2. Support the smallest useful route set: literal replies, maps, lists,
   assignments, path params, interpolated strings, and a narrow allowlist of pure
   value methods.
3. Add direct JSON serialization inside the template response path only. This
   should remove intermediate `serde_json::Value` construction for supported
   template responses while leaving the AST path untouched.
4. Run a benchmark at non-saturating load, currently `1000 rps`, with templates
   off and on. This measures nominal latency delta without confusing results
   with saturation artifacts.
5. Expand template expression support with simple unary and binary operations:
   arithmetic, comparisons, and boolean operators. Each operator needs AST vs
   template parity tests.
6. Add simple guards only after expression support is stable: `require ... else
   fail` first, then `if` branches only when every branch terminates in
   `reply` or `fail`. Complex branches, rescue, external calls, and dynamic
   control flow continue to fall back.
7. Add `query.*` and `headers.*` reads only after path param behavior is stable.
   Do not add schema/body coercion in this first route-template delivery.
8. Support terminal `fail` after response parity is proven. Non-terminal `fail`
   must fall back because mixed template/AST control flow is out of scope.
9. Run final validation with Rust tests, release build, and examples functional
   tests with templates off/on.

Current measured checkpoint after terminal `fail` support:

- Rust validation: `cargo fmt --check`, `cargo check`, `cargo test --lib`, and
  `cargo build --release` pass.
- Examples functional validation: 548/548 pass with templates off.
- Examples functional validation: 548/548 pass with
  `MARRETA_ROUTE_TEMPLATE_MODE=on`.
- Non-saturating Marreta-only benchmark at `1000 rps` for `2m`:
  - templates on: avg 0.389ms, p50 0.388ms, p95 0.566ms, p99 0.688ms,
    peak CPU 24.29%, peak memory 43.03MiB, error rate 0%;
  - templates off: avg 0.438ms, p50 0.436ms, p95 0.636ms, p99 0.761ms,
    peak CPU 27.99%, peak memory 41.04MiB, error rate 0%.

Expanded in-memory benchmark checkpoint (`1000 rps`, `2m`, 15 route shapes):

- Marreta: avg 0.598ms, p50 0.545ms, p95 1.011ms, p99 1.248ms,
  peak CPU 44.63%, peak memory 68.38MiB, error rate 0%;
- FastAPI: avg 0.727ms, p50 0.663ms, p95 1.183ms, p99 1.650ms,
  peak CPU 54.07%, peak memory 44.37MiB, error rate 0%;
- Fastify: avg 0.369ms, p50 0.369ms, p95 0.545ms, p99 0.693ms,
  peak CPU 41.37%, peak memory 44.75MiB, error rate 0%.

Endpoint-level p95 from the expanded benchmark shows the next runtime targets:

| Endpoint group | Marreta p95 | FastAPI p95 | Fastify p95 | Reading |
|---|---:|---:|---:|---|
| simple/request/operator/string/list routes | 0.716-0.815ms | 1.117-1.274ms | 0.505-0.662ms | Marreta beats FastAPI but trails Fastify. |
| `tasks` | 0.904ms | 1.159ms | 0.509ms | Direct task calls add visible overhead. |
| `pipeline` | 1.109ms | 1.197ms | 0.519ms | Pipeline composition is a clear Marreta hotspot. |
| `broadcast` | 1.140-1.267ms | 1.127-1.148ms | 0.507-0.510ms | Broadcast is expensive for tiny in-memory work. |

This benchmark checkpoint is a local engineering signal, not a public claim. It
confirms the first route-template cut helped simple routes, but also shows the
spec should remain open: route-local `task`, `pipeline`, and `broadcast` costs
are now the dominant visible gaps in this benchmark.

Additional implementation phases before marking this spec delivered:

Architectural review confirms this remains part of Spec 050 rather than a new
spec. The unit of purpose is the HTTP route hot path. Route-local `task`,
`pipeline`, and `broadcast` constructs are common inside real Marreta route
bodies, so delivering only literal JSON route templates would create a partial
and misleading performance result.

The boundary remains route-local:

- route-local task calls may be lowered into the route template frame;
- route-local pipelines may be lowered only when every stage is statically safe;
- route-local broadcast may be lowered only as semantically parallel branch execution;
- standalone task execution, consumers, async drivers, and external I/O remain
  outside this spec.

10. Task call fast path inside templated routes. Resolve direct calls to private
    or exported tasks during project load where the callee is statically known.
    Avoid rebuilding full interpreter environments for simple pure task calls.
11. Pipeline fast path for simple in-memory pipelines. Lower scalar `>>` chains
    and list `>> map ... keep ...` forms where all stages are pure and
    statically supported. Avoid unnecessary intermediate allocations where
    possible.
12. Broadcast fast path for simple route-local task branches. Broadcast remains
    semantically parallel. Lower only static pure task branches, execute them in
    parallel, and preserve result ordering. Do not introduce automatic sequential
    execution in this delivery. Any future sequential/thresholded strategy
    requires a separate proof that branches are pure and that error/cancellation
    semantics remain identical to the AST interpreter.
13. Re-run the expanded benchmark and examples validation after each phase. Do
    not merge to main until the complete 050 delivery has a meaningful, measured
    improvement against the expanded benchmark.

### Phase 11 Measurement - Pipeline Map Fast Path

After lowering route-local linear task pipelines and `map ... keep` pipelines,
the expanded in-memory benchmark at 1000 rps for 30 seconds produced:

| Run | avg | p50 | p95 | p99 |
| --- | ---: | ---: | ---: | ---: |
| Before Phase 11 | 0.553ms | 0.485ms | 0.971ms | 1.240ms |
| Phase 11 | 0.533ms | 0.477ms | 0.937ms | 1.188ms |

Endpoint-level impact on the main target route:

| Endpoint | Before p95 | Phase 11 p95 | Notes |
| --- | ---: | ---: | --- |
| `/pipeline` | 1.057ms | 0.726ms | `map ... keep` no longer forces AST fallback. |
| `/tasks` | 0.693ms | 0.689ms | Stable; direct task call optimization preserved. |
| `/broadcast/chain` | 1.221ms | 1.208ms | Still dominated by broadcast semantics/fallback. |

This is a meaningful local improvement, but the aggregate benchmark still trails
Node/Fastify (`p95 0.545ms`) because broadcast endpoints remain expensive. The
next optimization work should focus on route-local broadcast while preserving
parallel semantics and project-load fallback for unsupported forms.

### Phase 12 Measurement - Parallel Broadcast Fast Path

After lowering simple route-local broadcast branches to parallel template
branches, the expanded in-memory benchmark at 1000 rps for 30 seconds produced:

| Run | avg | p50 | p95 | p99 |
| --- | ---: | ---: | ---: | ---: |
| Phase 11 | 0.533ms | 0.477ms | 0.937ms | 1.188ms |
| Phase 12 | 0.487ms | 0.435ms | 0.866ms | 1.161ms |

Endpoint-level impact on the route-local composition endpoints:

| Endpoint | Phase 11 p95 | Phase 12 p95 | Notes |
| --- | ---: | ---: | --- |
| `/broadcast/scalar` | 1.067ms | 0.880ms | Static task branches lowered to template execution. |
| `/broadcast/list` | 1.156ms | 1.101ms | Improved, but still dominated by branch scheduling and result assembly. |
| `/broadcast/chain` | 1.208ms | 1.161ms | Improved without changing parallel semantics or output order. |
| `/pipeline` | 0.726ms | 0.690ms | Pipeline fast path preserved. |
| `/tasks` | 0.689ms | 0.645ms | Direct task fast path preserved. |

The implementation is intentionally not a sequential shortcut. It preserves the
public broadcast contract by executing eligible branches in parallel and
collecting results in declaration order. Unsupported broadcast forms continue to
fall back to the AST interpreter at project load.


### Post-Diagnosis Direction - Direct Baseline, AST Fallbacks, and HTTP Extractors

The benchmark suite now separates direct in-memory composition from broadcast
composition:

- `direct_*` endpoints are the primary comparison baseline for route execution
  latency because they represent serial in-memory work with equivalent semantics
  across Marreta, FastAPI, and Fastify;
- `broadcast_*` endpoints remain valuable, but they must be analyzed as a
  separate concurrency benchmark because Marreta `*>>` has explicit parallel
  branch semantics and pays scheduling/synchronization costs that direct loops in
  other runtimes do not pay.

This separation is required for benchmark fairness. The main Spec 050 acceptance
signal should use direct endpoints by default. Broadcast endpoints should be
included only when the analysis is explicitly about broadcast semantics.

Profiler data from the direct benchmark shows that the largest internal Marreta
costs are still routes that fall back to the AST interpreter. The current
priority remains language/runtime execution work before HTTP stack tuning:

- lower list literals containing task calls, for example
  `[double(10), triple(10)]`;
- lower task calls whose arguments come from local slots or temporary values,
  for example `list_length(data)`;
- lower simple pipeline map shapes that operate on locally constructed lists.

These changes directly target `/direct/list` and `/direct/chain`, which are the
largest remaining internal hot spots in the direct benchmark. The expected gain
is measurable because comparable template routes already execute in tens of
microseconds internally, while those fallback routes currently cost hundreds of
microseconds internally.

There is also an opportunistic HTTP-stack optimization worth testing later:
specialized Axum handlers. Today every route uses a universal handler signature
that extracts path params, query params, headers, trace context, and body bytes,
even when a route does not use them. A future controlled experiment may register
route handlers with narrower extractor sets based on static route requirements.
Examples:

```text
pure route        -> no path/query/body extractors
path-only route   -> Path extractor only
body-only route   -> Bytes/body extractor only
```

This is a valid hypothesis because it attacks per-request framework overhead
outside the Marreta language interpreter. However, it must not be mixed blindly
with template lowering work. It changes the server registration path, so it
requires isolated benchmarks and the same examples validation gate. If pursued,
add profiler labels or benchmark runs that make the delta attributable to
handler specialization, not to template execution.

Explicitly do not combine this delivery with:

- `Value::Map` storage refactors;
- auth/allow templating;
- db/doc/cache/queue/http-client templating;
- schema-coerced payload templating;
- standalone consumer/task templates outside route execution;
- broad interpreter rewrites.

Those may be future optimizations, but mixing them into the first
route-template branch would make correctness and performance deltas harder to
validate.

---

## 15. Complementary Analysis - Where the Fastify Gap Actually Lives

This section records a focused review of the current branch against the
`marreta-lang-bench` in-memory HTTP results, including the hot-path profiler
output. Its purpose is to re-aim the remaining 050 work with evidence, because
several recent strategies on the branch (tokio worker count, middleware removal)
did not move the benchmark and that non-result is itself a signal.

### 15.1 Method

Sources cross-referenced:

- `results/20260527T050-final-compare` (1000 rps, 30s, templates on): per-endpoint
  `http_req_duration` p95/p99 for Marreta vs Node/Fastify vs FastAPI;
- `results/20260527Tdirect-profile-on` hot-path profiler output: per-route
  internal phase timings (`total_execute_route`, `ast_execute`,
  `template_execute`, `env_setup`, ...);
- `results/20260527Tworkers-{1,2,4}` and `results/20260527Tdirect-no-middleware`:
  failed optimization experiments;
- `apps/marreta/routes/api.marreta` vs `apps/node/server.js`: equivalent route
  bodies.

### 15.2 Decisive Finding

For template-eligible routes the interpreter is no longer the bottleneck. The
profiler shows internal route execution far below the measured HTTP latency:

| Route | Mode | Internal `total_execute_route` (avg) | HTTP p95 Marreta | HTTP p95 Node |
|---|---|---:|---:|---:|
| `/health` | template | 4.5 us | 0.562 ms | 0.568 ms |
| `/item/:id` | template | 10.7 us | 0.623 ms | 0.570 ms |
| `/summary` | template | 11.3 us | 0.639 ms | 0.568 ms |
| `/tasks` | template | 14.3 us | 0.634 ms | 0.567 ms |
| `/lists` | template | 19.7 us | 0.649 ms | 0.563 ms |
| `/pipeline` | template | 21.0 us | 0.692 ms | 0.567 ms |

On `/health` the engine runs in 4.5 us and Marreta wins the p95 outright. Across
templated routes the interpreter accounts for roughly 1-3% of measured p95
latency. The remaining ~97% is the fixed HTTP-stack floor (hyper/axum/tokio,
socket, kernel, container scheduling, k6 measurement), which is shared and nearly
identical to Fastify's floor.

Conclusion: the template work already won the language-level battle for simple
routes. Further interpreter or template micro-optimization on routes that already
match or beat Node is wasted effort.

This also explains why the recent branch experiments produced no benefit. They
targeted the shared floor, not a Marreta-specific cost:

- tokio workers 1 -> 2 -> 4: aggregate p95 0.741 -> 0.730 -> 0.726 ms (noise);
- middleware removal: aggregate p95 0.720 vs 0.711 ms (noise).

At sub-millisecond scale on localhost, p95 deltas around 50 us are within
scheduling noise. Language-level decisions must be driven by the internal
profiler (`total_execute_route`, `ast_execute`), not k6 wall-clock.

### 15.3 The Two Real, Attackable Costs

The aggregate gap to Fastify is not "the language is slow". It is two specific
costs that drag the average up.

**Cost A - Broadcast via OS threads (largest aggregate offender).**
`template/ops.rs` lowers broadcast with `std::thread::scope` + `scope.spawn` per
branch. Spawning and joining OS threads for microsecond-scale pure work costs an
order of magnitude more than the work itself, plus synchronization and cache
effects. Node uses `Promise.all` over already-resolved promises on its event
loop, which is effectively free. This is why broadcast endpoints sit at
0.855-1.145 ms versus Node ~0.57 ms and pull the aggregate p95 to 0.861 ms.

**Cost B - Routes that still fall back to the AST interpreter.**
Fallback routes pay full interpreter instantiation (`env_setup` ~14 us) plus
tree-walking:

| Route | Internal exec | Why it falls back |
|---|---:|---|
| `/operators` | 57.6 us | pure arithmetic, but ineligible (reason to confirm) |
| `/control/:tier` | 55.5 us | `match` statement not templated |
| `/direct/list` | 192.6 us | list literal of task calls + task args from local slots |
| `/direct/chain` | 231.0 us | same family as `/direct/list` |

`/direct/list` and `/direct/chain` are 20-40x the cost of a templated route and
are exactly the shapes flagged in the Post-Diagnosis Direction above.

### 15.4 Attack Plan

Prioritized by measured return on investment.

1. **Rebuild the broadcast execution strategy (highest aggregate win).**
   Stop spawning fresh OS threads for cheap pure branches. Either execute
   statically pure template branches sequentially, or run them on a shared thread
   pool, while preserving the public parallel contract and declaration-order
   result assembly. Section 12 already requires that any non-parallel strategy
   prove branch purity and identical error/cancellation semantics; in the
   template path branches are statically known pure tasks, so that proof is
   tractable. Precondition: run a broadcast-focused profile pass first - the
   current profiler run recorded `requests: 0` for broadcast routes, so we have
   no internal microsecond baseline to measure the change against.

2. **Eliminate the remaining fallbacks by widening eligibility.**
   - `/operators`: investigate the exact `TemplateFallbackReason`. Likely causes
     are `null or "default"`, `(a > b) and true`, or the `In` operator path that
     currently returns `Null` in `ops.rs`. Templating this drops it from ~57 us
     to ~15 us.
   - `/direct/list` and `/direct/chain`: lower list literals containing task
     calls and task calls whose arguments come from local slots/temporaries.
     Largest possible internal drop (192-231 us -> ~15-20 us).

3. **Accept the HTTP floor; stop optimizing routes that already win.**
   Health/item/summary already match or beat Node with no language headroom
   left. If the floor itself becomes the target later, that is specialized Axum
   handler territory (per-route extractor sets), but the branch's own
   no-middleware experiment suggests limited upside. Deprioritize until A and B
   are done.

4. **Measurement hygiene (transversal).**
   - Drive runtime decisions from profiler microseconds, not k6 p95, for anything
     language-level.
   - Ensure observability parity in comparative runs. The
     `20260527Tdirect-profile-on` run had request logging enabled (one JSON log
     line emitted per request), while the Node app runs with `logger: false`.
     Comparative numbers should disable Marreta request logging and trace context
     for an apples-to-apples floor.

### 15.5 Expected Outcome

If A and B land, the broadcast endpoints and the four fallback routes converge
toward the templated-route profile (sub-30 us internal), which removes the
endpoints currently dragging the aggregate p95. The simple templated routes are
not expected to improve further, because they are already floor-bound and already
competitive with Node. Acceptance for these items should be stated as internal
profiler deltas plus a re-run of the expanded benchmark, not as a promise to beat
Fastify on the shared HTTP floor.

---

## 16. Final Assessment (Outcome and Decision)

> Status of this spec: **Concluded as a successful experiment whose
> implementation is intentionally NOT merged.** Its findings are carried forward
> by Spec 051.

### 16.1 What was delivered and measured

The route fast path was implemented end to end and the post-diagnosis gaps were
closed: literal replies, guards, terminal fail, arithmetic/boolean/comparison
operators (including division by a provably non-zero divisor and non-boolean
`and`/`or`), subscript access (`List[Integer]`, `Map[String]`), local task calls
with slot arguments, pipeline maps (including task calls inside `keep`/`skip`),
and broadcast (scalar/list/chain). With the fast path enabled, every benchmark
route executes through it.

Measured on the in-memory benchmark (1000 rps, 30s, 1 CPU / 1 GiB), comparing the
same binary with the fast path off (pure AST) versus on:

| Metric | AST only | Fast path | Delta |
|---|---:|---:|---:|
| aggregate p95 | 1.040 ms | 0.628 ms | ~1.66x |
| aggregate p99 | 1.318 ms | 0.784 ms | ~1.68x |
| aggregate avg | 0.581 ms | 0.411 ms | ~1.41x |
| CPU avg | 40.9% | 24.4% | -40% |

Per-route speedups ranged from ~1.3x (simple routes: interpreter setup avoided)
to 1.6-2.2x (composition: broadcast, pipeline, direct task/list chains). Against
the comparison runtimes the fast path moved Marreta from clearly behind to a
statistical tie with Node/Fastify on p95 (0.628 vs ~0.622) and ahead on p99
(0.784 vs 0.933), while roughly halving FastAPI. Memory was not a fast-path win
(~89 MiB vs ~94 MiB; within noise). Parity was preserved throughout: the full
functional suite (548 cases) passes with the fast path both off and on, and
template-vs-AST parity tests cover every supported construct.

### 16.2 The decisive finding

The fast path's advantage does **not** come from a fundamentally better execution
model. It comes from removing per-request overhead that the tree-walking
interpreter pays unnecessarily:

1. cloning the interpreter environment per request;
2. resolving variables by name through a hash map instead of by slot index;
3. converting `Value -> serde_json::Value` before serializing the response;
4. re-entering a general interpreter for route-local composition
   (`task`/`pipeline`/`broadcast`).

These are overheads, not model limitations — and (1), (2), (3) are retrofittable
into the single interpreter directly.

### 16.3 Why the implementation is not merged

Delivering the fast path means shipping a **second execution engine** that
re-implements a growing subset of the interpreter's semantics plus a parallel
static type system, with mandatory hand-maintained parity. This split is the
root of a maintenance liability that this work exercised twice in practice
(division-by-zero: AST raised, template returned Null; `and`/`or`: AST truthiness
vs boolean-only). Both were caught, but they demonstrate the hazard is live: the
further the templatable surface grows, the more it becomes a duplicate
interpreter, and the parity burden scales with it.

The project's direction is a **single execution approach that is also the most
performant one** — not two engines behind a default-off flag. The fast path is
therefore concluded as a proof of value, not a delivered feature.

### 16.4 Decision and carry-forward

- The fast-path implementation (the `template/` module and its runtime
  integration) is **not merged** to the runtime `main`. The AST interpreter on
  `main` is left intact and remains the single source of truth.
- The work branch is **tagged for reference** (lowering and parity details) so
  Spec 051 can reuse the analysis.
- **Spec 051** picks up §16.2's findings to optimize the single AST interpreter
  and retire the fast-path concept entirely (one engine, no flag, no fallback).
- Independently useful artifacts produced along the way **are kept**: the runtime
  Docker image (`Dockerfile`) and the containerized example/benchmark harness,
  which are not coupled to the fast path.

This is the honest close: the experiment answered its question (the wins are
real, ~1.7x latency / ~40% CPU, and they are overhead-removal) and that answer
makes a dedicated fast path unnecessary — the same wins belong in the one engine.
