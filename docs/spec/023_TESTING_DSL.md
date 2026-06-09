# 023 - API Scenario Testing

Status: Delivered

Delivery notes:

- `marreta test` discovers `tests/**/*_test.marreta`, loads the production
  project from `./app.marreta`, and executes real routes in memory.
- Scenario files support contextual `scenario`, `given`, `when`, `then`, and
  `returns` DSL words without reserving them globally in production code.
- External drivers for `db`, `doc`, `cache`, `queue`, and `http_client` are
  replaced by strict scenario mocks.
- `then status` and partial `then response is` assertions are implemented with
  `anything` support.
- `--list`, `--filter`, `--coverage`, elapsed timing, explicit non-convention
  file notes, and functional examples under `examples/functional_tests/tests/`
  are implemented.
- Review hardening landed before merge: reusable `given` matches, duplicate
  `given` rejection, clear `returns anything` failure, production-shape
  `db.native_query(...)` mocking, and unused-given messages with arguments.
- Follow-up polish is tracked in `023b_API_SCENARIO_TESTING_HARDENING.md`.

## Motivation

Marreta is a language for building REST APIs. Its first testing feature should
therefore validate API behavior, not expose a generic unit-test framework.

The developer should be able to describe a request scenario in the same style as
the application code: clear, low-ceremony, and focused on the public HTTP
contract.

The core idea:

```marreta
scenario "create order"
    given db.products.find(10) returns { id: 10, price: 100 }
    given db.orders.save(anything) returns { id: 99, total: 100 }

    when POST "/orders" with {
        product_id: 10,
        quantity: 1
    }

    then response is {
        status: 201,
        body: {
            id: 99,
            total: 100
        }
    }
```

This validates the real production route in-memory, while replacing external
infrastructure at the driver boundary. Tasks used by the route are exercised as
part of the scenario. Direct task/unit testing is intentionally not part of the
initial feature.

## Goals

- Add `marreta test` as the canonical scenario test command.
- Discover scenario files by convention from `tests/**/*_test.marreta`.
- Load the production project from `./app.marreta`, exactly like `marreta serve`
  and `marreta doctor`.
- Execute scenarios against real production routes in-memory.
- Add `scenario "name"` blocks.
- Add `given` declarations for expected external dependency interactions.
- Add `when METHOD "/path"` request execution.
- Add `then response is ...` and `then status ...` response assertions.
- Keep external components strict: unconfigured calls to `db`, `doc`, `cache`,
  `queue`, or `http_client` fail the scenario.
- Keep the initial feature deterministic and single-process.

## Non-goals

- Unit tests for tasks.
- Direct calls to production tasks from tests.
- Generic `expect` assertions as the primary test API.
- A separate `http_test.*` object.
- Call-count assertions such as `called db.orders.save once`.
- Real infrastructure integration tests.
- Load tests or performance tests.
- Snapshot testing.
- Line coverage reporting.
- Watch mode.
- Parallel scenario execution.
- `setup` / `teardown` hooks.
- Private task or private module testing.
- Test-specific production entrypoints.
- Replacing `doctor` as the config/connectivity validation command.

## Required Semantics

These are language/runtime obligations, not recommendations.

1. `marreta test` requires `./app.marreta` in the current directory.
2. The production project is loaded with the same project loader used by
   `marreta serve`.
3. Automatically discovered test files must match `tests/**/*_test.marreta`.
4. Test files are loaded in a scenario scope and cannot replace production
   routes, tasks, schemas, or config.
5. A scenario executes exactly one `when` request.
6. `when` dispatches in-memory through the real `RouteRegistry`; it must not
   open a TCP port.
7. Routes, `take`, validation, internal tasks, `reply`, `fail`, and uncaught
   error handling are the real production behavior.
8. External components must be declared through `given` before they are used.
9. Any unconfigured external call fails the scenario with a clear message.
10. User-controlled HTTP responses from `reply` and `fail` must not be modified
    by the test runner.
11. Each `scenario` block starts with an empty `given` registry.
12. `then response is ...` performs partial response matching by default.
13. Multiple `then` assertions are allowed and evaluated in declaration order.
14. Every `given` declaration must be consumed at least once by the scenario.
15. `scenario`, `given`, `when`, `then`, and `returns` are contextual scenario
    DSL words. Outside their scenario positions they remain normal identifiers.

The key rule:

```text
Scenarios validate the public REST contract.
Production code is loaded from app.marreta.
External infrastructure is replaced by given declarations.
```

## Executability

Scenario files are executable Marreta test files. They are not documentation-only
specifications.

The `when` statement is the executable request instruction. When `marreta test`
runs a scenario, it dispatches the `when` request in-memory against the real
route registry loaded from `app.marreta`.

The developer does not call the route manually. The scenario runner calls it:

```text
marreta test
  -> load app.marreta
  -> register production routes
  -> load tests/**/*_test.marreta
  -> for each scenario:
       -> install given-backed external drivers
       -> execute the when request against the real route registry
       -> capture the response
       -> evaluate then assertions
       -> verify every given was consumed
```

## File Convention

The default discovery convention is:

```text
tests/**/*_test.marreta
```

This is mandatory for automatic discovery. Files that do not match this pattern
are ignored by `marreta test` unless passed explicitly on the command line.

Recommended organization:

| Production route file | Scenario file |
|---|---|
| `routes/orders.marreta` | `tests/orders_test.marreta` |
| `routes/products.marreta` | `tests/products_test.marreta` |
| `app.marreta` | `tests/app_test.marreta` |

This mapping is a convention for readability and reporting only. A file named
`tests/orders_test.marreta` does not implicitly import or bind
`routes/orders.marreta`. The route under test is resolved by the loaded
production project and the request declared in `when`.

Explicit paths are allowed:

```bash
marreta test tests/custom_scenario.marreta
```

When an explicit path is provided, the file does not need to match
`*_test.marreta`. This is useful for local experiments, generated tests, or
focused CI commands.

## CLI

Initial command surface:

```bash
marreta test
marreta test tests/orders_test.marreta
marreta test --filter "create order"
marreta test --list
marreta test --coverage
```

Semantics:

- `marreta test` discovers and runs `tests/**/*_test.marreta`.
- `marreta test <path>` runs only the provided scenario file.
- `--filter TEXT` runs scenarios whose names contain `TEXT`.
- `--list` prints the loaded project scenario surface without executing
  scenarios.
- `--coverage` prints API coverage after execution: scenarios, assertions,
  declared `given` interactions, covered routes, and uncovered routes.
- Test runs print elapsed time at the end.
- Explicit files that do not match `tests/**/*_test.marreta` are allowed, but
  the CLI explains that they are not part of automatic discovery.

Coverage is API coverage, not line coverage. A route is covered when a passing
scenario resolves its `when` request to that production route.

Example `--list` output:

```text
Project: ecommerce-api v1.0.0
Entrypoint: app.marreta

Loaded routes:
  GET /health -> app.marreta:8
  POST /orders -> routes/orders.marreta:3

Scenario files:
  tests/orders_test.marreta
    scenario "create order" -> line 1
```

## Scenario Blocks

Basic syntax:

```marreta
scenario "health"
    when GET "/health"

    then status 200
```

Rules:

- `scenario` opens a scenario block.
- The scenario name must be a string literal.
- Scenario names must be unique within the same file.
- The same scenario name may appear in different files; reports always include
  the file path to disambiguate.
- A scenario may declare `given` dependency responses.
- A scenario must declare exactly one `when` request.
- A scenario must declare at least one `then` assertion.
- Multiple `then` assertions are allowed and evaluated in declaration order.
- The scenario fails at the first failing `then`.
- A scenario block only accepts `given`, `when`, and `then` steps.

There is no `setup` or `teardown` in 023. Scenarios should be self-contained.
Common scenario data should be expressed directly in request bodies, response
matchers, or `given` return values.

## Given

`given` declares an expected external dependency interaction and how it responds
during the current scenario.

```marreta
given db.products.find(10) returns { id: 10, price: 100 }
given db.orders.save(anything) returns { id: 99, total: 100 }
given cache.get("cart:1") returns null
given doc.orders.find("abc") returns { id: "abc" }
given queue.push "orders.created", anything returns true
given db.native_query("SELECT 1") returns [{ value: 1 }]
given http_client.get("https://payments.example/status") returns {
    status: 200,
    body: { ok: true }
}
```

`given` uses the same call shapes as production Marreta code. It should not add
test-only syntax or remove production syntax. For example, `db`, `doc`, `cache`,
and `http_client` use method calls, while `queue.push` / `topic.publish` use the
same producer syntax accepted by route code.

Supported targets for 023:

- `db.*`
- `doc.*`
- `cache.*`
- `queue.*`
- `http_client.*`

Matching rules:

- exact argument matching is supported
- `anything` matches any value inside `given` and response matchers
- `anything` is a scenario matcher token, not a normal runtime value; it cannot
  be assigned to variables, passed to production code, or used as a `returns`
  value
- duplicate `given` declarations for the same target and same matcher are an
  error within the same scenario
- the same `given` may be matched multiple times during the scenario
- every `given` must be matched at least once by the scenario
- when multiple `given` declarations match the same external call, the most
  specific matcher wins; exact values are more specific than `anything`

Unconfigured external calls fail:

```text
Unconfigured db call: db.products.find(10)

Add:
  given db.products.find(10) returns ...
```

This protects local runs and CI from accidentally depending on Postgres,
MongoDB, Redis, RabbitMQ, or external HTTP services.

Unused `given` declarations fail the scenario:

```text
Unused given: queue.push("orders.created", anything)

The scenario declared this external interaction, but production code did not
execute it.
```

This gives API scenarios a low-ceremony way to validate important side effects
without introducing a separate `then called ... once` assertion in the initial
feature. Exact call counts remain deferred.

For unit-returning production operations such as `queue.push` and
`topic.publish`, the `returns` value is only used to make the `given` declaration
complete and readable. The route still receives the normal unit result.

## When

`when` declares the HTTP request to execute against the real production route
registry.

Supported methods:

```marreta
when GET "/products"
when POST "/orders" with { product_id: 10, quantity: 1 }
when PUT "/orders/99" with { status: "paid" }
when PATCH "/orders/99" with { status: "cancelled" }
when DELETE "/orders/99"
```

Request bodies declared with `with` are serialized as JSON before in-memory
dispatch. This mirrors JSON API requests. For routes using `take raw`, a string
body such as `with "plain text"` reaches the route as JSON string text,
including the quotes. Native plain-text/raw scenario request bodies are deferred.

Headers:

```marreta
when POST "/orders" with {
    product_id: 10,
    quantity: 1
} and headers {
    authorization: "Bearer test-token"
}
```

Query strings are expressed in the path:

```marreta
when GET "/products?active=true&page=2"
```

The `when` request must behave as if the route was called over HTTP, except that
dispatch is in-memory:

```text
when POST "/orders"
  -> real RouteRegistry
  -> real route body
  -> real take bindings
  -> real validation, tasks, reply/fail behavior
  -> given-backed external driver boundary
  -> captured response
```

## Then

`then` validates the captured HTTP response.

Minimal status assertion:

```marreta
then status 201
```

Response assertion:

```marreta
then response is {
    status: 201,
    body: {
        id: anything,
        total: 100
    }
}
```

Header assertion:

```marreta
then response is {
    status: 200,
    headers: {
        "content-type": "application/json"
    }
}
```

`then response is ...` uses partial matching by default:

- fields listed in the expected response must match
- extra response fields are allowed
- nested maps are matched recursively
- list values must match exactly unless a later spec adds list matchers
- `anything` matches any response value

This keeps common API assertions low-ceremony while avoiding brittle full-body
matching by default.

Example:

```marreta
scenario "create order"
    given db.products.find(10) returns { id: 10, price: 100 }
    given db.orders.save(anything) returns { id: 99, total: 100, status: "created" }

    when POST "/orders" with { product_id: 10, quantity: 1 }

    then response is {
        status: 201,
        body: {
            id: 99,
            status: "created"
        }
    }
```

The real response may include additional body fields, such as `total` or
`created_at`, without failing the scenario.

## Error Scenarios

Expected application errors are validated like any other HTTP response.

```marreta
scenario "reject order without product"
    when POST "/orders" with { quantity: 1 }

    then response is {
        status: 400,
        body: {
            error: "product_id required"
        }
    }
```

The test runner must not add metadata to responses produced by `reply` or
`fail`. Extra diagnostic information is only printed in the test report when
production code raises an uncaught runtime error.

## Runtime Architecture

The scenario runner should use the existing runtime instead of creating a
parallel execution model.

Required runtime pieces:

- scenario file discovery
- parser support for `scenario`, `given`, `when`, and `then`
- scenario registry with name, file, line, and body
- scenario runtime mode for replacing infrastructure drivers with given-backed
  drivers
- in-memory HTTP dispatcher over the real route registry
- partial response matcher
- scenario reporter

External dependency flow:

```text
production route
  -> db/doc/cache/queue/http_client operation
  -> given registry lookup
  -> configured value returned
```

## Scenario Isolation

Each `scenario` block is isolated:

- `given` declarations are scoped to the current scenario
- the given registry starts empty for each scenario
- duplicate `given` declarations are checked inside the current scenario only

Two different scenarios may declare the same dependency independently:

```marreta
scenario "create order"
    given db.orders.save(anything) returns { id: 1 }
    when POST "/orders" with { product_id: 10 }
    then status 201

scenario "create another order"
    given db.orders.save(anything) returns { id: 2 }
    when POST "/orders" with { product_id: 20 }
    then status 201
```

The second scenario does not inherit `given` declarations from the first
scenario.

## Reporting

Passing run:

```text
Project: ecommerce-api v1.0.0

PASS tests/health_test.marreta
  PASS health

PASS tests/orders_test.marreta
  PASS create order

2 passed, 0 failed
```

Failing run:

```text
Project: ecommerce-api v1.0.0

FAIL tests/orders_test.marreta
  FAIL create order

tests/orders_test.marreta:9
then response is { status: 201, body: { id: 99 } }
       status was 500

1 passed, 1 failed
```

Exit codes:

- `0` when all selected scenarios pass
- `1` when any scenario fails or scenario discovery/loading fails

## Implementation Plan

### Phase 1 - CLI and discovery

1. Add `marreta test` command.
2. Resolve `./app.marreta` using the same project-root rule as `serve`.
3. Discover `tests/**/*_test.marreta`.
4. Add `marreta test <path>`, `--filter`, `--list`, and `--coverage`.
5. Report missing test directory as `0 passed, 0 failed`, not as a project
   error.

### Phase 2 - scenario AST

1. Add parser support for `scenario "name"` blocks.
2. Add parser support for `given TARGET returns VALUE`.
3. Add parser support for `when METHOD "path"` with optional `with` and
   `and headers` blocks.
4. Add parser support for `then status CODE`.
5. Add parser support for `then response is MAP`.
6. Track scenario file, line, and name.

### Phase 3 - in-memory HTTP execution

1. Load the production project exactly like `serve`.
2. Build an in-memory request from the `when` declaration.
3. Dispatch through the real route registry.
4. Capture `{ status, body, headers }`.
5. Ensure `reply` and `fail` response bodies are not modified by the test
   runner.
6. Preserve Marreta stack output for uncaught production errors.

### Phase 4 - given-backed external drivers

1. Add given declarations for `db`, `doc`, `cache`, `queue`, and `http_client`.
2. Replace external drivers with strict given-backed drivers in scenario runtime
   mode.
3. Fail unconfigured external calls with actionable messages.
4. Support exact argument matching and `anything`.
5. Reject duplicate given declarations in a scenario.
6. Fail scenarios that declare a `given` that is never consumed.

### Phase 5 - response assertions and reporting

1. Implement `then status`.
2. Implement partial recursive matching for `then response is`.
3. Support multiple `then` assertions evaluated in declaration order.
4. Support `anything` inside response matchers.
5. Produce useful failure output with file, line, scenario name, expected value,
   and actual value.
6. Implement `--filter`, `--list`, and `--coverage` reporting.

### Phase 6 - examples and documentation

1. Add an API scenario testing example under `examples/`.
2. Add scenario files that cover success and error responses.
3. Add given examples for each external component family.
4. Document the recommended workflow for Marreta developers.
5. Update `CHANGELOG.md` and `docs/spec/SPEC.md` when the implementation lands.

## Test Plan

### Phase 1

- `marreta test` fails clearly outside a Marreta project.
- `marreta test` discovers `tests/**/*_test.marreta`.
- `marreta test <path>` runs an explicit file outside the naming convention.
- `marreta test --filter TEXT` selects matching scenario names.
- `marreta test --list` prints loaded routes and discovered scenario files
  without running scenarios.
- `marreta test --coverage` prints route coverage based on passing scenarios.

### Phase 2

- valid `scenario` blocks parse.
- scenario names must be unique within the same file.
- a scenario with no `when` fails during loading.
- a scenario with more than one `when` fails during loading.
- a scenario with no `then` fails during loading.
- a scenario with multiple `then` assertions parses and executes in order.
- invalid `given`, `when`, and `then` syntax produces source-located errors.

### Phase 3

- `when GET` reaches a real production route.
- `when POST` passes payload and headers to a real production route.
- route params and query params are populated correctly.
- route `reply` body is returned unchanged.
- route `fail` body is returned unchanged.
- missing route returns a scenario failure with a clear route-not-found message.
- uncaught `raise` inside production code renders the Marreta stack in the
  scenario report.

### Phase 4

- `given db.*` returns the configured value.
- `given doc.*` returns the configured value.
- `given cache.*` returns the configured value.
- `given queue.*` returns the configured value.
- `given http_client.*` returns the configured response.
- unconfigured external call fails with an actionable message.
- unused `given` declaration fails with an actionable message.
- duplicate `given` declaration fails during scenario loading.
- `anything` matches any argument value.
- `anything` cannot be used as a normal runtime value.

### Phase 5

- `then status 200` passes when the response status is `200`.
- `then status 200` fails when the response status differs.
- `then response is` matches exact scalar fields.
- `then response is` allows extra body fields.
- `then response is` recursively matches nested maps.
- `then response is` compares lists exactly.
- `then response is` supports `anything` as a response matcher.
- multiple `then` assertions stop at the first failure.
- failure output includes file, line, scenario name, expected value, and actual
  value when available.

### Phase 6

- `cargo test --lib`
- `cargo test --bin marreta`
- existing `examples/functional_tests/test.sh --docker`
- new scenario testing example script
- documentation examples can be copied into a Marreta project and run

## Developer Workflow

Recommended workflow for a Marreta developer:

1. Put production API code under normal project files loaded by `app.marreta`.
2. Put scenario files under `tests/`.
3. Name scenario files with `*_test.marreta` so `marreta test` discovers them.
4. Use `scenario` to describe a public API behavior.
5. Use `given` for every external dependency touched by the route.
6. Use `when` to declare the HTTP request.
7. Use `then response is` or `then status` to validate the public HTTP
   contract.

Example project layout:

```text
app.marreta
routes/orders.marreta
routes/products.marreta
tests/orders_test.marreta
tests/products_test.marreta
marreta.env
```

Example command:

```bash
marreta test
```

## Deferred

- direct task/unit tests
- optional `given` declarations for interactions that may or may not happen
- exact call-count assertions such as `then called db.orders.save once`
- shared route matcher with the production HTTP router
- generic boolean assertions
- schema contract assertions such as `expect_valid`
- strict full-response matching such as `then response is_exactly`
- status set assertions such as `then status in [200, 201]`
- private task tests with explicit module binding
- parallel scenario execution
- line coverage reports
- snapshot assertions
- watch mode
- property-based tests
- real-infrastructure integration test mode
- richer given sequencing such as `then returns`
- editor integration for scenario discovery and source links
