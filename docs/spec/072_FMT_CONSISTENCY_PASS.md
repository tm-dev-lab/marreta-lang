# 072 - Fmt Consistency Pass

> Status: Delivered
> Type: Tooling (formatter) + docs
> Scope: One correctness fix (the formatter discovers fewer project files than the loader, so
> it silently skips files the runtime loads) and four cheap, invariant-safe output
> normalizations (blank-line collapse, final newline, file-edge blanks, comment spacing), plus
> a documented stance section for the deliberate non-goals (no wrapping, no alignment, no
> sorting). Pulled from the stealth parking lot; every item was verified against the code or
> probed against the real binary, and re-verified on pull-in (2026-06-12): the formatter bug,
> the loader's recursive discovery, and all three probes reproduce as described. Note found on
> re-verification: `lint`'s discovery (`collect_project_files`) already recurses, so it does not
> carry the bug and §2.1's lint part is verify-only.

---

## 1. Purpose

`marreta fmt` has an unusually strong safety foundation: `format_source`
(`src/formatter.rs:56`) parse-gates the input and the output, snapshots the full token stream
including the semantic layout tokens (`Indent`/`Dedent`/`Newline`, only `Eof` dropped), and
fails on any divergence; idempotency and a corpus test over `docs/examples` guard it further.
None of the changes here touch that foundation. They fix one real bug and close four gaps the
foundation already makes safe to close.

## 2. The change

### 2.1 Share the loader's file discovery (the correctness fix, item zero)

**The bug.** `file_loader::collect_marreta_files` (`src/file_loader.rs:777`) recursively
collects every `.marreta` file under the project root. `formatter::discover_project_files`
(`src/formatter.rs:89`) walks only four fixed directories: `routes`, `schemas`, `tasks`,
`tests`. A project with an `auth/customer_auth.marreta` (the exact organization the auth spec,
024, suggests as an example) or any custom folder is loaded by `serve`/`test`/`doctor` but
silently skipped by `marreta fmt`. Same disease the reserved-word spec (068) cured for
keywords: two hand-maintained lists of one concept, drifted.

**The fix.** One discovery function, shared. Extract or reuse the loader's recursive
collection as the single source (the formatter's needs are a superset match: all `.marreta`
under the root, entrypoint included for fmt). `discover_project_files` delegates to it.
**Lint is already correct** (`src/lint.rs` `collect_project_files` already recurses via
`collect_recursive`, verified on pull-in), so the lint part is verification only, no fix.

**Care points.**
- The loader excludes the entrypoint from its recursive walk (it parses it separately); fmt
  must format `app.marreta` too, so the shared function needs to serve both shapes (a
  parameter or the caller appends the entrypoint, as `discover_project_files` already does).
- The loader has no dotted/hidden or `target/`-style exclusions (verified: `collect_recursive`
  walks every subdirectory). fmt mirrors that exactly: no exclusions either.
- Test: a fixture project with `auth/x.marreta` and a custom `lib/y.marreta`; assert
  `discover_project_files` returns them. Stronger: an invariant test asserting fmt discovery
  equals loader discovery over the same fixture (by construction after the refactor, but the
  test pins it against future drift).

### 2.2 Collapse consecutive blank lines (max one)

**Probe (reproduce):**

```bash
printf 'a = 1\n\n\n\n\nb = 2' | marreta fmt --stdin --file t.marreta
```

Today the four interior blank lines survive. Collapse runs of 2+ blank lines to exactly 1.

**Why it is safe by construction:** the lexer already collapses *interior* consecutive
newlines, so collapsing blank-line runs cannot change the `significant_tokens` invariant (the
guard in `format_source` proves it on every run anyway). The file-terminal newline is a
separate case, handled in 2.3.

**Where:** `normalize_source` (`src/formatter.rs:145`) pushes one `FormattedLine::blank()` per
blank input line. Skip the push when the previous pushed line is already blank.

### 2.3 Enforce exactly one final newline

**Probe:** same command as 2.2, note the output ends without a trailing newline because the
input did. Today the final newline is preserved-as-was (`had_final_newline`,
`src/formatter.rs:146`). Change to always end the output with exactly one `\n`. This deletes
the `had_final_newline` variable and simplifies the function.

**Token-stream fact (verified on the binary, correcting the parked spec's claim).** Adding a
final newline where the input had none *does* change `significant_tokens`, which snapshots
`Newline`. The original "safe by construction" wording was wrong on this point. The verified
fact: the lexer collapses interior consecutive newlines, but the file-terminal newline differs
between an input with a final `\n` and one without, and at the end of an indented file it sits
behind the synthesized block-closing `Dedent`s: `... Newline Dedent* Eof` (with `\n`) versus
`... Dedent* Eof` (without). The terminal `Newline` separates nothing (no statement follows it,
and a `Dedent` is a synthesized block close, not a statement), so it is not meaning-bearing.
The fix walks the snapshot back over the terminal `Dedent` run and drops the single `Newline`
behind it, on both the before and after snapshots, so normalizing the final newline never trips
the divergence guard while every interior `Newline` and every `Dedent` stays protected. The
corpus cannot guard this (every corpus file already ends in a newline), so a unit test on an
indented body with no final newline is the guardian.

### 2.4 Strip file-edge blanks

Same family as 2.2: no blank lines at the start of the file, and no trailing run of blank
lines before the final newline. Implement in the same pass.

### 2.5 Comment spacing: `#comment` becomes `# comment`

**Probe:**

```bash
printf '#comment\nx = 1\n' | marreta fmt --stdin --file t.marreta
```

Today `#comment` is kept verbatim (comments only get reindented). Normalize a leading `#`
followed by a non-space to `# `.

**Where:** the comment branch of `FormattedLine::new` (`src/formatter.rs:196`), which
currently copies `stripped` verbatim for comments.

**Edge rule (decide here, not in code review):** insert the space only when the character
after the leading `#` is neither a space nor another `#`. That leaves `## heading-style` and
`#` alone, while normalizing `#comment` and `#x = note`. Divider comments like `#====` become
`# ====` under this rule; run the corpus test and eyeball the corpus diff to confirm that is
acceptable before freezing (the repo corpus mostly uses `# ===` style already, so fallout
should be near zero). Comment content beyond the first character is never touched (no
reflow). Bonus: this also canonicalizes the lint suppression comment (`# marreta: allow
<code>`) from the lint DX spec before it spreads through corpora.

### 2.6 Documented stance: the deliberate non-goals

Docs requirements, file-precise (verified against the guide on 2026-06-12):

- **`docs/guide/reference/conventions.md`** is the home for what fmt enforces (it is the
  house-style page and already says it keeps `marreta fmt` predictable). Two changes: the
  comments rule ("Use `#` for all comments", around line 150) gains the new spacing norm
  ("with one space after `#`", matching 2.5), and a new short **Formatting** subsection
  lists the fmt-enforced normalizations (indentation, intra-line spacing, blank-line
  collapse, single final newline, file-edge blanks, comment spacing) followed by the stance
  block below.
- **`docs/guide/reference/cli.md`** (the `marreta fmt` row, line ~48): the description
  states project-wide coverage explicitly, "formats every `.marreta` file the project
  loads", which is also the user-visible statement of the 2.1 discovery fix.
- No new page, so no `SUMMARY.md` change.

The stance block, stating these as decisions rather than omissions:

- **No line wrapping and no max line width.** Precedent: gofmt famously enforces no line
  length. In Marreta the argument is stronger: `Newline` and `Indent` are semantic tokens
  (statement separation, block structure), so wrapping changes the token stream and would
  fight the meaning-preservation invariant that is the formatter's safety anchor. Wrapping is
  only grammar-legal at specific continuation points (pipeline stages, map entries). The line
  reads: fmt does not wrap lines, line length is the author's.
- **No elastic alignment** (for example aligning schema-field colons): editing one field
  would re-align the whole block and create diff noise.
- **No sorting** of fields or routes: order is semantic (OpenAPI document order,
  intentional readability).
- **No comment reflow**: comment content belongs to the author.

## 3. Implementation outline

- `src/formatter.rs`: the shared-discovery refactor (2.1), the blank-line pass (2.2-2.4) in
  `normalize_source`, the comment-spacing rule (2.5) in `FormattedLine::new`.
- `src/lint.rs`: discovery already recurses, so this is a verification only (no change),
  recorded in the delivery notes.
- Docs: the stance block (2.6) plus updating the fmt docs for the new normalizations, same
  change (docs DoD).

### Test requirements (house standards)

- **Unit tests, one per rule, positive and negative** (the per-spec unit-test gate):
  blank-collapse (run collapses, single blank preserved), final newline (missing gains one,
  existing single is untouched), file-edge blanks, comment spacing (`#comment` gains the
  space; `##`, bare `#`, and `# already-spaced` untouched), and the discovery fixture
  (`auth/x.marreta` plus a custom `lib/y.marreta` returned by `discover_project_files`).
- **Invariant tests**: fmt-discovery-equals-loader-discovery over the fixture (pins the 2.1
  fix against future drift, the catalog-to-token pattern), idempotency over every new rule,
  and the existing token-stream corpus test re-run over the reformatted corpus.
- **Functional coverage of the new behavior, end-to-end** (the house rule: exercise the new
  behavior through the real surface, not only no-regression): CLI integration tests in
  `tests/integration_tests.rs` running the **real binary** against a fixture project that has
  an `auth/` directory and a custom directory, asserting (a) `marreta fmt` rewrites files in
  both (the bug case made green), (b) `marreta fmt --check` exits non-zero before and zero
  after, and (c) a file piped through `fmt --stdin --file` shows the new normalizations
  (probes from 2.2/2.3/2.5 as assertions). The dockerized `functional_tests` suite is not
  applicable (fmt touches no provider); for a CLI feature the integration tier is the
  end-to-end, state this in the delivery notes.
- The corpus reformat diff (blank collapses, final newlines across `docs/examples`) is part
  of the delivery commit and reviewed as such.

### Coverage analysis (spec protocol)

- **VS Code extension: no change required, verified.** The extension's formatting provider
  shells out per file via `fmt --stdin --file <path>`
  (`docs/editors/vscode/providers/format.js:11`) and never uses project discovery, so the 2.1
  fix and the new normalizations reach format-on-save automatically through the CLI output.
  No grammar, completion, or command surface changes. Re-verify at implementation time that
  no extension code path calls project-mode `fmt`; if one appeared meanwhile, it inherits the
  fix with no change.
- **e2e**: none (no language behavior changes; the in-memory guardian has nothing new).
- **Docs**: the stance block and the normalization list in the fmt section of the CLI
  reference (2.6).

## 4. Out of scope

- Line wrapping or any max-width enforcement (see 2.6, documented stance).
- Alignment, sorting, comment reflow (see 2.6).
- Range formatting / format-selection for the editor (LSP nicety, no demand yet).
- Any change to the token-stream invariant machinery itself.

## 5. Acceptance criteria

1. `marreta fmt` (project mode) formats every `.marreta` file the loader would load,
   including files in non-standard directories (`auth/`, custom folders); discovery is shared
   with the loader, pinned by an invariant test. Lint discovery verified already aligned (it
   recurses), no change.
2. Runs of 2+ blank lines collapse to exactly 1, files end with exactly one final newline,
   and no blank lines remain at file start or as a trailing run (probes from 2.2/2.3
   reproduce clean).
3. A leading `#` followed by a non-space, non-`#` character gains one space (`#comment` to
   `# comment`); `##`-style and bare `#` comments are untouched; comment content is never
   otherwise modified. The corpus diff under this rule was reviewed and accepted.
4. The token-stream invariant, the idempotency test, and the corpus test pass over all new
   rules; the corpus reformat diff is part of the delivery commit.
5. CLI integration tests run the real binary over the `auth/` + custom-dir fixture: project
   `fmt` rewrites files in both, `--check` flips non-zero to zero across a format, and the
   stdin probes from 2.2/2.3/2.5 hold as assertions (the functional end-to-end for a CLI
   feature, recorded as such in the delivery notes).
6. Docs updated per 2.6: `reference/conventions.md` carries the comment-spacing rule and the
   Formatting subsection (normalizations + stance block with the semantic-token rationale,
   no alignment, no sorting, no reflow); `reference/cli.md`'s fmt row states "formats every
   `.marreta` file the project loads". No `SUMMARY.md` change (no new page).
7. The extension is confirmed unaffected (per-file `--stdin` provider,
   `providers/format.js:11`); re-verified at implementation time, with no extension change
   shipped unless a project-mode call appeared meanwhile.
8. Standard gates: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, full
   test suite. No runtime tier needed (fmt does not touch serve), unless the shared-discovery
   refactor moves loader code, in which case run `functional_tests` as well.

---

## Delivery notes

Delivered. All gates green (`cargo fmt --check`, `clippy -D warnings`, full suite, and the
runtime tier: `functional_tests`, `migrations_functional`, e2e), corpus left `fmt`-clean.

What landed:

- **Shared discovery (2.1).** `formatter::discover_project_files` delegates to the loader's
  recursive `collect_marreta_files` (`src/file_loader.rs`, now `pub(crate)`), replacing the
  fixed four-directory list. `marreta fmt` now formats every `.marreta` the runtime loads,
  including non-canonical folders (`auth/`, custom dirs). Pinned by an invariant test
  (`fmt_discovery_equals_loader_discovery_plus_entrypoint`). Lint discovery was already
  recursive, so it was verify-only, no change.
- **Blank-line pass (2.2-2.4)** and **comment spacing (2.5)** in `src/formatter.rs`
  (`normalize_source`, `FormattedLine::new`), with unit tests per rule.
- **Foundation touch, recorded.** The parked spec's "safe by construction" claim for the final
  newline (2.3) was incomplete. `significant_tokens` snapshots `Newline`, and at the end of an
  indented file the file-terminal `Newline` sits behind synthesized `Dedent`s
  (`... Newline Dedent* Eof` with a final `\n` versus `... Dedent* Eof` without). The snapshot
  now walks back over the terminal `Dedent` run and drops the single `Newline` behind it on
  both sides, so the final-newline normalization never trips the divergence guard while every
  interior `Newline` and every `Dedent` stays protected. This was surfaced explicitly during
  review (not embedded), which let the code review catch the gap with a live probe; two unit
  tests guard it because the corpus cannot (every corpus file already ends in a newline).
- **Docs (2.6).** `reference/conventions.md` gained the comment-spacing rule and a Formatting
  section with the non-goals stance block (no wrapping, no alignment, no sorting, no reflow);
  `reference/cli.md`'s fmt row states it formats every file the project loads.
- **CLI integration tests** over the real binary (non-canonical dirs, `--check` flip, stdin
  normalizations); **corpus reformat** of `docs/examples` + `e2e` (the `functional_tests` files
  carried pre-existing manual alignment the 071 binary already flagged).

---

## P.S. Do not forget the docs of record

On delivery, update both `CHANGELOG.md` and `docs/spec/SPEC.md`. See SPEC.md section 1.3.
