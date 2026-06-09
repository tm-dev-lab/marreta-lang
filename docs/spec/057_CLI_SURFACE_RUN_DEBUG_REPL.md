# 057 - CLI Surface: Trim to the API Workflow

> Status: Delivered
> Type: CLI surface / developer experience
> Scope: Align the CLI with the language's purpose (exposing REST APIs). Remove
> the `run` script-runner and the `repl` interactive evaluator (both off-identity
> scripting surfaces), remove `run`'s now-redundant test harness with no runtime
> change, hide the `tokenize` and `parse` debug commands from the public help while
> keeping them callable, and make bare `marreta` print help.

---

## 1. Purpose

Four commands created at the start of the project were never revisited: `run`,
`repl`, `tokenize`, and `parse`. An audit found:

- All four are still functional. They delegate to the single canonical
  `Lexer` -> `Parser` -> `Interpreter` pipeline (the same one `serve`, `lint`,
  `fmt`, and `file_loader` use), so they track the language automatically. They are
  not stale.
- `tokenize` and `parse` print the token stream and the `{:?}` AST. They are
  compiler-internals debug tools with no value for someone writing an API, but real
  value for engine, lexer, and parser debugging (for example, the recent CRLF lexer
  fix was easy to confirm by eyeballing the token stream).
- `run` and `repl` both exercise only the **scripting subset** of the language
  (expressions, `task`, `match`, namespaces). Neither can exercise the core of the
  language (`route`, `schema`, `take`, `reply`, `http_client`, auth, `db`), because
  that needs a running server. `run` executes a script once and exits; `repl`
  evaluates the same subset interactively.

The language's identity is exposing REST APIs that run under `marreta serve`. A
script runner and an interactive expression evaluator are the two faces of the same
off-identity scripting surface. The real developer loop is `init` -> `serve` ->
hit endpoints or `test`, and exploration is done with a scratch route plus `serve`
or `test`, in context. This spec trims the CLI to the API workflow.

## 2. Remove the `run` command

`run` is removed from the CLI and the codebase.

### 2.1 Removal

- Remove the `run` subcommand arm and `run_file` from `src/main.rs`, and its line
  from the help text.
- Remove the CLI tests that exist only to exercise the `run` command's own argument
  handling (the missing-file and missing-argument cases). These test behavior that
  ceases to exist; removing them is not weakening language coverage.

### 2.2 The test harness used `run`, and that coverage is redundant

`tests/integration_tests.rs` had two helpers, `run_source` and `run_file`, that
shelled out to `marreta run` and asserted on `print` output (and in several cases on
error text and a non-zero exit code). They backed 72 of the 98 integration tests,
exercising the scripting subset (arithmetic, interpolation, `task`, block tasks,
`match`, ranges, `while`, pipelines, broadcast, `require`/`reject`/`fail`, and the
runtime error cases). Nothing else depends on `run`: the shell suites
(`functional_tests`, `migrations_functional`) and the example projects use `serve`
and `test`.

The first instinct was to keep these tests by migrating the harness to in-process
library execution, which would have required adding an output sink to the
`Interpreter` purely to capture `print`. That was rejected: bending the **runtime**
to serve a test harness is the wrong trade, especially for tests that turn out to be
redundant.

An audit confirmed the redundancy. The interpreter has **536 unit tests**
(`src/interpreter/tests*.rs`) that drive the same scripting surface directly via a
`run(source) -> Value` / `run_err(source) -> MarretaError` helper, asserting on the
returned value or the precise error **variant** (`UndefinedVariable`, `UndefinedTask`,
`NotCallable`, `WrongArity` with `expected`/`got`, `PropertyNotFound`,
division/modulo by zero, `TypeError`, and so on). That is strictly stronger than the
`run`-based tests, which only asserted substrings of the rendered error text. The
HTTP-level `e2e` suite and `functional_tests` cover the same constructs end to end
through routes.

Therefore the 72 `run`-based tests and their `run_source` / `run_file` helpers are
**removed outright**, with **no runtime change**. Their scenarios remain covered by
the interpreter unit tests (the proper layer) plus `e2e` and `functional_tests`. No
backfill was needed: each scenario category was verified present in the interpreter
unit tests before deletion.

## 3. Remove the `repl` command

`repl` is removed from the CLI and the codebase.

- Remove the `repl` subcommand arm, `run_repl`, the REPL special-command handling,
  `needs_continuation`, `print_repl_help`, and the `marreta repl` help line.
- Remove the single REPL integration test (the version-banner test).
- `execute_source` in `src/main.rs` loses its only callers (`run` and `repl`) and
  is removed. No replacement is needed: language-evaluation coverage already lives in
  the interpreter unit tests (see 2.2).

## 4. Hide `tokenize` and `parse`

`tokenize` and `parse` stay fully functional and callable, but are removed from the
public `--help` output, so the advertised surface is the API workflow (`serve`,
`test`, `doctor`, `init`, `fmt`, `lint`, `migrate`). They remain available for
engine debugging and bug reports.

- Drop their two lines from the help text. Keep the `tokenize` and `parse`
  subcommand arms and `debug_tokenize` / `debug_parse`.
- Add smoke tests for both (currently there are none): a small source file produces
  a non-empty token stream and a parsed AST, and a malformed file exits non-zero
  with a readable message. This guards their CLI wiring against regressions even
  though they are unadvertised.
- Do not introduce a `marreta debug ...` namespace. That would add public surface
  for an internal tool; the goal is to reduce surface, not reshape it.

## 5. Bare `marreta` prints help

Today bare `marreta` (no arguments) starts the REPL. With the REPL removed, bare
`marreta` prints the same help as `marreta --help`, which is the right default for a
CLI whose surface is the API workflow.

## 6. Out of scope

- Any interactive evaluator or script runner. Both are removed; exploration is done
  with `serve` and `test`.
- Any change to the runtime. This spec touches only the CLI surface (`src/main.rs`)
  and the test suite. The interpreter is not modified, and no output sink or
  execution harness is introduced.

## 7. Acceptance criteria

1. `marreta run` and `marreta repl` no longer exist: their subcommand arms,
   `run_file`, `run_repl`, `execute_source`, `needs_continuation`,
   `print_repl_help`, and REPL special-command handling are gone, and the help no
   longer lists them. The run-only CLI tests and the REPL banner test are removed.
2. Bare `marreta` (no arguments) prints help.
3. The runtime is unchanged: no output sink, no execution harness, no edit to
   `src/interpreter.rs` behavior. The 72 `run`-based integration tests and their
   `run_source` / `run_file` helpers are removed, with their scenarios confirmed
   still covered by the interpreter unit tests (`src/interpreter/tests*.rs`) plus
   `e2e` and `functional_tests`.
4. `tokenize` and `parse` are absent from `marreta --help` but still run, and each
   has a smoke test (valid input produces output, malformed input exits non-zero
   with a readable message).
5. Standard gates: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
   the full test suite, `functional_tests`, and `migrations_functional` all green.

## 8. Delivery notes

- `src/main.rs`: removed the `run` and `repl` subcommand arms, `run_file`,
  `run_repl`, `execute_source`, `needs_continuation`, `print_repl_help`, and the
  REPL special-command handling. Bare `marreta` (the `None` arm) now prints help.
  Help text no longer lists `run`, `repl`, `tokenize`, or `parse`. The `tokenize`
  and `parse` arms and `debug_tokenize` / `debug_parse` are kept.
- No runtime change. `src/interpreter.rs` is untouched in behavior (the briefly
  explored output sink was reverted), and no execution-harness module exists.
- `tests/integration_tests.rs`: removed the 72 `run`-based tests and the
  `run_source` / `run_file` helpers, plus the run-only CLI tests and the REPL banner
  test. Added smoke tests for `tokenize` and `parse` (still callable though
  unadvertised) and a test asserting bare `marreta` prints help and that the help
  omits the removed commands.
- Coverage audit: every scenario the removed tests exercised is covered by the
  interpreter unit tests (`src/interpreter/tests*.rs`, 536 tests, asserting on the
  returned value or the precise error variant) plus `e2e` and `functional_tests`.
- Gates: `cargo fmt --check` clean, `cargo clippy --all-targets -- -D warnings`
  clean, suite 1462 lib + 3 bin + 35 HTTP + 31 integration, `functional_tests`
  548/548, `migrations_functional` PASS.

---

## P.S. Do not forget the docs of record

On delivery, update both `CHANGELOG.md` and `docs/spec/SPEC.md`. See SPEC.md
section 1.3.
