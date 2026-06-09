# 055 - Repository Layout: docs Grouping and the e2e Suite

> Status: Delivered
> Type: Repository organization / testing
> Scope: Group the non-runtime material under a single `docs/` directory, add a
> top-level `e2e/` directory next to `src/` and `tests/`, and add a manual,
> cross-platform workflow that serves an in-memory feature project and exercises
> the language over localhost against a published release binary. Amends the
> layout defined by Spec 052.

---

## 1. Purpose

The repository root mixes the runtime (`src/`, `tests/`) with material that
supports it but does not ship in the binary: `spec/`, `examples/`, `benchmarks/`,
`performance/`, `editors/`, `assets/`. New contributors cannot tell at a glance
what is the runtime and what is supporting material, and `examples/` invites casual
edits to projects that are actually load-bearing.

This spec does two related things:

1. **Declutter the root** by grouping the supporting material under `docs/`, so the
   root shows the runtime (`src/`, `tests/`), the end-to-end suite (`e2e/`), and
   `docs/` for everything else.
2. **Add an `e2e/` feature suite**: a single in-memory Marreta Lang project that
   exercises as many language features as possible through HTTP endpoints, plus a
   manual workflow that downloads a published release binary and runs the suite over
   localhost across the full OS matrix. It is the deep companion to the release
   smoke test (the smoke test proves a fresh project boots; the e2e suite proves the
   language features behave over HTTP, on every platform). It uses no external
   service, so it runs anywhere with only the binary.

The two are coupled: the cleanup makes room for `e2e/` as a clear, first-class
sibling of `src/` and `tests/`.

## 2. Layout Change

### 2.1 Target layout

```
marreta-lang/
├── Cargo.toml, README.md, CONVENTIONS.md, LICENSE, Dockerfile, CHANGELOG.md
├── .github/workflows/        (must stay at the root, required by GitHub)
├── src/                      (runtime, unchanged)
├── tests/                    (Rust integration tests, unchanged)
├── e2e/                      (new: in-memory feature suite + runner)
└── docs/
    ├── spec/                 (was spec/)
    ├── examples/             (was examples/)
    ├── benchmarks/           (was benchmarks/)
    ├── performance/          (was performance/)
    ├── editors/              (was editors/)
    └── assets/               (was assets/)
```

### 2.2 What moves and why

`spec/`, `examples/`, `benchmarks/`, `performance/`, `editors/`, and `assets/` all
move under `docs/` unchanged internally. `docs/` is the umbrella for "supporting
material that is not the shipped runtime". It is a loose fit for `benchmarks/` and
`examples/` (they are harnesses, not prose), but the goal is one obvious place for
everything that is not `src/`, `tests/`, or `e2e/`.

`.github/workflows/` stays at the root (GitHub requires it there).

### 2.3 Relationship to Spec 052

Spec 052 re-consolidated the split repositories into one engineering monorepo and
placed `spec/`, `examples/`, `benchmarks/`, `performance/`, and `editors/` at the
root. This spec keeps everything in the monorepo (the 052 principle holds: material
that versions and is validated against the runtime lives here) and only changes the
on-disk grouping. It **amends the layout** in Spec 052 section 2.1.

## 3. Migration Plan

The runtime and CI are unaffected: nothing in `src/`, `tests/`, or `.github/`
references these directories by path. **The risk is in the harness scripts inside
the moved directories**, which use depth-relative (`../..`) and cross-directory
paths that the extra `docs/` level breaks. The migration is therefore gated on a
full path sweep, not just the manifest and README.

1. **Sweep first.** Grep every `*.sh`, `*.yml`/`*.yaml`, `Dockerfile`, and
   `README`/`*.md` under the moved directories for `../..`-style relative paths and
   cross-directory references (one moved dir pointing at another, or at the repo
   root). For each hit, either fix the path for the new depth or explicitly declare
   that harness unsupported in this spec's delivery notes. Known offenders found
   during review (non-exhaustive; the sweep is authoritative):
   - `examples/init_functional/test.sh`: `REPO_ROOT="${SCRIPT_DIR}/../.."`.
   - `performance/tests/load/run.sh` and `run_doc.sh`: `ROOT_DIR="${SCRIPT_DIR}/../.."`.
   - `performance/tests/load/docker-compose.yml`: mounts `../../examples/ecommerce/seed.sql`
     and `context: ../..`.
   - `performance/tests/load/Dockerfile`: `CMD ["./marreta", "serve", "examples/ecommerce/app.marreta"]`.
   - `performance/tests/load/run_doc.sh`: `COMPOSE_FILE="$ROOT_DIR/examples/ecommerce/docker-compose.yml"`.
   - `examples/entra_id_auth/test_entra_id.sh`: `../../marreta-lang/target/...` (a
     stale pre-052 sibling-repo path; fix or declare unsupported).
2. **Move the directories** with `git mv` so history is preserved: `spec`,
   `examples`, `benchmarks`, `performance`, `editors`, `assets` into `docs/`.
3. **Fix the swept paths** so every still-supported harness resolves at the new
   depth (cross-references between moved dirs stay relative within `docs/`).
4. **`Cargo.toml`**: replace the per-directory `exclude` entries with `/docs` (and
   add `/e2e`), keeping `/.github`.
5. **`README.md`**: update every `assets/brand/images/...` reference (the mascot
   images, the gallery link) to `docs/assets/brand/images/...`, and update the
   repository-layout table to the new paths.
6. **Re-run every affected harness** after the move (see the checklist in §7), not
   only `functional_tests` and `migrations_functional`.
7. **Historical references**: old `CHANGELOG.md` entries that mention `docs/spec/`
   or `examples/` describe past state and are left as-is.

## 4. The e2e Suite

### 4.1 Principles

- **In-memory, no external service.** No `db`, `doc`, `cache`, `queue`, or JWKS,
  and no network beyond localhost. The only runtime dependencies are the `marreta`
  binary, a POSIX shell, and `curl`. Assertions use shell and `curl` only (no `jq`
  or Python): JSON checks are golden whole-body matches or `grep` substrings. The
  workflow guarantees `curl` is present on each OS (preinstalled on Linux/macOS,
  installed in the WSL leg).
- **Maximize language-feature coverage** through real HTTP endpoints.
- **Deterministic where possible.** Endpoints return fixed shapes so the runner can
  assert exact bodies, except where a value is inherently non-deterministic (uuid,
  time), which use substring or shape assertions.
- **Versioned as an example, not a per-push gate.** The project lives in `e2e/` and
  is exercised on demand (see the workflow). It is not run on every push.
- **Load-bearing notice.** The README states it exists to validate the runtime over
  HTTP and that changes must keep the e2e workflow green.

### 4.2 Feature coverage map

Routes are grouped by feature area. Indicative endpoints:

- **control_flow**: `match` with `fallback`, `if/else` (and `else if`),
  `require`/`reject` with `else fail`, early `reply`/`fail` termination.
- **transforms**: pipelines `>>`, `map` + `keep`/`skip`, `reduce`, broadcast `*>>`,
  list subscript, string interpolation `#{}`.
- **request_binding**: path params (`/:id`), query params, request headers,
  `take raw` (raw text body), and JSON body binding. These are core REST surfaces
  and fully in-memory.
- **contracts**: `take payload as Schema`, schema types (string, integer, boolean,
  decimal, float, datetime, optional `?`, lists, references), validation failure
  (422) on bad input.
- **generated docs**: assert `/openapi.json` (and `/docs`) respond and contain the
  declared routes. Generated OpenAPI is a public contract of the language and has
  regressed before, so a minimal assertion guards it. Fully in-memory.
- **responses**: `reply` json, `reply html`, `reply text`, status codes, `fail`,
  task return values (inline `=>` and block).
- **namespaces**: `math`, `json`, `base64`, `uuid`, `time`, `log`, `fs` (writing
  under a temp path), and `feature.enabled` (driven by `MARRETA_FEATURE_*`).
- **http_client**: routes that call the app's **own** localhost endpoints, covering
  GET/POST, headers, and status handling without any external dependency.
- **auth**: an `api_key` provider and a `jwt` provider configured with an HMAC
  secret (no JWKS/network). See 4.4 for how each is validated.

### 4.3 Structure

```
e2e/
├── app.marreta
├── schemas/
├── routes/
│   ├── control_flow.marreta
│   ├── transforms.marreta
│   ├── request_binding.marreta   # path params, query, headers, take raw, json body
│   ├── contracts.marreta
│   ├── responses.marreta
│   ├── namespaces.marreta
│   ├── http_client.marreta
│   └── auth.marreta
│   # /openapi.json and /docs are generated automatically (no route file)
├── tests/                 # scenario tests, also run via `marreta test`
├── marreta.env            # in-memory only: ports, feature flags, auth secrets/hashes
├── run.sh                 # takes a marreta binary, lint + test + serve + battery
└── README.md              # load-bearing notice
```

### 4.4 Auth coverage

- **`api_key`** is exercised live: the battery sends the configured header and
  asserts allowed vs forbidden vs missing-key responses.
- **`jwt` (HMAC)** is versioned as a configured example and validated through the
  project's scenario tests via `given auth.<provider>`. It is **not** exercised live
  over HTTP, since minting an HS256 token in the shell battery is not worth the
  complexity. The route exists to demonstrate the `jwt` surface.

### 4.5 The runner (`run.sh`)

Given a `marreta` binary, it runs `marreta lint`, `marreta test` (scenarios,
including the auth mocks), then serves the project and hits the endpoint battery
over `http://localhost:<port>` asserting status and body, then stops the server.
Assertions are golden (exact body) where deterministic, and substring or shape
checks for `uuid`/`time` routes. Shell scripts keep LF endings (enforced by the
existing `.gitattributes`).

### 4.6 Out of scope for e2e

`db`, `doc`, `cache`, `queue`, and `jwt` via `jwks_url` need external services and
stay covered by `docs/examples/functional_tests` and `migrations_functional` (with
Docker). The e2e suite is the infra-free subset focused on language behavior.

## 5. The e2e Workflow

A new `.github/workflows/e2e.yml`, modeled on the release smoke workflow:

- **Manual only** (`workflow_dispatch`) with a `tag` input (the release name), the
  same parameter shape as the smoke and release workflows. It is **not** a push/PR
  gate.
- **Full matrix**, identical to the smoke test: Linux x86_64, Linux arm64, macOS
  arm64, macOS x86_64 (run on the arm64 runner under Rosetta 2), and Windows via
  WSL (running the Linux x86_64 binary).
- **Steps**: check out the repo at the tag (for the `e2e/` project and `run.sh`),
  download the published release binary for that tag (as the smoke test does), then
  run `e2e/run.sh` against it (lint + test + serve + battery + stop). This validates
  the **published** binary exercises the full language surface on every platform.

## 6. Non-Goals

- **No runtime behavior change.** This is repository layout plus a new suite.
- **No per-push CI gate.** The e2e workflow is manual and matches the smoke test.
- **No external services in e2e.** Infra-bound features stay in `functional_tests`.
- **No live JWT over HTTP** in the battery (covered via scenario mocks instead).
- **No new umbrella beyond `docs/`** and **no move of `.github/`**.

## 7. Verification Checklist (mandatory)

The move must not break anything. Before delivery, all of the following pass:

- `cargo build --release` and `cargo test` green.
- `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` clean.
- The path sweep (§3.1) is complete: every `*.sh`, `*.yml`/`*.yaml`, `Dockerfile`,
  and `*.md` under `docs/` resolves at the new depth, or the harness is listed as
  unsupported in the delivery notes.
- Every still-supported harness passes after the move, not just the two below:
  - `docs/examples/functional_tests` → 548/548 (rebuild the `marreta-lang:dev`
    image first, since the Dockerfile copies the release binary).
  - `docs/examples/migrations_functional` → PASS.
  - `docs/examples/init_functional` → PASS.
  - `docs/performance/tests/load` scripts → run or explicitly declared unsupported.
- `cargo package --list` excludes `docs/` and `e2e/` (and still excludes
  `.github/`).
- Every README link and image resolves (no reference to the old `assets/...` path
  remains); the repository-layout table matches the new tree.
- `e2e/run.sh` passes locally against a freshly built binary (the workflow is then
  the same run on each platform).

## 8. Delivery Quality Gates (mandatory)

Per the standing pre-release policy, this spec ships with tests for new behavior
(`e2e/run.sh` plus the project's scenario tests), `cargo fmt --check` and `cargo
clippy --all-targets -- -D warnings` must pass with no suppressions added to dodge
them, and no existing test is weakened, skipped, or bypassed. If a gate cannot be
met honestly, revisit the design rather than work around it.

## 9. Delivery Documentation

On delivery, as part of the same work: flip this spec to Delivered with a short
delivery-notes block, add it to the `SPEC.md` index and follow-ups (noting it amends
Spec 052), and record it in `CHANGELOG.md`.

## 10. Decisions

Resolved during design:

1. **Umbrella name**: `docs/`.
2. **Auth depth**: `api_key` validated live; `jwt` (HMAC) versioned as an example
   and validated via scenario mocks, not live over HTTP.
3. **Workflow**: manual only, full OS matrix like the smoke test, taking the release
   name (`tag`) and validating the published binary. Not a per-push gate.

## 11. Delivery Notes

Implemented on branch `feature/docs-layout-and-e2e-055`.

- **Layout move**: `git mv` of `spec`, `examples`, `benchmarks`, `performance`,
  `editors`, `assets` into `docs/` (history preserved). `Cargo.toml` exclude becomes
  `/docs`, `/e2e`, `/.github`. README mascot image paths and the gallery link point
  to `docs/assets/...`, and the repository-layout table was updated (adds `e2e/` and
  `docs/*` rows).
- **Path sweep**: fixed `docs/examples/init_functional/test.sh` (`REPO_ROOT` from
  `../..` to `../../..`). `functional_tests` and `migrations_functional` use
  `SCRIPT_DIR`-relative paths and survived unchanged.
- **Declared outside the auto-validated harness set**:
  `docs/performance/tests/load` (its `ROOT_DIR=${SCRIPT_DIR}/../..` already resolved
  to `performance/`, not the repo root, before this move, and it needs a Docker/k6
  load rig) and `docs/examples/entra_id_auth` (needs a real IdP and Node, and carries
  a pre-052 `../../marreta-lang/...` fallback). Both stay in place as examples;
  repairing them is tracked separately, out of this spec's scope.
- **e2e suite**: the `e2e/` project plus `run.sh` (lint, the scenario tests, serve,
  and a focused live HTTP smoke for what the scenario runner cannot exercise, shell
  + curl only) and `.github/workflows/e2e.yml` (manual, full matrix, downloads the
  published binary by tag). The deep per-endpoint assertions live in the scenarios;
  the live smoke does not duplicate them. Coverage is **driven by the built-in
  catalog** (`marreta tooling catalog`, 129 entries) so every in-memory
  function is exercised: all 53 type methods (`string`/`list`/`map`/`integer`/
  `float`/`decimal`), the in-memory namespace functions (`math`/`json`/`base64`/
  `uuid`/`time`/`fs`/`log`/`feature`), `http_client` over all five verbs via
  self-call, language constructs (control flow, transforms, request binding,
  responses, error handling, expressions, tasks including inline/block/composition/
  recursion/inner), rich schema contracts (enum/decimal/datetime/nested/typed-list),
  auth (`api_key` live, `jwt` HMAC via scenario mock), and generated `/openapi.json`
  + `/docs`. External surfaces (`cache`/`queue`/`topic`/`db`/`doc`, `jwt` via JWKS)
  are excluded. The suite is a candidate **delivery gate**: a new in-memory function
  should not ship without a check here.
- **Validation**: `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`
  clean; `cargo test` 1461 lib + 3 bin + 35 HTTP + 98 integration; `cargo package
  --list` excludes `docs/` and `e2e/`; README links and images resolve.
  `docs/examples/functional_tests` 548/548, `migrations_functional` PASS,
  `init_functional` PASS. `e2e/run.sh` green: lint, 59 scenario tests (the deep
  per-endpoint assertions, one concern per file under `tests/`), and 17 live HTTP
  smoke assertions for what the scenario runner cannot exercise (real api_key
  hashing, real query/header/raw binding, real http_client self-calls, the rescue
  recovery path, generated docs). `marreta test --coverage` and the `marreta doctor`
  Tests section both report 51/51 routes (100%) as a consequence of the per-endpoint
  scenarios.
