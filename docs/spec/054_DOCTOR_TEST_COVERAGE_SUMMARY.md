# 054 - Doctor Test Coverage Summary

> Status: Delivered
> Type: Tooling / developer experience
> Scope: Add a consolidated, static test-presence summary to `marreta doctor`,
> reusing the scenario loading and route matching already built for
> `marreta test`. Doctor does not execute scenarios, does not list routes, and
> points to `marreta test --coverage` for detail.

---

## 1. Purpose

`marreta doctor` is the project health check: it reports project structure,
intent, persistence, auth, feature flags, and optional connectivity. Today it
says nothing about tests, so a developer cannot tell from a health check whether
the API has scenario tests at all.

`marreta test --coverage` already answers the detailed question (which routes are
covered by scenarios that passed, with per-route listing). What is missing is a
quick, at-a-glance signal in the place developers already look before a commit or
a release: "do my endpoints have tests, and how many do not?".

This spec adds a small `Tests` section to the doctor report with consolidated
numbers only, and orients the developer to `marreta test --coverage` for the rest.

## 2. Design Principles

1. **Static, not executed.** Doctor never runs scenarios. It reports test
   *presence* (which declared routes have at least one declared scenario), not
   pass/fail coverage. Computing pass/fail requires running the suite, which is
   the job of `marreta test --coverage`.
2. **Consolidated only.** The section reports counts. It does not list covered or
   uncovered routes. The per-route breakdown stays in `marreta test --coverage`.
3. **No misleading percentage.** Any ratio is labeled as "routes with a scenario",
   never as a bare "coverage" that could be read as "tested and green".
4. **Reuse, do not duplicate.** The scenario loaders and the scenario-to-route
   matching already exist for `marreta test`. Doctor reuses them as-is.
5. **Informational, never fatal.** A low or zero test presence is a note, not a
   doctor failure. It must not change doctor's exit behavior.

## 3. What Doctor Reports

### 3.1 Output shape

A new section in the doctor report:

```
Tests
  scenarios declared: 12 across 4 files
  routes with a scenario: 9 / 11 (81.8%)
  routes without a scenario: 2
  run `marreta test --coverage` for per-route detail and pass/fail coverage
```

When there are no scenarios:

```
Tests
  scenarios declared: 0
  routes with a scenario: 0 / 11 (0.0%)
  routes without a scenario: 11
  run `marreta test --coverage` for per-route detail and pass/fail coverage
```

The trailing instruction line is rendered without a status tag (a plain,
informational line), unlike the tagged metric lines above it.

### 3.2 Metrics defined

- **scenarios declared:** total scenarios across the project test files. The
  "across N files" count is the number of files that hold **at least one declared
  scenario**. A discovered file with zero scenarios does not count toward N. (A
  malformed file cannot reach this point: `load_project` already parses every
  `.marreta`, so the command would have failed at load, see §5.1.)
- **routes with a scenario:** count of declared routes that at least one declared
  scenario resolves to via static route matching, over the total declared routes.
  Shown as `X / Y` with a labeled percentage (the line stays "routes with a
  scenario", so the percentage reads as presence, never as a bare "coverage"; see
  §10).
- **routes without a scenario:** the complement, as a single number (no list).
- **scenarios with no matching route:** shown **only when greater than zero**, as a
  consolidated count. It flags stale scenarios whose `when VERB "/path"` resolves
  to no declared route.

All counts are derived statically. A scenario counts toward "routes with a
scenario" only when its `when VERB "/path"` resolves to a declared route, using
the same matching the runner uses (see §5).

## 4. Static vs Run-Based Coverage

There are two distinct, complementary signals. This is intentional and must be
kept clear so the two never contradict each other:

| | `marreta doctor` (this spec) | `marreta test --coverage` |
|---|---|---|
| Runs scenarios | No | Yes |
| Signal | Test *presence* (declared) | Coverage by scenarios that *passed* |
| Detail | Consolidated counts only | Per-route covered/uncovered lists |
| Cost | Static, no infra | Executes the suite with mocks |

Doctor answers "is there a test for this endpoint at all?". `--coverage` answers
"which endpoints are exercised by tests that pass?". Doctor explicitly points to
`--coverage` for the authoritative, run-based view.

## 5. Reuse Plan

The work that matters (loading scenarios and matching them to routes) already
exists in `src/scenario_tests.rs` and is already separated from execution. Doctor
reuses three pieces:

1. **Scenario loading:** `discover_scenario_files` plus per-file
   `load_scenario_file`, the same parsing `marreta test` uses, without executing
   anything. These files are already valid at this point (see §5.1).
2. **Route matching:** the static resolution of a scenario `when VERB "/path"` to
   a declared route. It already exists as `scenario_plan` / `find_route` in
   `src/scenario_tests.rs`, and is already static (no scenario is run to produce a
   plan), but both are **private** today. Expose a single thin public helper, for
   example `plan_scenario_route_presence(routes, scenario) -> Option<RouteKey>`,
   that wraps the existing matching, and have doctor call it. Doctor must **not**
   reimplement route matching, and the internals must **not** be made public ad
   hoc beyond that one helper.
3. **A small extracted consolidation helper.** Today `print_api_coverage` takes
   `&[ScenarioRun]` (run results) and computes counts inline. Extract the counting
   into a shared summarizer that takes the set of routes and a set of covered
   route keys plus the scenario/file totals (and the unmatched-scenario count),
   and returns a `CoverageSummary` struct. Then:
   - `marreta test --coverage` builds the covered set from scenarios that
     **passed** (current behavior, unchanged output).
   - `marreta doctor` builds the covered set from scenarios that are **declared**
     (via the public presence helper from item 2, not the private internals).
   Both render from the same `CoverageSummary`, so the numbers are computed one
   way only.

The only genuinely new code is: wiring doctor to load the scenarios, extracting
the summarizer, and rendering the consolidated section. No new parser, no new
matching logic, no execution path in doctor.

### 5.1 Loading note

`load_project` (used by `serve`, `test`, and `doctor`) recursively parses **every**
`.marreta` file under the project root, including the ones in `tests/`. So a
malformed scenario file fails the command at load with a clear parse error, the
same way it does for `serve` and `test`. This is not a doctor-specific concern and
is intentionally **not** special-cased here.

A consequence: by the time the `Tests` section runs, the doctor report is built
from an already-loaded project, so every discovered scenario file is known to
parse. The section therefore counts scenarios from files that load and does not
need a tolerant, per-file fallback or a parse-error note. It still loads via
`discover_scenario_files` + `load_scenario_file` to reuse the runner's parsing and
to read the `scenario` blocks it needs for matching.

One case is **not** gated by project load: reading the `tests/` directory itself.
`collect_recursive` ignores `read_dir` errors (`Err(_) => return`), so an
unreadable `tests/` directory slips past project load. The Tests section therefore
surfaces a discovery I/O error as a soft `SKIP` note instead of silently reporting
"no tests". This keeps `has_errors` unchanged.

## 6. Non-Goals

- **No line, statement, or branch coverage** of route bodies. This spec is route
  (endpoint) presence only.
- **No pass/fail coverage in doctor.** Doctor does not execute scenarios.
- **No per-route listing in doctor.** Lists stay in `marreta test --coverage`.
- **No change to doctor exit semantics.** Test presence is informational.
- **No new flag.** The section is part of the standard doctor report.
- **No change to the `marreta test --coverage` output.** The refactor that
  extracts the summarizer must preserve the existing `--coverage` output exactly.
- **No special handling of malformed `.marreta` files.** `load_project` already
  parses every file (including `tests/`), so a malformed file fails the command at
  load like `serve` and `test` do. The Tests section does not try to tolerate or
  soft-note that case (see §5.1).

## 7. Implementation Notes

- Extract a summarizer (for example `coverage::summarize`) producing a
  `CoverageSummary { routes_total, routes_with_scenario, scenarios_total,
  files_total, unmatched_scenarios }`, plus the route-key formatting currently
  inlined in `print_api_coverage`. Keeping `unmatched_scenarios` in the struct
  keeps the counting and its tests centralized for both callers.
- `print_api_coverage` is refactored to build its covered set from passing
  `ScenarioRun`s and then call the summarizer for the headline numbers, keeping
  its per-route covered/uncovered lists.
- `build_doctor_report` gains a `Tests` `DoctorSection` built from the summarizer
  fed by statically planned scenarios. Doctor loads scenarios from the project
  root and tolerates load errors.
- Scenarios whose `when` path resolves to no declared route do not count toward
  "routes with a scenario". Per §10 (Decisions), surface this count only when it
  is greater than zero.

## 8. Test Plan

- **Unit:** `summarize` over fixtures with some routes covered, some not, and a
  scenario whose `when` matches no route. Assert `routes_total`,
  `routes_with_scenario`, `scenarios_total`, `files_total`, and
  `unmatched_scenarios`.
- **Regression:** the full `marreta test --coverage` block is asserted golden
  (headings, order, spacing, covered and uncovered sections) for the `marreta
  init` fixture, so any change to the output is caught.
- **Doctor integration:** a temporary project with routes and `tests/` scenarios
  produces a `Tests` section with the correct consolidated numbers, the pointer
  line (untagged), and no route names listed.
- **Edge:** no `tests/` and no scenarios reports `0 declared`. A malformed
  scenario file is not a Tests-section concern: `load_project` fails the command
  at load (like `serve` and `test`), so it is not tested here. An unreadable
  `tests/` directory (a discovery I/O error, which project load does not catch)
  produces a `SKIP` note, and that is tested.

## 9. Delivery Quality Gates (mandatory)

Marreta Lang is about to ship its first public release. These gates are not
optional for this spec, and they apply to every spec delivered from here on:

- **Unit tests are required.** Every new unit of behavior introduced by this spec
  (the extracted `summarize` helper, the doctor `Tests` section assembly, the
  scenario-to-route presence counting) ships with unit tests. "Delivered" means
  tested, not merely implemented.
- **`cargo fmt --check` must pass.** No unformatted code is merged.
- **`cargo clippy --all-targets -- -D warnings` must pass cleanly.** Lints are
  fixed at the source. Adding `#[allow(...)]` (or any other suppression) to
  silence a real lint, or narrowing the lint configuration to dodge it, is not
  acceptable. The only tolerated allowances are the architecture-inherent ones
  already justified centrally in `Cargo.toml`.
- **No bypass, no gambiarra.** The gates exist to keep the code honest. The
  implementation must not weaken or skip an existing test to go green, must not
  disable a check, and must not hack around a gate instead of fixing the root
  cause. This restates and reinforces the standing project rule against weakening
  tests.
- **Existing suites stay green.** Unit, integration, and the functional suites
  (`examples/functional_tests`, `examples/migrations_functional`) must pass after
  the change.
- **Update the docs on delivery.** When this spec is implemented, as part of the
  same delivery: flip its status to Delivered with a short delivery-notes block,
  add it to the `SPEC.md` index and follow-ups, and record the work in
  `CHANGELOG.md`. Do this at delivery time, not later.

If a gate cannot be met honestly, that is a signal to revisit the design or raise
it for discussion, never to bypass the gate.

## 10. Decisions

These were open questions during review and are now decided:

1. **Unmatched scenarios.** Report "scenarios with no matching route: K" **only
   when K is greater than zero**. It is a useful stale-test signal and stays
   consolidated (a count, not a list).
2. **Percentage.** **Show a labeled percentage** on the "routes with a scenario"
   line, for example `9 / 11 (81.8%)`. The review preferred `X / Y` alone, but the
   owner asked for the percentage; it is kept on the "routes with a scenario" line
   so it reads as presence and never as a bare "coverage" figure.
3. **Auto-discovery scope.** Use exactly the same default discovery as
   `marreta test`: `tests/**/*_test.marreta`, with **no filter and no new doctor
   flag**.

## 11. Delivery Notes

Implemented on branch `feature/doctor-test-coverage-054`.

- **Shared module `src/coverage.rs`:** `CoverageSummary { routes_total,
  routes_with_scenario, scenarios_total, files_total, unmatched_scenarios }` with
  `routes_without_scenario()` and `routes_with_scenario_pct()`, plus
  `summarize(...)` and `route_key(verb, path)`.
- **Public matching helper:** `scenario_tests::plan_scenario_route_presence(routes,
  scenario) -> Option<String>` wraps the private `scenario_plan` / `find_route`.
  No internals were made public beyond this one helper, and doctor does not
  reimplement matching.
- **`print_api_coverage` (`test --coverage`):** routed through
  `coverage::route_key` and `coverage::summarize` for the headline counts. Output
  is unchanged, guarded by a golden integration test of the full block.
- **Doctor `Tests` section:** `build_tests_section` loads scenarios via
  `discover_scenario_files` + `load_scenario_file` (already validated by
  `load_project`, see §5.1) and reports consolidated counts only (scenarios
  declared, routes with/without a scenario with a labeled presence percentage,
  unmatched only when greater than zero). A discovery I/O error (unreadable
  `tests/`, which project load does not catch) surfaces as a soft `SKIP` note. The
  trailing `marreta test --coverage` pointer renders without a status tag
  (`DoctorStatus::Plain`). The section is informational and never sets
  `has_errors`.
- **Validation (clippy 1.96, CI parity):** `cargo fmt --check` and `cargo clippy
  --all-targets -- -D warnings` clean; full suite 1461 lib + 3 bin + 35 HTTP + 98
  integration green; `examples/functional_tests` 548/548;
  `examples/migrations_functional` PASS. Unit tests added for `summarize` /
  `route_key` / `routes_with_scenario_pct`, `plan_scenario_route_presence` (match
  and miss), and the doctor section; integration tests for the golden
  `test --coverage` block and the doctor section. No test was weakened or bypassed.
