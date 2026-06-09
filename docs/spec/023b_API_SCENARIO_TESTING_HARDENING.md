# 023b - API Scenario Testing Hardening

Status: Delivered

Delivery notes:

- `given` matching now chooses the most-specific matching declaration.
  Exact-value matchers outrank `anything`, while ties keep declaration order.
- Scenario runtime supports `db.transaction` through a fake transaction wrapper
  over the same given-backed DB driver. No real database connection is opened.
- Scenario route matching has focused parity tests for root routes, path params,
  multiple params, missing segments, extra segments, and trailing slashes.
- Focused scenario tests now cover first-failing `then`, nested `anything`,
  `--filter` selection through `run_scenarios`, query/payload/header/raw
  bindings, non-string headers, computed `then status`, native query shape,
  duplicate `given`, and transaction execution.

## Motivation

023 delivered the first REST-first scenario testing slice. The remaining work is
not required for the initial merge, but it should be tracked explicitly so the
testing DSL does not accumulate ambiguous behavior.

This follow-up keeps the v1 identity of `marreta test`: API-first, low ceremony,
real route execution, and mocked external infrastructure.

## Goals

- Document and, if needed, improve `given` matcher precedence.
- Complete coverage of the 023 test plan with focused regression tests.
- Decide and implement scenario-runtime behavior for routes using
  `db.transaction`.
- Prove or improve route-matcher parity between the scenario runner and the
  production HTTP router.
- Keep the test DSL aligned with production call shapes for `db`, `doc`,
  `cache`, `queue`, and `http_client`.

## Non-goals

- Direct task/unit testing.
- Generic `expect` assertions as the primary API.
- Real-infrastructure integration mode.
- Snapshot testing.
- Watch mode.

## Scope

### Matcher Precedence

023 originally used declaration-order matching:

```marreta
given db.items.find(anything) returns { id: 0 }
given db.items.find(42) returns { id: 42 }
```

023b implements most-specific-wins matching. With the example above,
`db.items.find(42)` matches the exact `42` declaration, while other ids still
match `anything`.

Ties keep declaration order. Duplicate `given` declarations for the same target
and same matcher remain a scenario error.

### Scenario Runtime Transactions

023b implements a no-op transactional wrapper over the same scenario mocks.
Operations inside `db.transaction` still resolve through `given` declarations.
Commit and rollback are accepted by the fake transaction, and no real database
connection is opened.

### Route Matcher Parity

023 uses an in-memory route matcher for scenario execution. 023b adds focused
tests for non-trivial cases:

- literal routes vs param routes
- multiple params
- root route
- trailing slash behavior
- query-string stripping
- unsupported or ambiguous route shapes

Full shared matcher extraction remains deferred. If future route features make
parity hard to guarantee, the scenario runner should reuse shared route matching
code rather than maintaining an independent matcher.

### Test Plan Completion

Added focused tests for the remaining 023 plan items and reviewer notes:

- multiple `then` assertions stop at the first failure
- nested response matcher with `anything` in maps and lists
- `--filter` selection behavior
- API coverage report content is still covered by the functional suite
- query params in `when` URLs
- non-string headers fail clearly
- route `take` bindings for payload, query, headers, and raw body
- uncaught `raise` stack-context assertions remain deferred to a dedicated error
  reporting test pass
- computed `then status` values

## Validation Plan

- `cargo test --lib scenario_tests::tests`
- `cargo test --lib parser::tests::test_parse_api_scenario`
- `cargo test --bin marreta`
- `examples/functional_tests/test.sh`
- Manual `marreta test --coverage` in `examples/functional_tests`

## Exit Criteria

- Matcher precedence behavior is explicit and tested.
- Transaction behavior is implemented through a fake scenario transaction.
- Route matching has focused parity tests; shared matcher extraction remains
  deferred.
- The remaining high-value 023 test-plan items have focused coverage; uncaught
  `raise` stack-context assertions remain deferred.
