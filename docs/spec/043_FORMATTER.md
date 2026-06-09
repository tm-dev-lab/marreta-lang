# 043 — Formatter

> Status: Delivered
> Type: Developer tooling / CLI
> Scope: Add `marreta fmt` as the canonical formatter for Marreta source files

---

## 1. Purpose

Marreta uses significant indentation. That makes formatting part of source
correctness, not only code style.

For a new language, the formatter is also an adoption tool. Developers should
not need to memorize style rules or stop to read documentation before writing a
working route. The CLI should make source layout predictable.

This spec introduces:

```bash
marreta fmt
marreta fmt app.marreta
marreta fmt routes/ schemas/ tasks/
marreta fmt --check
marreta fmt --stdin --file routes/greetings.marreta
```

The formatter defines the canonical textual shape of Marreta code.

---

## 2. Design Principles

1. **Formatter is syntax-preserving.**
   Formatting must not change runtime behavior.

2. **Formatter is idempotent.**
   Running `marreta fmt` twice produces the same output as running it once.

3. **Indentation is canonical.**
   Since indentation defines blocks, the formatter uses one project-wide rule:
   four spaces per indentation level.

4. **No clever rewriting in the first cut.**
   The formatter normalizes structure and whitespace. It does not refactor
   expressions, reorder declarations, inline variables, or simplify logic.

5. **Comments are preserved.**
   A formatter that deletes or moves comments unpredictably is destructive and
   not acceptable.

6. **Editor integration is a first-class consumer.**
   `--stdin --file` is required so VS Code and future editors can format unsaved
   buffers without touching disk.

7. **Default behavior follows project CLI conventions.**
   Like `serve`, `doctor`, `test`, and `migrate`, `marreta fmt` without explicit
   paths is a project command anchored at the directory containing
   `app.marreta`.

---

## 3. CLI Surface

### 3.1 Format Project

```bash
marreta fmt
```

Behavior:

- Requires `app.marreta` in the current directory.
- Formats all project source files:
  - `app.marreta`
  - `routes/**/*.marreta`
  - `schemas/**/*.marreta`
  - `tasks/**/*.marreta`
  - `tests/**/*.marreta`
- Writes changes in place.
- Prints a short summary.
- Does not recursively scan arbitrary directories outside the canonical project
  layout.

Example output:

```text
Formatted 8 files.
```

### 3.2 Format Explicit Paths

```bash
marreta fmt routes/
marreta fmt routes/greetings.marreta schemas/
```

Behavior:

- Formats only the supplied files/directories.
- Recurses into directories for `*.marreta`.
- Writes changes in place.
- Does not require the current directory to contain `app.marreta`.

Explicit paths are the escape hatch for examples, fixtures, temporary snippets,
or repository-wide maintenance:

```bash
marreta fmt examples/functional_tests/
marreta fmt /tmp/scratch.marreta
```

### 3.3 Check Mode

```bash
marreta fmt --check
marreta fmt --check routes/
```

Behavior:

- Does not write files.
- Exits `0` if all files are already formatted.
- Exits non-zero if any file would change.
- Prints the list of unformatted files.
- Does not emit unified diffs in the first cut.

Example output:

```text
FORMAT routes/greetings.marreta
FORMAT tasks/greetings.marreta

2 files need formatting. Run `marreta fmt`.
```

### 3.4 Stdin Mode

```bash
marreta fmt --stdin --file routes/greetings.marreta
```

Behavior:

- Reads source from stdin.
- Uses `--file` for diagnostics and project-relative context.
- Writes formatted source to stdout.
- Does not modify disk.

This mode is mandatory for editor format-on-save and format-on-type workflows.

---

## 4. Formatting Rules

### 4.1 Indentation

- Four spaces per indentation level.
- No tabs for indentation.
- Nested blocks increase indentation by one level.

```marreta
route GET "/greetings"
    message = "Hello from Marreta"
    reply 200, { message: message }
```

### 4.2 Top-Level Declarations

One blank line between top-level declarations:

```marreta
project_name = "hello-api"
project_version = "0.1.0"

route GET "/greetings"
    reply 200, { message: "Hello" }

task greet() => "Hello"
```

Project metadata assignments stay together without forced blank lines.

### 4.3 Inline Tasks

Inline task syntax stays inline when already authored inline:

```marreta
task double(n) => n * 2
```

The formatter does not convert inline tasks into block tasks or vice versa.

### 4.4 Block Tasks and Routes

Block bodies are formatted by indentation only:

```marreta
task greet(name)
    message = "Hello, " + name
    message
```

### 4.5 Maps and Lists

Single-line maps/lists may remain single-line:

```marreta
reply 200, { ok: true, message: "Hello" }
items = ["a", "b", "c"]
```

Multi-line maps/lists are preserved as multi-line and indented relative to the
containing expression:

```marreta
reply 200, {
    ok: true,
    message: "Hello"
}
```

The first cut does not force line wrapping by width.

### 4.6 Pipelines

Pipelines keep one stage per line when authored multi-line:

```marreta
items = db.items
    >> where(active: true)
    >> order_by("name")
    >> fetch
```

The formatter does not change sequential `>>` into broadcast `*>>`, or the
reverse.

### 4.7 Comments

Line comments remain attached to the nearest following statement when possible:

```marreta
# Public health route.
route GET "/health"
    reply 200, { ok: true }
```

Inline comments are preserved:

```marreta
count = 10 # temporary limit
```

---

## 5. Error Behavior

If a file cannot be parsed, formatting fails and the original file is left
unchanged.

Example:

```text
ERROR routes/greetings.marreta:3:1 invalid indentation: expected 4 spaces, got 2
```

Formatter errors are syntax/tooling errors. They do not run application code.

---

## 6. Non-Goals

- No semantic refactoring.
- No declaration sorting.
- No automatic rename.
- No import organization; Marreta has no imports.
- No configurable style in the first cut.
- No line-width wrapping in the first cut.

The lack of configuration is intentional. A formatter is valuable because every
project converges on the same style.

---

## 7. Implementation Notes

The first implementation should not be a pure AST-printer. For an
indentation-sensitive language, discarding the original token stream too early
risks losing comments, blank lines, and other source trivia.

The formatter should be token/CST/trivia-aware:

- retain a concrete stream of tokens;
- retain comments and meaningful blank lines;
- use parser/block information to compute indentation depth;
- normalize whitespace around known syntax;
- avoid semantic rewrites.

If the formatter cannot preserve comments safely for a file, it must fail
without modifying that file.

Acceptable implementation strategy for the first cut:

1. Tokenize source including comments/newlines.
2. Parse enough block structure to know indentation levels.
3. Reprint statements with canonical indentation.
4. Preserve comments in original relative positions.

The formatter must never execute the interpreter.

---

## 8. Test Plan

Unit tests:

- Indentation normalization.
- Idempotency.
- Comment preservation.
- Route/task/schema blocks.
- Inline task preservation.
- Multi-line map/list preservation.
- Pipeline indentation.
- Parse error does not produce output.

Functional tests:

- `marreta fmt --check` fails for unformatted fixture.
- `marreta fmt` rewrites fixture.
- Second `marreta fmt --check` passes.
- `marreta fmt --stdin --file ...` returns formatted stdout and does not write.
- `marreta fmt` without `app.marreta` in the current directory fails with a
  project-root error.
- `marreta fmt explicit/path/` works outside a project root.
- `marreta fmt --check` lists files that would change and does not print diffs.
- Comments survive formatting unchanged in representative positions.
- Formatted project still passes `marreta doctor` and `marreta test`.

---

## 9. Closed Decisions

1. **Default scope:** `marreta fmt` without paths requires `app.marreta` in the
   current directory and formats only canonical project files. This prevents
   accidental rewrites of monorepo fixtures or intentionally malformed test
   cases, keeps traversal predictable, and matches other project commands.

2. **Arbitrary scope:** `marreta fmt <path...>` formats explicit files or
   directories and does not require a project root. This keeps repository-wide
   maintenance possible without making the default dangerous.

3. **Check output:** `--check` lists files and returns a non-zero exit code.
   Unified diff output is out of scope for the first cut.

4. **Comments:** comment preservation is blocking. If comments cannot be
   preserved safely, the formatter must not modify the file.

5. **Trailing whitespace:** trailing whitespace may be removed, including on
   comment lines, as long as the comment text and relative position are
   preserved.
