# 065 - Developer Experience Polish

> Status: Delivered
> Type: Developer experience (CLI formatter, parser diagnostics, serve startup, init scaffold)
> Scope: Four independent developer-experience improvements bundled into one spec: (1) grow
> `marreta fmt` from a text normalizer into a token-aware formatter that also normalizes
> intra-line spacing per the house style; (2) give a clear indentation-specific parser error for
> the unexpected-indent case (today it falls through to a generic "expected expression"); (3)
> announce provider connection progress at `marreta serve` startup so a slow or failing provider
> is not a silent wait; (4) make the `marreta init` MongoDB healthcheck authenticate so
> `docker compose up --wait` does not report healthy during MongoDB's first-run init window. No
> language surface changes; these are tooling/diagnostics/scaffold polish. All four surfaced
> while dogfooding initializer-generated projects.

---

## 1. Purpose

The runtime and CLI are correct here, but the developer's first-contact experience has rough
edges that this spec smooths:

- `marreta fmt` (`src/formatter.rs`, `normalize_source`) is a **text** normalizer: it fixes
  indentation width (to 4 spaces per block depth), trailing whitespace, and blank lines between
  top-level declarations, then re-parses. It does not touch intra-line spacing, so `a=b`,
  `{x:y}`, and `200,{` pass through unchanged. That is below what a language `fmt` is expected
  to do and leaves the `conventions.md` house style unenforced.
- The parser already reports the **dedent** mistake well (`invalid indentation (expected N
  spaces, got M)`), but an **over-indent / unexpected indent** falls through to a generic
  `expected expression, got ''`, which does not point at the real cause.
- At `marreta serve` startup the first lines a user sees are the per-provider connection
  successes (`src/main.rs`: `DB connected (...)`, `DocDB connected (...)`, `Queue connected
  (...)`, `Cache connected (...)`). If a provider connection hangs or fails, nothing is printed
  until the error or timeout, so the user cannot tell what is happening.
- The MongoDB healthcheck in the `marreta init` docker-compose template (`src/init.rs`,
  `docker_compose`) is an unauthenticated `mongosh ... ping`. It passes against MongoDB's
  temporary first-run init server, so `docker compose up --wait` reports the container healthy
  before the real authenticated server is listening; `marreta serve` then gets a connection
  refused (a retry works), which is confusing.

## 2. fmt: token-aware spacing normalization

Grow `marreta fmt` so that, on top of today's indentation and blank-line normalization, it
reemits each line's content with canonical intra-line spacing per `conventions.md`:

- one space around binary operators and `=`;
- one space after `:` in maps and after `,`, none before;
- one space inside `{ }` map/record braces, none inside `( )` call parentheses;
- no other reflow (no line wrapping, no alignment, no reordering) in this spec.

The formatter must be **comment-aware**. The current lexer discards comments entirely
(`src/lexer.rs`), so a pure token reemit would drop them. The formatter therefore takes a
comment-preserving path — either by emitting comment tokens, or by a hybrid line scanner that
carries comment text through — so that full-line comments, leading comment groups, and trailing
comments survive verbatim, along with string and interpolation contents.

The safety contract is explicit, not just "re-parses". The parsed `Program` carries source
positions (`line`/`column`), so a `Program` `PartialEq` would always differ after reindenting
and cannot be the invariant. Instead the guard compares the **token stream** before and after,
retaining the semantic layout tokens (`Indent`, `Dedent`, `Newline` — block structure and
statement separation) and dropping only the `Eof` sentinel; the lexer already collapses
consecutive newlines, so blank-line normalization does not perturb it. If the streams differ,
`format_source` returns an error rather than writing the output. Combined with **idempotency**
(`fmt(fmt(x)) == fmt(x)`), this guarantees formatting never changes the program's meaning. The
significant indentation computed today is preserved; no line wrapping, alignment, or reordering.

## 3. Parser: clear error for unexpected indentation

When a line is indented deeper than any open block expects (the over-indent case), the parser
should emit an indentation-specific diagnostic in the same family as the existing dedent error
(`invalid indentation (...)`), naming the offending line, instead of falling through to
`expected expression, got ''`. The dedent message stays as is; this only fixes the over-indent
path so both indentation mistakes read clearly.

## 4. serve: provider connection progress

At startup, before attempting provider connections, print an up-front line that the app is
connecting to its configured providers, and a per-provider "connecting to <provider>
(<backend>)" line immediately before each connection attempt, keeping the existing success
lines. A provider that hangs or fails is then visible and attributable in real time rather than
a silent wait until the error or timeout. Output channel and format follow the existing startup
logs (`src/main.rs` connection points, `src/server.rs` startup banner). Only **configured**
providers are announced: a project that configures no providers prints no provider-progress
lines, so the output is never noisy or misleading.

## 5. init: authenticated MongoDB healthcheck

In the generated `docker-compose.yml` (`src/init.rs`, `docker_compose`), the mongodb
healthcheck authenticates with the same root credentials the template already sets
(`-u <user> -p <pass> --authenticationDatabase admin`), so it only passes once the real
authenticated server is accepting connections, closing the `--wait` race. No other service
changes.

## 6. Implementation outline

- **fmt (item 2):** `src/formatter.rs`. The token-level spacing pass operates on the lexed
  tokens per line; `normalize_source`'s indentation/blank-line logic is kept. Idempotency and
  spacing-rule unit tests, plus existing fmt tests (e.g. `normalizes_indentation_to_four_spaces`)
  stay green.
- **parser (item 3):** the indentation-handling path in `src/lexer.rs`/`src/parser.rs` that
  today produces the dedent error; add the over-indent branch with an `invalid indentation`
  message. Unit test for the over-indent case.
- **serve (item 4):** `src/main.rs` provider-connection sequence; emit the progress lines. A
  test or snapshot of the startup log ordering if practical.
- **init (item 5):** the mongodb block string in `docker_compose`; update the existing init
  unit tests/fixtures that assert the compose content.
- The four items are kept **mechanically independent** in implementation and tests (separate
  code paths and separate test cases), so bundling them in one spec does not entangle them.
- **Cross-repo note (item 5):** `marreta init`'s docker-compose output is mirrored by the site
  initializer (marreta-lang-site Spec 008). Changing the mongo healthcheck changes the generated
  file, so the site's generator and its `tests/fixtures/init` must be re-synced
  (`scripts/sync-init-fixtures.sh`) after this lands. This is a **required follow-up** before the
  release is considered fully consistent, even though it lands in the site repo separately.

### Coverage analysis

- **VS Code extension:** no change. None of these touch the language surface (no namespace,
  keyword, builtin, grammar token, snippet, or completion/hover/definition behavior). The
  extension stays a thin CLI client; `fmt` still runs through the CLI.
- **e2e:** no change. These are formatter, diagnostic, startup-log, and scaffold changes, not
  in-memory language execution or resolution semantics, so the e2e guardian gains no scenarios.
  Coverage is via unit tests (fmt idempotency/spacing, parser over-indent), init tests
  (healthcheck), and `functional_tests` for serve.
- **Documentation (`docs/guide`):** update the CLI reference's `fmt` description to state it
  normalizes intra-line spacing per the house style (today it only normalizes indentation and
  blank lines); review the container tutorial for the generated mongo healthcheck and
  `how-to/observe-logs.md` for the startup output. Any example shown must be verified against a
  tested project under `docs/examples`.

## 7. Out of scope

- Making `requires_marreta` / `COMPAT_FLOOR` dynamic, and any other init template change beyond
  the mongo healthcheck.
- fmt line wrapping, alignment, or expression reordering (only spacing in this spec).
- A broader parser error-message overhaul; only the unexpected-indent case is addressed.
- Changing provider connection behavior or ordering (only adding the progress logs).

## 8. Acceptance criteria

1. `marreta fmt` rewrites `message=greetings.build_greeting( "Marreta" )`, `{message:message}`,
   and `reply 200,{ msgs: msgs }` to the canonical house-style spacing; existing
   indentation/blank-line behavior is unchanged.
2. The formatter is comment-aware: full-line comments, leading comment groups, and trailing
   comments survive verbatim, each covered by a test.
3. For any input, the token stream (including `Indent`/`Dedent`/`Newline`, dropping only `Eof`)
   is identical before and after formatting, and formatting is idempotent
   (`fmt(fmt(x)) == fmt(x)`). A change that re-parses but alters that stream fails this. The
   `docs/examples` corpus is formatted and checked under this invariant.
4. An over-indented line reports an `invalid indentation` diagnostic naming the line, not
   `expected expression, got ''`; the dedent message is unchanged.
5. `marreta serve` prints an up-front "connecting to providers" line and a per-provider line
   before each connection attempt; a deliberately unreachable provider shows its line before the
   failure instead of a silent wait. A project with no providers configured prints no
   provider-progress lines.
6. A project generated with `--with doc` (or all providers) reaches `marreta serve` connecting
   to MongoDB on the first try after `docker compose up -d --wait`, because the healthcheck only
   reports healthy once authenticated.
7. The site initializer's drift is recorded as a required follow-up (resync its
   `tests/fixtures/init` once item 5 lands).
8. Standard gates: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, the full
   test suite, and — for the runtime/init changes — `functional_tests` and
   `migrations_functional`.

---

## P.S. Do not forget the docs of record

On delivery, update both `CHANGELOG.md` and `docs/spec/SPEC.md`. See SPEC.md section 1.3.

---

## Delivery notes

Delivered on `feature/developer_experience_polish-065`, squash-merged to `main`. Two gates
passed: spec review (Approved with the comment-aware/AST-equivalence/configured-only adjustments
folded in) and code review of the diff (two rounds of fixes: layout-aware token guard and
keyword-operator spacing). The four items, kept mechanically independent:

- **fmt token-aware spacing** (`src/formatter.rs`): a comment/string-safe line scanner respaces
  code (one space around binary ops and `=`, after `,`/`:`, inside `{ }`, tight `.`/`?`/calls,
  unary `-`/`+` tight, keyword operators `and/or/not/in` spaced). Safety is a **layout-aware
  token-stream guard** (full stream incl. Indent/Dedent/Newline, minus Eof) plus idempotency;
  a corpus test formats every `docs/examples/**/*.marreta` under that invariant.
- **Parser over-indent error** (`src/error.rs`, `src/parser.rs`): new `UnexpectedIndentation`
  variant renders as an `invalid indentation` diagnostic naming the line, instead of the generic
  `expected expression, got ''`. The dedent message is unchanged.
- **serve startup progress** (`src/main.rs`): an up-front "Connecting to providers" and a
  per-provider "connecting" line before each attempt, configured providers only.
- **MongoDB healthcheck** (`src/init.rs`): the generated docker-compose mongo healthcheck
  authenticates, closing the `docker compose up --wait` race.
- **VS Code extension:** no change (verified — the extension shells out to `marreta fmt`/`lint`,
  so it picks up the new behavior with the rebuilt binary).
- **Cross-repo follow-up (required):** `marreta init`'s docker-compose changed, so the site
  initializer (marreta-lang-site Spec 008) generator + `tests/fixtures/init` must be re-synced.

Gates: all green — `cargo fmt --check`, `clippy -D warnings`, full unit suite, plus the runtime
tier (release rebuilt, `marreta-lang:dev` image rebuilt, `functional_tests`,
`migrations_functional`, and `e2e`).
