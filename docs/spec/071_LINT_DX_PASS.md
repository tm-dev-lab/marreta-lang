# 071 - Lint DX Pass

> Status: Delivered
> Type: Tooling (lint) + editor + docs
> Scope: Grow `marreta lint` from its current minimalist set into a focused, high-signal launch
> surface. Add a small batch of rules each justified by a runtime footgun or a recorded security
> debt, on top of three pieces of infrastructure that keep the lint honest and discoverable: inline
> suppression, a single rule catalog, and a `reference/lint` docs page wired to the editor. Pre-launch,
> no compatibility burden. Few and precise, not a catalog of noise.

---

## 1. Purpose

`marreta lint` ships eight rules today (`duplicate_route`, `unknown_schema_reference`,
`invalid_feature_flag_name`, `unused_variable`, `unused_private_task`, `unused_exported_task`,
`unreachable_statement`, `suspicious_self_recursive_task`). It is one of the most visible DX surfaces
(it runs on `marreta lint` and live in the editor), and for launch it is too thin: it does not catch
the language's two quietest footguns, nor a real SQL-injection vector that is already recorded in the
pre-launch security review.

Two runtime facts (verified) decide the two highest-value rules:

- **A route with no `reply`/`fail` returns a silent 204.** `src/server.rs` maps a route body that
  completes without a response (`Ok(_)`) to `StatusCode::NO_CONTENT`. Forgetting `reply` on a branch
  is a successful empty response that nothing complains about.
- **A `match` with no matching arm and no `fallback` returns a silent `Null`.** Confirmed by the
  interpreter's own test ("No fallback - returns Null when no arm matches"). The `Null` propagates and
  surfaces as an error far from its origin.

And one recorded security debt:

- **A SQL identifier built from a runtime value is an injection vector.** `src/db/query_builder.rs`
  interpolates the `order_by` string straight into SQL (`ORDER BY {}`), unparameterized and unquoted.
  Filter *values* are parameterized (`$1`), but the *identifier* (the `order_by` clause, a `select`
  computed alias, a `like`/`in` field) is not. `order_by(query.sort)` is injection. This is item 6 of
  the pre-launch security CONCERNS.

This spec closes those gaps with a few precise rules, and adds the infrastructure that lets
high-signal-but-imperfect rules exist without becoming mass-suppressed noise.

## 2. The change

### 2.1 A rule catalog (the structural backbone)

Introduce a single **rule catalog**: a table of every lint code with its default severity and a short
description. It becomes the one source for the docs page (§2.4), the editor `codeDescription` links
(§2.5), and a future `--explain`. An invariant test asserts **every emitted code is in the catalog
and has an anchor on the docs page** (the same catalog-to-token pattern that made Spec 068
drift-proof), so the next rule cannot be born without a doc. The existing eight rules are folded into
the catalog as part of this spec.

### 2.2 Inline suppression (lands before the new rules)

A line-level suppression, `# marreta: allow <code>`, suppresses a diagnostic of that code on the
following (or same) line. This is the escape valve that lets conservative, high-signal rules exist
without forcing a project to either live with a false positive or disable a rule globally. It lands
**before** the new rules so each new rule ships with its valve.

### 2.3 New rules

Each is justified by a runtime fact (§1) or recorded debt. Severities chosen for launch:

- **`shadows-injected-binding`** (warning) - a local shadows an injected binding (`params`, `auth`,
  `payload`, ...). Scope-aware: flagged only where the binding is actually live. (The §1.4 sister
  follow-up this spec subsumes.)
- **`route-without-response`** (warning) - a route path can finish without `reply`/`fail`, so it
  silently 204s. Conservative path analysis **local to the route body**. What counts as terminating a
  path: a `reply`/`fail` in **statement position**, and a `reply`/`fail` in a **`match` arm body**
  (arm bodies are expressions, not statements, so a `fail` arm like `fallback -> fail 400, ...` must
  count, otherwise "match requires all arms plus `fallback`" would never terminate anything).
  `if/else` requires both branches, `match` requires all arms plus `fallback`, and `require`/`reject`
  do not terminate the happy path. Three settled boundaries, each stated in the rule's docs entry:
  - **A `fail` inside a called task does not terminate the path.** No interprocedural analysis at
    launch: a task call never terminates a route path. Local-to-route-body analysis is what keeps the
    rule predictable; the rare deliberate case ("task that always fails at the end of a route") is
    covered by suppression.
  - **`rescue` is value recovery, not a route exit path** (verified against the AST: `rescue` exists
    only as an expression modifier and a pipeline-stage `Inline`/`Block`, never a route-level handler,
    and the server has no separate handler-completion path). Handler bodies are not paths, and a
    `reply`/`fail` inside a rescue body does **not** terminate the happy path. Counting it would
    "save" exactly the routes that carry the bug this rule exists to catch (a route whose only `fail`
    lives in a `>> rescue` block still 204s on the happy path).
  - **A `fail` in any other deep expression position** (for example `x or fail ...`, if it parses) is
    outside the analysis: a conservative false positive, covered by the suppression valve.
  Warning severity for the same reason: the conservative analysis can false-positive at the edges,
  which is what suppression is for.
- **`match-without-fallback`** (warning) - a `match` with no `fallback` whose value is **consumed**
  (assigned, or in expression position). A match-statement with effect arms and a discarded result
  has no footgun and is not flagged.
- **`non-literal-sql-identifier`** (warning) - a `db` `order_by` / `select` computed alias /
  `like` / `in` whose identifier argument is built from a non-literal (a variable or expression)
  rather than a literal string. Reuses the literal-string check from the index-inference walker. This
  is a **warning that surfaces the risk, not a fix**: the lint tells the developer a SQL identifier
  comes from a runtime value, it does not sanitize it. The identifier hardening itself is a named
  security follow-up (§4), and the lint stays valuable after it lands: a dev-time warning plus a
  runtime guard is defense in layers, not redundancy.
- **`unused-schema`** (warning) and **`unused-auth-provider`** (warning) - a declared schema or auth
  provider that is never referenced. Closes the "declared and never used" family the lint already
  half-covers (tasks, variables).

**Cheap-if-free** (include only if the existing machinery gives them, else defer): extend
`unused_variable` to a route's `take` bindings if it does not already cover them, and
`unused-task-parameter`.

### 2.4 The lint reference docs page

A `docs/guide/reference/lint` page documents each code: what it flags, why (the runtime fact or risk),
and how to fix it, with a stable anchor per code. It is generated from / verified against the catalog
(§2.1), and ships in this spec (docs DoD: new rule, new doc).

### 2.5 Editor: codeDescription links and a suppress quick-fix

The extension stays a thin client. Two additions:

- **`codeDescription` links.** Each diagnostic carries the LSP `codeDescription` with an `href` to
  its docs anchor (for example `marreta.dev/docs/reference/lint#route-without-response`), so the
  "what is this and how do I fix it" is one hover away, where the developer already is. This replaces
  the need for an `--explain` CLI flag.
- **A suppress quick-fix.** Discoverability of the valve is part of the valve: the new conservative
  rules will false-positive at the edges, and the difference between a "noisy lint" and a "mature
  lint" on first impression is the suppression being one click away. The quick-fix inserts
  `# marreta: allow <code>` on the line above the diagnostic, computed purely from the diagnostic
  itself (a text edit, thin client, cheap like the Spec 059 unused-variable quick-fix). **Ordering
  rule:** the suppress quick-fix is listed **after** any real fix in the code-actions list. It is
  available, never promoted: suppressing must not be the default action.

## 3. Implementation outline

- `src/lint.rs`: the rule catalog (code, default severity, short description) as the single source;
  fold the existing eight rules in; the new rules; the inline-suppression pass applied to all codes.
- The catalog-to-docs invariant test (every emitted code is catalogued and anchored).
- `non-literal-sql-identifier`: reuse the literal-string detection from `src/doc/index_inference.rs`
  (the same `literal_str` shape) over the `db` pipeline surface.
- `docs/guide/reference/lint.md` + `SUMMARY.md` entry, anchored per code, following `docs/STYLE.md`.
- VS Code extension: attach `codeDescription.href` per diagnostic code, and a suppress quick-fix
  that inserts `# marreta: allow <code>` (ordered after any real fix). Thin-client change only.
- Coverage analysis (per the spec protocol):
  - **VS Code extension:** yes - the `codeDescription` href wiring and the suppress quick-fix (§2.5).
  - **e2e:** the lint runs at load/CLI, not over the served HTTP path, so the lint rules live in unit
    tests + the `marreta lint` CLI tests, not the e2e HTTP suite. (Confirm whether any rule warrants
    an e2e touch; default no.)
  - **Documentation:** the `reference/lint` page (§2.4).

## 4. Out of scope

- **The SQL-identifier hardening itself** is a named security follow-up ("db identifier hardening",
  tracked in SPEC.md §1.4), not this spec. It is a different change class: it alters the query
  builder's SQL generation, so it needs the runtime gate tier (functional, migrations) that this
  tooling/docs spec's AC6 explicitly excludes, and it has its own design space (quote the identifier,
  validate against known columns, or reject). The security backlog staged it this way on purpose:
  lint plus docs first, the stronger validation second. The lint is not made redundant by it, see
  §2.3 (defense in layers).
- `constant-condition` (high signal, very low frequency - post-launch).
- Per-project lint config (rule toggles/severity). YAGNI until a team asks, and it freezes an API
  surface; inline suppression is the escape valve at a tenth of the cost.
- `--fix` autofix (the editor already has the unused-variable quick-fix from Spec 059; the rest does
  not justify the text-edit machinery yet).
- `--explain` as a CLI flag (the editor `codeDescription` link is the better destination).

## 5. Acceptance criteria

1. The rule catalog exists as the single source of code + default severity + description; the existing
   eight rules are folded in; an invariant test asserts every emitted code is catalogued and has a
   docs anchor.
2. `# marreta: allow <code>` suppresses a diagnostic of that code at line level, for every rule.
3. The new rules emit correctly with the chosen severities, each with positive and negative unit
   tests: `shadows-injected-binding` (scope-aware), `route-without-response` (conservative path
   analysis), `match-without-fallback` (value-consumed only), `non-literal-sql-identifier` (literal
   vs runtime identifier), `unused-schema`, `unused-auth-provider`. `route-without-response` includes
   the two deciding-case tests: a route whose only `fail` lives inside a `>> rescue` block is flagged
   (the happy path 204s), and a route whose `match` has all arms plus `fallback` all terminating
   (including a `fail` arm) is clean.
4. The `reference/lint` page documents every code with a stable per-code anchor, verified against the
   catalog; examples follow `docs/STYLE.md`.
5. The extension attaches `codeDescription.href` per code (pointing at the docs anchor) and offers a
   suppress quick-fix that inserts `# marreta: allow <code>`, listed after any real fix.
6. Standard gates: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, the full test
   suite; for the extension, `node --check` + a VSIX package. No runtime-behavior change beyond the
   lint and the editor diagnostic metadata.

---

## P.S. Do not forget the docs of record

On delivery, update both `CHANGELOG.md` and `docs/spec/SPEC.md`. See SPEC.md section 1.3.

---

## Delivery notes

`marreta lint` grew from 8 rules to 15, with the infrastructure to keep it honest, on the order the
spec asked (infra, then rules, then docs/editor).

- **Infrastructure** (`src/lint.rs`): a rule **catalog** as the single source (code, default severity,
  summary), enforced both ways - a `LintDiagnostic::new` debug assertion rejects an un-catalogued
  code, and a catalog-to-docs test asserts every code has a `### <code>` anchor on the reference page
  (the Spec 068 drift-proofing pattern). **Inline suppression** `# marreta: allow <code>`, string-aware
  (a `#` inside a literal does not hide the directive), standalone silences the next line, trailing
  silences its own.
- **Rules** (all warning): `shadows_injected_binding` (scope-aware), `route_without_response`,
  `match_without_fallback` (value-consumed only), `non_literal_sql_identifier` (db identifier from a
  runtime value, hardened to catch interpolation), `unused_schema` (persistent excluded), and
  `unused_auth_provider`.
- **Verified runtime facts**: the silent 204, the silent `null` from a fallback-less match, and the
  raw `order_by` interpolation. The fourth, vetted **before** coding, inverted a design: `rescue` is
  not a route-level handler (only an expression modifier and a pipeline stage), so the
  `route_without_response` analysis treats a rescue body as a value, not a path - a `fail` reachable
  only on the recovery path does not save the happy path. The deciding case is exactly the corpus's
  `/errors/rescue_block`.
- **Docs + editor**: a `reference/lint` page (section per code, anchored); the extension links each
  diagnostic code to its docs anchor and offers a suppress quick-fix listed after any real fix and
  never preferred.
- **Corpus**: lint-clean of the new rules. Two genuinely-orphan schemas removed; the deliberate
  `/errors/rescue_block` fixture suppressed with an explanatory comment - the suppression valve
  dogfooded in a real file, which is the functional proof that it works.
- **Two process catches paid off**: the vetted rescue fact (above), and a corpus-validation false
  positive (`unused_schema` missed a `reply ... as Schema` nested in an `if`, the omni_hub
  `OrderDetails` case) fixed before any user saw it.
- **Follow-up opened**: `db identifier hardening` (the runtime guard for the SQL identifier vector the
  `non_literal_sql_identifier` lint warns about) is a named security follow-up in SPEC.md §1.4.
- **Gates**: core (`fmt`, `clippy -D warnings`, the suite with ~30 new lint tests), runtime
  (`functional_tests` 567/567, `migrations_functional`, e2e which lints the project clean), and the
  extension (`node --check` + VSIX), all green.
