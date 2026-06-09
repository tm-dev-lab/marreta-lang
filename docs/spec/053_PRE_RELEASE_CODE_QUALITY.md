# 053 - Pre-Release Code Quality & Hardening

> Status: Delivered (the deferred root README has since landed; see §11)
> Type: Code quality / release readiness
> Scope: Lint, formatting, cleanup, structure, and release hygiene across the
> runtime ahead of the first public release — with **no change to language
> behavior** and **no weakening of the test suites**.

---

## 1. Purpose

MarretaLang is approaching its first public release. Before the source is
public, the codebase should hold up to outside scrutiny: clean linting, no
avoidable panics on the runtime paths, justified `unsafe`, navigable module
sizes, and the basic packaging a public Rust project is expected to ship
(license, README, CI, publish metadata).

This spec is the **analysis + plan**. Implementation happens afterward on a
dedicated branch, landed incrementally and measured against the existing gates.

## 2. Motivation

The work in 049–051 made the runtime fast and the engine single. What remains
before going public is presentation and robustness hygiene — the things a
first-time reader of the repo judges in the first five minutes, and the things
that turn a malformed request into a process panic instead of a clean error.

Hard constraint, carried from prior specs: the unit and functional suites must
**keep passing at the end without being altered to pass**. A test may only be
changed if the change is discussed and agreed first, and never to mask a real
regression (see [[feedback-never-rewrite-failing-tests]]).

## 3. Current State (analysis)

Measured on `main` at the time of writing (counts are indicative, to be
re-derived during implementation):

**Clean already:**
- `cargo fmt --check` passes.
- 0 `TODO`/`FIXME`/`HACK`/`XXX` in `src/`.
- 0 `dbg!` leftovers.
- Exactly one `#[allow(...)]` (`clippy::too_many_arguments`) — no suppression
  abuse.
- 0 references to benchmarks, sibling repos, machine paths, or internal
  strategy in `src/` (the [[feedback-no-repo-references-in-code]] rule holds).
- Every `pub mod` in `lib.rs` carries a module doc comment.

**To address:**

1. **Clippy is not clean.** ~70 warnings in the library plus ~2 in the binary,
   and ~20 `clippy::approx_constant` **errors** in inline test code (literals
   like `3.14159…` that clippy wants replaced with `std::f64::consts::PI`).
   Library warnings are mostly mechanical: collapsible `if` (~32), `map_or`
   simplification (~6), `args.get(0)` → `.first()` (~6), useless `vec!` (~4),
   needless `return` (~3), items after a test module (~3), overly complex types
   (~2), large `Err` variants (~2), and a few one-offs.

2. **Panic surface.** ~1230 `.unwrap()/.expect()/panic!` occurrences in `src/`,
   most in tests. Non-test, library-path occurrences are concentrated in
   `parser.rs` (~71), `interpreter.rs` (~26), and `value.rs` (~15). Any of these
   reachable from untrusted input (source text, request payloads, env/config)
   should become typed errors, not panics.

3. **`unsafe` is undocumented.** 15 `unsafe` blocks, **all** wrapping
   `std::env::set_var`/`remove_var` (unsafe under edition 2024). Most are test
   helpers and config/timezone setup. None carry a `// SAFETY:` comment. One is
   a genuine concern: mutating `MARRETA_TIMEZONE` via `set_var` at request time
   in the interpreter is not thread-safe under the multi-threaded server.

4. **Oversized modules.** `interpreter.rs` is ~13.4k lines; `parser.rs` ~4.3k;
   `server.rs` ~3.5k; `value.rs` ~3.0k. Large single files hurt first-read
   navigability.

5. **Missing release packaging.** No `LICENSE` file (despite `license = "MIT"`),
   no root `README.md`, no CI workflows, no `CONTRIBUTING`, no MSRV pin. The
   `Cargo.toml` `[package]` has only name/version/edition/description/license —
   missing `repository`, `homepage`, `documentation`, `readme`, `keywords`,
   `categories`, `authors`, `rust-version`, and an `exclude` to keep
   `benchmarks/`, `spec/`, and `examples/` out of a published crate.

6. **No enforced lint posture.** No crate-level `#![deny/warn]` and no CI gate,
   so nothing prevents regressions in the above.

7. **CLI output is ad hoc.** ~224 `println!/eprintln!` across `src/`. Almost all
   are legitimate CLI I/O (lint/doctor/serve/error output), but they are not
   routed through a single layer; the set should be audited so it is provably
   intentional output, not stray prints.

## 4. Workstreams

Each is independently landable and independently validated. Ordered by
risk/leverage (cheap and mechanical first; structural last).

- **W1 — Mechanical clippy cleanup.** Apply the safe, behavior-preserving
  suggestions (collapsible `if`, `.first()`, drop useless `vec!`/`return`, move
  items before test modules, factor complex types). Prefer `clippy --fix` then
  review each hunk.
- **W2 — `approx_constant` in tests.** These are intentional test literals;
  replacing the value with `consts::PI` would change the assertion. Suppress
  locally with a scoped `#[allow(clippy::approx_constant)]` (or a module-level
  allow on the affected test modules). **Do not change the test values.**
- **W3 — Release packaging & repo hygiene.** Add `LICENSE` (MIT, matching
  `Cargo.toml`), a root `README.md` (what MarretaLang is, quickstart, links),
  complete the `Cargo.toml` publish metadata, pin an MSRV, and add an `exclude`
  list. Verify with `cargo publish --dry-run` (or `cargo package`).
- **W4 — CI gate.** A workflow running `cargo fmt --check`, `cargo clippy`
  (clean), and `cargo test`, plus the example functional suites against a
  freshly built image, so the cleanliness can't regress.
- **W5 — Panic audit.** Triage non-test `unwrap/expect` on input-reachable paths
  (parser, interpreter request handling, config/env loading, value coercion).
  Replace with typed errors where a malformed input could trigger them; leave
  (with justification) only those that are true invariants.
- **W6 — `unsafe` audit.** Add `// SAFETY:` comments to every block; consolidate
  the env-mutation helpers; eliminate the request-time `MARRETA_TIMEZONE`
  `set_var` in favor of passing the timezone through interpreter state (removes
  a real thread-safety hazard). Consider `#![warn(clippy::undocumented_unsafe_blocks)]`.
- **W7 — Lint posture.** Decide and set a crate-level lint stance (at minimum
  clippy-clean enforced by CI; optionally `#![warn(missing_docs)]` on the public
  surface) and document it.
- **W8 — Module decomposition (largest, last).** Split `interpreter.rs` (and
  optionally `parser.rs`, `server.rs`) into submodules by concern via **pure code
  movement** — no logic changes. This is the highest-churn item; do it on its own
  with tests green before and after, and keep diffs move-only.
- **W9 — CLI output audit.** Confirm every `println!/eprintln!` is intentional
  CLI I/O; remove any stray debug prints; optionally route through a small output
  helper for consistency.

## 5. Semantic Contract

- **No observable behavior change.** Same HTTP responses (byte-identical),
  same errors and spans, same CLI output semantics, same language behavior.
  This is hygiene, not a feature or perf spec.
- **Tests stay green and unaltered.** The unit suite, `functional_tests`, and
  `migrations_functional` must pass at the end. Tests are changed only when
  discussed and agreed, and never weakened to go green (W2's local `allow` is a
  lint suppression, not a test-logic change).
- **No performance regression** versus the 051 standing; W8's moves must not
  alter hot paths.

## 6. Non-Goals

- No new language syntax, features, or semantics.
- No public-API redesign beyond packaging/doc hygiene.
- No bytecode/JIT or further runtime optimization (that is a separate track).
- No dependency overhaul; only metadata/MSRV and obviously unused removals.
- No rewrite of tests to satisfy a lint or to paper over a finding.

## 7. Validation

Per change, the established gate:

- `cargo fmt --check`, `cargo clippy` (target: clean under the agreed lint set),
  `cargo test`;
- `cargo build --release`, copy to `~/.local/bin/marreta`, rebuild the
  `marreta-lang:dev` image and verify its binary sha;
- run `functional_tests` and `migrations_functional` against the rebuilt image;
- for W3, `cargo package` / `cargo publish --dry-run` succeeds and the packaged
  file list excludes `benchmarks/`, `spec/`, `examples/`.

See [[feedback-interpreter-functional-tests]] for the full per-change ritual.

## 8. Acceptance

- `cargo clippy --all-targets` is clean (0 warnings/errors) under the agreed
  lint configuration, including test code.
- A CI gate enforces fmt + clippy + tests so cleanliness cannot regress.
- `LICENSE` exists and `Cargo.toml` is publish-ready: `cargo package` excludes
  `benchmarks/`, `performance/`, `spec/`, `examples/`, and `editors/`. (A root
  `README.md` is **deferred** to a follow-up at the owner's request — the one
  open item; see §11.)
- Every `unsafe` block has a `// SAFETY:` justification (the suspected
  request-time timezone hazard turned out to be a lock-guarded test helper, so
  there was nothing to remove — see §11).
- The input-reachable panic audit (W5) is complete, with any remaining
  `unwrap/expect` justified as true invariants.
- Unit + functional + migrations suites pass, unaltered (except agreed W2
  lint suppressions).
- Oversized modules are reduced (target: no single `src/*.rs` dominating), via
  move-only refactors, if W8 is undertaken in this pass.

## 9. Sequencing

Incremental, lowest-risk first, validated after each: W1 → W2 → W3 → W4 →
W5 → W6 → W7 → W9 → W8. W8 (module decomposition) is the largest and is landed
last, on its own, as move-only commits.

## 10. Relationship To Other Specs

- Builds on the delivered single engine (Spec 051); does not change its
  behavior or performance.
- Honors [[feedback-never-rewrite-failing-tests]],
  [[feedback-no-repo-references-in-code]], and
  [[feedback-interpreter-functional-tests]].

---

## 11. Delivery Notes

Implemented on branch `feature/pre-release-code-quality-053`. Every step was
validated with `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, and
the unit/integration suites; the interpreter-touching work (W6, W8) also passed
the containerized `functional_tests` (548) and `migrations_functional` suites.
No test was weakened to pass.

- **W1/W2 — clippy clean.** Mechanical fixes + `FromStr for QueueProvider`;
  architecture-inherent lints centralized in `[lints.clippy]`; `approx_constant`
  suppressed only in the affected test modules (intentional sample literals).
- **W3 — packaging.** `LICENSE` + full publish metadata (`cargo package`
  verified, excludes benchmarks/spec/examples). The README was initially deferred
  at the owner's request, then delivered as a landing-page README on the
  `docs/readme-zero-ceremony` branch (merged in `23872d1`), which also promoted
  `CONVENTIONS.md` to the repo root, added branding, set `publish = false`, and
  replaced CI with manual build and release workflows. The `readme` manifest key
  was restored. No open acceptance items remain.
- **W4 — CI gate.** `.github/workflows/ci.yml` runs fmt + clippy(`-D warnings`)
  + tests.
- **W5 — panic audit: no input-reachable panics found.** parser has zero real
  `unwrap` (the count was the `expect(kind)` combinator); interpreter/value
  `unwrap`s are all lock-poison or guarded invariants; every `expect("…")` is a
  documented invariant. No code change required.
- **W6 — every `unsafe` block documented** with a `// SAFETY:` comment (all are
  `std::env` mutation in startup/test contexts plus one trace-guard pointer
  deref; the suspected request-time timezone hazard turned out to be a
  lock-guarded test helper).
- **W7 — lint posture.** clippy enforced clean via CI; `missing_docs`
  deliberately not enabled (MarretaLang ships as a CLI/runtime, not a library
  API).
- **W9 — CLI output audit.** All `println!/eprintln!` are intentional CLI I/O
  (command output, the `print` builtin, the `parse` debug command); no stray
  debug. No change.
- **W8 — module decomposition.** `interpreter.rs` 13,392 → 3,088 lines, in two
  move-only phases: (1) the ~6.5k lines of `#[cfg(test)]` modules to sibling
  files; (2) the 167-method `impl` into `operators`/`access`/`namespaces`/
  `infra`/`pipeline` submodules (cross-module entry points widened to
  `pub(super)`).
- **Extra — test flakiness fixed.** The HTTP integration test helpers waited a
  fixed sleep for the server to bind; replaced with a readiness poll so the
  suite (and the new CI gate) is deterministic under parallel load.
