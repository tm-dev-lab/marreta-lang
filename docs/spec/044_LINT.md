# 044 — Lint

> Status: Delivered
> Type: Developer tooling / static analysis
> Scope: Add `marreta lint` for source-quality diagnostics that do not require running the application

---

## 1. Purpose

Marreta is a new language. Developers will not have years of Stack Overflow
answers, AI training data, or institutional knowledge to lean on.

The linter should catch common mistakes at authoring time and explain them in
language terms, before the developer starts the server or reads documentation.

This spec introduces:

```bash
marreta lint
marreta lint routes/
marreta lint --strict
marreta lint --format json
marreta lint --stdin --file routes/greetings.marreta --format json
```

The linter complements `doctor`:

- `doctor` validates project structure, configuration, and optional connectivity.
- `lint` validates source quality and likely mistakes.

---

## 2. Design Principles

1. **Lint catches bugs, not taste.**
   Style belongs to `marreta fmt`. Lint should report likely mistakes,
   unreachable code, broken references, and suspicious constructs.

2. **Diagnostics must teach the language.**
   Messages should include what happened and how to fix it. This matters more
   for Marreta than for mature languages with abundant external material.

3. **No runtime dependency.**
   Lint must not require DB, cache, doc, queue, HTTP services, or a running app.

4. **Low false-positive tolerance.**
   A noisy linter will be ignored. The first cut should prefer fewer,
   high-confidence rules.

5. **Editor integration is a first-class consumer.**
   JSON output and stdin mode are required for VS Code diagnostics over unsaved
   buffers.

6. **Project conventions match the rest of the CLI.**
   Like `fmt`, `serve`, `doctor`, `test`, and `migrate`, `marreta lint` without
   explicit paths is anchored at the current directory containing `app.marreta`.

---

## 3. CLI Surface

### 3.1 Lint Project

```bash
marreta lint
```

Behavior:

- Requires `app.marreta` in the current directory.
- Parses project source files using the same parser as `serve`, `test`, and
  `doctor`, but must not bootstrap a runtime or execute startup statements.
- Emits human-readable diagnostics.
- Exits `0` when no error-level diagnostics exist.
- Exits non-zero when at least one error-level diagnostic exists.

Warnings do not fail by default.

If a project-level parse/load error prevents full analysis, `lint` reports that
error and exits non-zero. It does not attempt partial analysis on a broken
project in the first cut.

### 3.2 Lint Explicit Paths

```bash
marreta lint routes/
marreta lint routes/greetings.marreta
```

Behavior:

- Accepts explicit `.marreta` files or directories.
- Directories are searched recursively for `.marreta` files.
- Explicit paths may run outside a project root for scratch files.
- When a project root is available, source-aware rules use project context.
- When no project root is available, only single-file syntax and local rules
  run.

### 3.3 Strict Mode

```bash
marreta lint --strict
```

Behavior:

- Warnings fail the command.
- Info diagnostics do not fail the command.
- Intended for CI once teams want stricter gates.

### 3.4 JSON Mode

```bash
marreta lint --format json
```

Response shape:

```json
[
  {
    "file": "routes/greetings.marreta",
    "line": 8,
    "column": 5,
    "severity": "warning",
    "code": "unreachable_statement",
    "message": "statement is unreachable after reply",
    "help": "Remove the statement or move it before the reply."
  }
]
```

JSON diagnostics are stable tooling contracts for editors and CI.

### 3.5 Stdin Mode

```bash
marreta lint --stdin --file routes/greetings.marreta --format json
```

Behavior:

- Reads the current file contents from stdin.
- Uses the on-disk project for other files when a project root is available.
- Overlays the stdin content for the specified file.
- Produces diagnostics as if the buffer had been saved.

This is mandatory for editor diagnostics while typing.

Human output is optimized for terminal use. JSON output is optimized for tools
and should remain stable once delivered.

---

## 4. Diagnostic Model

### 4.1 Severity

Allowed severities:

- `error`: likely invalid or broken behavior.
- `warning`: likely bug or maintenance problem.
- `info`: educational note or low-risk suggestion.

### 4.2 Codes

Each rule has a stable code:

```text
source_load_error
duplicate_route
unknown_schema_reference
unreachable_statement
unused_variable
unused_private_task
invalid_feature_flag_name
suspicious_self_recursive_task
```

Codes are part of the public tooling contract.

### 4.3 Help Text

Diagnostics should include concise help when the fix is obvious:

```text
warning unreachable_statement at routes/greetings.marreta:12:5
statement is unreachable after reply
help: Remove the statement or move it before the reply.
```

---

## 5. Initial Rule Set

The first cut intentionally contains only high-confidence rules. It should
prefer missing a subtle issue over teaching developers to ignore noisy output.

### 5.1 Source Load Errors

Surface parser and non-executing structural source errors through the lint
diagnostic model.

Severity: `error`

Examples:

- invalid indentation
- duplicate routes
- route conflicts caused by equivalent path patterns
- invalid schema names
- circular schema references
- invalid persistent schema references

Rationale:

- These are already invalid for `serve`/`test`.
- `lint` gives editors and CI a static command to surface them earlier.
- The implementation must not call a project loader path that bootstraps runtime
  or evaluates top-level statements. Lint is source analysis, not execution.

### 5.2 Duplicate Routes

Detect duplicate `METHOD + path` declarations when project analysis can
continue far enough to inspect routes.

Severity: `error`

```marreta
route GET "/greetings"
    reply 200, { ok: true }

route GET "/greetings"
    reply 200, { ok: false }
```

If the existing project loader already rejects the duplicate route first, lint
may report the loader error instead of a separate rule-specific duplicate route
diagnostic. The important contract is that `marreta lint` catches it before
runtime.

### 5.3 Unknown Schema Reference

Detect schema references that cannot be resolved:

- `take payload as MissingSchema`
- `reply 200 as MissingSchema, value`
- `task process(payload as MissingSchema)`
- `queue.push "x" as MissingSchema, payload`
- `http_client.get(url) as MissingSchema`
- `MissingSchema { ... }`

Severity: `error`

This is a source-quality error even when the unresolved reference would only
fail on an exercised code path at runtime.

### 5.4 Unreachable Statement After Reply/Fail/Raise

Detect statements after terminal statements in the same block:

```marreta
route GET "/x"
    reply 200, { ok: true }
    log.info("never runs")
```

Severity: `warning`

Only same-block statements after the terminal statement are in scope. The first
cut does not try to prove reachability across `match`, `if`, loops, rescue
branches, or nested blocks.

### 5.5 Unused Private Task

Detect private tasks that are never called.

Severity: `warning`

Do not warn for exported tasks in the first cut. Exported tasks may be intended
for cross-file or future use.

This rule is project-aware. It should only run when lint has full project
context. It should treat calls from routes, tasks, consumers, scenarios, and
top-level startup code as usages.

### 5.6 Unused Variable

Detect assignments in a straight-line block whose target is never read by any
later expression in that same block.

Severity: `warning`

```marreta
route GET "/x"
    message = "unused"
    reply 200, { ok: true }
```

This rule must be conservative:

- Do not warn when the target appears in a later expression.
- Do not warn on task parameters.
- Do not attempt cross-file or global data-flow inference.
- Do not attempt to prove branch-specific usage in the first cut.

### 5.7 Invalid Feature Flag Literal

Detect invalid literal names:

```marreta
feature.enabled("Bad__Name")
```

Severity: `error`

This rule only applies when the argument is a literal string. Dynamic
expressions remain runtime-validated.

### 5.8 Suspicious Self-Recursive Task

Detect direct recursion with no obvious conditional guard:

```marreta
task loop() => loop()
```

Severity: `warning`

This is intentionally conservative. The runtime already has recursion limits;
lint should only catch obvious mistakes.

---

## 6. Deferred Rules

These rules are valuable, but not part of the first cut because they require
more evidence or deeper analysis to avoid false positives.

### 6.1 Unused Exported Task

Exported tasks are public project API. They may be called by future files, test
fixtures, or external entry points not visible to a single lint run.

### 6.2 Scenario Mock Hygiene

Unused `given` mocks and scenario-specific test hygiene remain under
`marreta test` for now. Lint should not duplicate the scenario runner's
semantic checks until there is evidence that editor diagnostics need them.

---

## 7. Non-Goals

- No type inference engine in the first cut.
- No database connectivity.
- No queue/cache/doc connectivity.
- No performance lint.
- No domain advice.
- No formatting rules; use `marreta fmt`.
- No automatic fixes in the first cut.
- No source rewriting.
- No lint rules that depend on live environment variables, provider state, or
  network availability.

---

## 8. Relationship To Doctor

`doctor` remains the operational validator:

- project metadata
- config values
- feature flag config
- migrations
- provider connectivity via `--connect`

`lint` remains the source-quality validator:

- duplicate declarations
- unused code
- unreachable code
- unresolved references
- suspicious static patterns

If a diagnostic can be detected purely from source, prefer `lint`.
If it requires environment/config/provider state, prefer `doctor`.

Examples:

- `feature.enabled("Bad__Name")` is lint because it is source-literal invalid.
- `MARRETA_FEATURE_BAD__NAME=true` is doctor/config because it comes from env.
- `db.products.find(1)` with no DB configured is doctor/config, not lint.
- Duplicate route declaration is lint because it is source structure.

---

## 9. Implementation Notes

The linter should reuse the project loader and AST. It should not execute
interpreter code.

Suggested internal model:

1. Discover files using the same project conventions as `fmt`.
2. Parse files and preserve source locations.
3. Reuse parser and non-executing structural checks for existing source errors.
4. Build symbol tables:
   - routes by method/path
   - schemas by visibility/module
   - tasks by visibility/module
5. Walk AST and collect diagnostics.
6. Render diagnostics as human text or JSON.

The first cut may report a single fatal project-load error when loading fails.
It does not need to recover and continue after invalid source.

---

## 10. Test Plan

Unit tests:

- Diagnostic JSON serialization.
- Human diagnostic rendering.
- Duplicate route detection.
- Unknown schema references across all schema surfaces.
- Unreachable statement detection.
- Unused variable detection.
- Unused private task detection.
- Invalid feature flag literal detection.
- Strict mode exit behavior.

Functional tests:

- `marreta lint` passes on a clean project.
- `marreta lint` reports duplicate routes.
- `marreta lint --strict` fails on warnings.
- `marreta lint --format json` emits stable JSON.
- `marreta lint --stdin --file ...` catches unsaved-buffer diagnostics.
- `marreta lint` does not require selected local services to be running.
- `marreta lint <file>` works outside a project root for syntax/local rules.
- `marreta lint` reports invalid source with the file path.

---

## 11. Open Questions

1. Should future auto-fixes be exposed as `marreta lint --fix`, or kept out to
   preserve explicitness?
2. Should `lint` eventually aggregate multiple parser errors, or is first fatal
   parse error enough for Marreta's indentation-sensitive grammar?
