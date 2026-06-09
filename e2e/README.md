# e2e — Marreta Lang feature suite

**Load-bearing CI fixture, not a tutorial.** This in-memory project exercises the
Marreta Lang language features end to end over HTTP. Changes here must keep the
**Release E2E** workflow green.

It uses no external service (no database, document store, cache, queue, or JWKS):
everything runs with only the `marreta` binary, a POSIX shell, and `curl`.

## Run it locally

```bash
cargo build --release
bash e2e/run.sh target/release/marreta
```

The meaningful, per-endpoint assertions live in the **scenario tests** under
`tests/`, one concern per file (per the [conventions](../docs/guide/reference/conventions.md)), written as a real app would:
`then status` plus `then response is { body: { ... } }`, `given` to mock
`http_client`/`auth`, and `with { ... }` for payloads.

`run.sh` lints the project, runs those scenario tests (`marreta test`), then serves
it and **smoke-tests the live HTTP path**, focusing on what the scenario runner
cannot exercise: real `api_key` hashing, real query/header/raw binding, real
`http_client` self-calls over loopback, the `rescue` recovery path, and the
generated docs.

Because every declared route has a meaningful scenario, `marreta test --coverage`
and the `marreta doctor` Tests section both report 100% route coverage as a
consequence (not as the goal).

## In CI

The `Release E2E` workflow is **manual** (`workflow_dispatch`) and takes a release
`tag`, like the smoke test. It downloads the published binary for that tag and runs
this suite across the full OS matrix (Linux x86_64/arm64, macOS arm64, macOS
x86_64 under Rosetta, and Windows via WSL). It is the deep companion to the release
smoke test, validating the published artifact exercises the language on every
platform.

## Coverage

Coverage is driven by the built-in catalog (`marreta tooling catalog`) so that every
in-memory language function is exercised:

- **type methods** (all of them): `string.*`, `list.*`, `map.*`, `integer.*`,
  `float.*`, `decimal.*`.
- **namespace functions** (in-memory ones): `math`, `json`, `base64`, `uuid`,
  `time`, `fs`, `log`, `feature`.
- **`http_client`** all five verbs (`get`/`post`/`put`/`patch`/`delete`) via
  self-call over localhost.
- **language constructs**: `match`, `if/else`, `require`/`reject`, `raise`/`rescue`,
  pipelines, `map`/`keep`/`skip`, `reduce`, broadcast `*>>`, subscript,
  interpolation, conditional assignment, `or`/`and`/`not`, request binding (path
  params, query, headers, `take raw`, JSON body), schema contracts (validation +
  422, including `enum`/`decimal`/`datetime`/nested/typed-list), responses (`reply`
  json/html/text, `fail`), tasks (inline `=>`, block with implicit return,
  composition, recursion), auth (`api_key` live, `jwt` HMAC via scenario mock), and
  generated `/openapi.json` + `/docs`.

Out of scope (need external components, covered by `docs/examples/functional_tests`):
`cache`, `queue`, `topic`, `db`, `doc`, and `jwt` via JWKS.

Because it tracks the catalog, this suite is a candidate **delivery gate**: a new
in-memory language function should not ship without a check here.
