# 059 - VS Code Extension: Fixes and Enrichment

> Status: Delivered
> Type: Editor tooling / developer experience
> Scope: Fix the known false positive and gaps in the VS Code extension, and enrich
> it (cross-file go-to-definition, a purple mallet icon, palette commands and tasks,
> CodeLens, more snippets, standalone-file support, visible tooling errors, and
> marketplace polish). The extension stays a thin client of the `marreta` CLI: all
> language analysis lives in the CLI, surfaced through `marreta tooling …` endpoints
> and `marreta lint`/`fmt`. Some items therefore add or extend CLI tooling endpoints.

---

## 1. Purpose

The VS Code extension (`docs/editors/vscode`) works but is basic and has a few real
bugs. An audit surfaced a confirmed false positive, several gaps where the extension
is more restrictive than the CLI, and a clear set of enrichment opportunities. This
spec collects all of them.

## 2. Architecture principle (non-negotiable)

The extension is a **thin client of the `marreta` CLI** and stays that way. It
performs **no language analysis of its own**: no parser, no symbol resolution, no
diagnostics logic in JavaScript. Everything that requires understanding Marreta
source is produced by the CLI (`marreta tooling …`, `marreta lint`, `marreta fmt`)
and the extension only spawns the CLI, routes requests, and renders results. The
only non-CLI pieces are declarative editor assets that contain no analysis: the
TextMate grammar, snippets, `language-configuration.json`, the manifest, and the
icon. Any feature that needs source understanding is implemented as (or backed by) a
CLI endpoint, never reimplemented in the extension.

## 3. CLI-side work (Rust)

### 3.1 Fix unused-variable false positive on string interpolation

A variable used only inside a string interpolation (`reply 200, "Hello #{name}"`) is
flagged `unused_variable`. Root cause: interpolation is preserved as a raw string
literal in the AST and resolved at runtime, so the lint never sees `name` as an
identifier. Fix in `src/lint.rs`: when collecting identifier reads, scan string
literals for `#{…}` placeholders and collect the identifiers referenced inside them.
Conservative (over-collecting is safe: it can only mark a variable as used). Add a
regression test.

### 3.2 Lint diagnostics carry a span

Lint diagnostics expose only `line`/`column`, so the extension underlines a single
character. Add an end position (end line/column, or length) to the lint diagnostic
JSON so a diagnostic can underline the whole offending token/span. The extension
uses it to size the squiggle. The CLI remains the source of truth for ranges.

### 3.3 `tooling definition` endpoint (go-to-definition)

Add `marreta tooling definition --stdin --file <path> --line N --column N
--format json`, returning the declaration location (`file`, `line`, `column`) of the
symbol referenced at that position, or null.

**Resolution contract (frozen).** The endpoint resolves the **token at the cursor**
using the **lexer position plus the parsed AST/project context** — not an AST-only
span (the AST does not carry a precise column span on every `Expression::Identifier`
or task call) and not a name-only guess. Concretely: it locates the token at
`line`/`column` in the source, determines from the surrounding parsed construct
whether that token is one of the v1 reference sites below, and looks the name up in
the project symbol table to return the declaration location. When the token is not a
known reference site, it returns null. Resolution is project-wide, so it spans files.

**v1 reference sites.** The endpoint resolves, at minimum:

- **task** — a task call `name(args)`, and a bare task name used as a pipeline stage
  (`>> name`) or broadcast target.
- **schema** — a schema name in a route/`on queue`/`on topic` `take … as Name`, a
  route-level `as Name`, a `reply [CODE] as Name`, an `http_client … as Name`
  (response schema), a task parameter `as Name`, a schema constructor `Name { … }`,
  and a nested schema field whose type references another schema.
- **auth** — the provider name in `require auth <provider>`.

This is the CLI-backed basis for the editor's Definition provider, consistent with
`completions`/`hover`. Any reference site not listed is out of v1 scope and must
return null rather than a partial/incorrect guess (no silent partial coverage).

### 3.4 Emit `auth` providers in `tooling symbols`

`tooling symbols` currently emits `route`, `scenario`, `schema`, and `task`, but not
`auth` providers, so auth declarations are invisible to symbol/definition tooling.
Emit auth providers as symbols (`kind: "auth"`, name = provider name, with
`file`/`line`/`column`) so they appear in the outline and resolve in 3.3.

### 3.5 Tooling endpoints work without a project root

`marreta lint --stdin` already falls back to a project-less mode when there is no
`app.marreta`. Ensure `tooling completions`, `tooling hover`, `tooling symbols`, and
`tooling definition` behave the same over `--stdin`: when no project root is found,
operate on the single buffer rather than erroring. This is what lets the extension
serve standalone `.marreta` files (4.2).

## 4. Extension-side work (thin CLI client)

### 4.1 Cross-file go-to-definition

Register a `DefinitionProvider` that calls `tooling definition` with the cursor
position and returns the resolved `Location`. Ctrl+Click / F12 on a `task`, `schema`,
or `auth` name jumps to its declaration, including across files in the project.

### 4.2 Standalone-file support

Today every provider (completion, hover, diagnostics, format, symbols) returns early
when there is no `app.marreta`, so a `.marreta` file outside a project gets no
intelligence even though the CLI supports it. Use the CLI's project-less `--stdin`
behavior (3.5) so a standalone buffer still gets diagnostics, completion, hover,
formatting, symbols, and definition.

### 4.3 Visible tooling errors

`showToolingError` only does `console.warn`, so when the `marreta` binary is missing
or misconfigured the user sees nothing and assumes the extension is broken. Surface
an actionable, **one-time** notification (for example "Marreta CLI not found — set
`marreta.path`") with a button to open settings, while still logging details. Do not
spam on every keystroke.

### 4.4 Diagnostic span

Use the span from 3.2 to underline the full token instead of a single character.

### 4.5 Palette commands and tasks

Contribute commands for the common CLI loop — `Marreta: Serve`, `Marreta: Test`,
`Marreta: Doctor`, `Marreta: Init`, `Marreta: Format File` — that run the CLI in an
integrated terminal/task. Add a `problemMatcher` so `marreta test` / `marreta lint`
output surfaces in the Problems panel. These are thin CLI invocations.

### 4.6 CodeLens

Add CodeLens anchored on CLI-provided symbols: "Run scenario" above each `scenario`,
"Serve" / "Open docs" near routes. Lens positions come from `tooling symbols`; the
actions run CLI commands. No editor-side parsing.

### 4.7 More snippets

The snippet set is sparse (route, task, schema, require, on queue, on topic,
scenario). Add reply, fail, match, if/else, auth, take, pipeline, an `http_client`
call, and a `db` query. Static snippets only.

### 4.8 Indentation fixes

`language-configuration.json` increases indent after `require … else` /
`reject … else` and after a conditional suffix (`x = v if cond`), which do not open
blocks. Tighten `increaseIndentPattern` so single-line statements do not trigger an
indent. Declarative config only.

### 4.9 Quick-fix code actions

Offer code actions driven by **CLI diagnostics** (no editor-side analysis): for an
`unused_variable` diagnostic, a "Remove unused variable" action that deletes the
flagged assignment using the diagnostic span from 3.2. Richer fixes that need source
understanding — for example "Create missing schema" from an unknown-schema
diagnostic — are applied only when the CLI supplies the fix (a future
fix-suggestion field on the diagnostic JSON); the extension never synthesizes such
edits itself. The concrete deliverable here is "Remove unused variable"; the
CLI-backed fix-suggestion contract is noted for follow-up.

### 4.10 Status bar item

A status bar item shows the resolved `marreta` version (from `marreta --version`)
and tooling health: healthy when the CLI is found and responding, a warning state
when it is missing/misconfigured that links to the 4.3 notification / settings. Pure
CLI invocation, no analysis.

## 5. The icon (purple mallet)

`.marreta` files should show the purple mallet (mascot color). Two mechanisms, and
this spec does both:

- **Editor tab / Open Editors icon (guaranteed):** contribute
  `languages[].icon = { light, dark }` pointing at a mallet SVG. This shows on tabs
  and wherever the language icon is used, regardless of the user's icon theme.
- **File explorer icon (opt-in):** ship a small **file icon theme** ("Marreta File
  Icons") that maps `.marreta` to the mallet. A contributed icon theme cannot
  override the user's active icon theme (that is by design in VS Code: explorer
  icons belong to the active File Icon Theme, which is why Java/Rust icons come from
  Material/Seti, not the language extension). Users who want the mallet in the
  explorer activate this theme.

A 16px-friendly mallet **SVG** in the brand purple is added. The mascot in
`docs/assets/brand/images/` is only the visual reference for deriving it; the raster
mascot is too detailed at 16px. **All icon assets live inside the extension
directory** (for example `docs/editors/vscode/icons/`), never under `docs/assets`,
so the extension stays self-contained and the `.vsix` packages them directly (the
`.vscodeignore` keeps the rest out). The file icon theme JSON also lives in the
extension directory.

## 6. Marketplace polish

- Add an extension `icon` (mascot) to the manifest, with the asset stored inside the
  extension directory (`docs/editors/vscode/`), not under `docs/assets`.
- Fix `homepage` (currently points to `docs/vscode-marreta`; the path is
  `docs/editors/vscode`).
- Add `keywords`, `license`, and refine `categories`.
- Update the README to describe the new capabilities and the icon theme.

## 7. Out of scope

- A long-running language server daemon. The per-request CLI spawn model stays
  (debounced), consistent with the current design.
- An editor-side parser, refactoring engine, or debugger.
- Find All References and rename. (The `tooling definition` work in 3.3 lays the
  groundwork; references/rename can be a later spec.)
- CLI-supplied fix suggestions beyond "Remove unused variable" (for example
  "Create missing schema"), which need a diagnostic fix-suggestion contract in the
  CLI; that contract is a follow-up. The editor never synthesizes such fixes itself.

## 8. Phasing

1. **CLI contract**: 3.1 (interpolation fix), 3.2 (span), 3.4 (auth symbols), 3.5
   (project-less stdin), 3.3 (`tooling definition`), each with unit tests.
2. **Extension core**: 4.1 (definition), 4.2 (standalone), 4.3 (visible errors), 4.4
   (span), 4.8 (indentation).
3. **Extension enrichment**: 5 (icon), 4.5 (commands/tasks), 4.6 (CodeLens), 4.7
   (snippets), 4.9 (quick-fix), 4.10 (status bar), 6 (marketplace polish).

## 9. Acceptance criteria

1. A variable used only inside a `#{…}` interpolation is **not** flagged
   `unused_variable` (regression test in `src/lint.rs`).
2. Lint diagnostics carry a span; the extension underlines the full token.
3. `marreta tooling definition …` resolves task, schema, and auth references to their
   declaration location (project-wide), and `tooling symbols` includes `auth`.
4. `tooling` endpoints and lint work over `--stdin` with no project root; the
   extension provides diagnostics/completion/hover/format/symbols/definition for a
   standalone `.marreta` file.
5. Ctrl+Click / F12 on a task/schema/auth name jumps to its declaration across files.
6. A missing/misconfigured `marreta` binary produces a single actionable
   notification, not silence.
7. `.marreta` files show the purple mallet on the editor tab; an opt-in Marreta file
   icon theme shows it in the explorer.
8. Palette commands (Serve/Test/Lint/Format Document/Doctor/Init) exist; CodeLens
   runs scenarios from CLI-provided symbol positions, with a single Serve lens on the
   project bootstrap (`app.marreta`). A terminal-task `problemMatcher` is deferred
   (the diagnostics provider already feeds the Problems panel live) — see §10.
9. Snippet set (including `db` query) and indentation rules are expanded/fixed;
   manifest metadata is corrected for the marketplace.
10. A "Remove unused variable" quick-fix is offered from the `unused_variable`
    diagnostic, and a status bar item shows the `marreta` version and tooling health.
11. The extension performs no language analysis itself — every source-understanding
    feature is backed by a CLI endpoint.
12. Standard gates for the CLI work: `cargo fmt --check`,
    `cargo clippy --all-targets -- -D warnings`, the full test suite,
    `functional_tests`, and `migrations_functional` green. The extension smoke
    (`docs/editors/vscode/smoke.marreta`) still loads.
13. The extension has its own gate, since this spec adds substantial JS (definition
    provider, CodeLens, commands/tasks, status bar, notifications): `node --check`
    passes on `extension.js`, `client/*.js`, and `providers/*.js` (catches syntax
    errors and broken requires), and the extension packages cleanly
    (`npm ci` + a VSIX build via `@vscode/vsce package`) when the Node toolchain is
    available. This gate runs on every phase that touches extension JS.

## 10. Delivery notes

Delivered in three phases on `feature/vscode-extension-enrichment-059`.

- **Phase 1 — CLI contract (Rust):** interpolation unused-variable fix (`src/lint.rs`,
  scans `#{…}`); lint diagnostics carry a span (`end_line`/`end_column`); `tooling
  symbols` emits `auth` providers; new `tooling definition` endpoint
  (`src/tooling/definition.rs`, token-at-cursor + AST/project context, resolving
  task call / pipeline stage / `as` schema / constructor / `require auth`, null
  otherwise); project-less `--stdin` fallback for `tooling symbols`. Unit tests for
  each. Gates: fmt + clippy clean; suite 1472 + 3 + 35 + 37; functional 548/548;
  migrations PASS.
- **Phase 2 — extension core:** `DefinitionProvider`; `toolingContext` standalone-file
  support; one-time actionable notification when the CLI is missing; full-token
  diagnostic spans; indentation fix (dropped `require/reject … else`).
- **Phase 3 — enrichment:** purple mallet icon (`languages[].icon` + opt-in file icon
  theme, assets in `icons/`); gallery icon; richer snippets; marketplace metadata;
  palette commands (Serve/Test/Doctor/Init/Format); CodeLens (run scenario / serve);
  "Remove unused variable" quick-fix (CLI-diagnostic driven); status bar (version +
  health).
- **Extension gate:** `node --check` passes on all JS, JSON/SVG assets validate, and
  `@vscode/vsce package` produces a clean VSIX (24 files).

Deferred (noted, not blocking): a terminal-task `problemMatcher` for `marreta
test`/`lint` output — the diagnostics provider already feeds the Problems panel live,
so a separate task matcher is a minor follow-up. Richer CLI-supplied fix suggestions
(e.g. "create missing schema") remain gated on a future diagnostic fix-suggestion
contract, per §7.

---

## P.S. Do not forget the docs of record

On delivery, update both `CHANGELOG.md` and `docs/spec/SPEC.md`. See SPEC.md
section 1.3.
