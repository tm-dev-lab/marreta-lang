# Marreta Lang — Changelog

Track of all implementation progress. Use this to resume work after session loss.

---

## Current Status

**Phase:** Pre-release readiness — first public release preparation
**Status:** `marreta-lang` is a single engineering monorepo (Spec 052 re-consolidation, superseding the Spec 047 split), with the layout grouped by Spec 055: the runtime is `src/` and `tests/`, the `e2e/` feature suite sits alongside them, and all supporting material (`spec/`, `examples/`, `benchmarks/`, `performance/`, `editors/`, `assets/`) lives under `docs/`. Delivered since v0.16: the single-engine AST hot-path optimization (Spec 051, retiring the route execution template fast path), a digital-bank MongoDB benchmark, pre-release code quality and hardening (Spec 053), the first public-release documentation pass (landing-page README, root `CONVENTIONS.md`, branding, `publish = false` with manual build/release workflows plus a release smoke test), a consolidated test-coverage summary in `marreta doctor` (Spec 054), the `docs/` grouping plus the `e2e/` suite and Release E2E workflow (Spec 055), a one-line install script with a cross-platform install-validation workflow (Spec 056), a CLI surface trim that removed the off-identity `run` and `repl` commands and hid the `tokenize`/`parse` debug commands (Spec 057), consistent framing (header rule + footer with status and elapsed time, on `stderr`) for the one-shot human-facing commands (Spec 058), and a VS Code extension fix-and-enrichment pass (interpolation unused-variable fix, `tooling definition` cross-file go-to-definition, lint diagnostic spans, purple mallet icon, palette commands, CodeLens, quick-fix, status bar — extension stays a thin CLI client) (Spec 059), and a messaging producer split so topics publish via a dedicated `topic.publish` namespace while queues keep `queue.push`, symmetric with `on topic` / `on queue` (Spec 060), and file-name namespaces so an exported task is reached cross-file as `file.task()` (consistent with `db.find`), with cross-file bare calls retired, load-time collision rules, a `doctor` Modules section, an `unused_exported_task` lint, and matching VS Code coloring/completion (Spec 061), and a relation-aware payload validator so persistent (`db:`) schemas work as API contracts even inside a relation cycle (references to persistent schemas are foreign-key relations, let through, not recursed), with the only remaining loop — an all-value-schema cycle — re-checked at load and shared with `marreta lint` via one helper (Spec 062), and a runtime compatibility manifest where a project declares the minimum Marreta runtime it needs via `requires_marreta` in `app.marreta`, enforced at load (`IncompatibleRuntime`), stamped by `marreta init` from a `COMPAT_FLOOR`, and governed by a language-versioning policy in SPEC.md §1.5 (Spec 063), and the first hand-authored documentation guide under `docs/guide/`, a Diátaxis tree (tutorials, how-to, reference, concepts) serving GitHub and the future site, kept complete by process (a marreta-spec Documentation axis plus the PR checklist) rather than a generator or CI gate, with a `topic.publish` scenario-mock parity fix shipped alongside (Spec 064), and a developer-experience polish pass — a token-aware `marreta fmt` that normalizes intra-line spacing (comment/string-safe, guarded by a layout-aware token-stream invariant plus idempotency), a clear `invalid indentation` parser error for over-indented lines, provider connection progress logs at `marreta serve` startup, and an authenticated MongoDB healthcheck in the `marreta init` docker-compose template (Spec 065), and document indexes that the runtime infers from the query surface (the `where`/`order` shape of each `doc.<collection>` pipeline) and ensures in MongoDB at `serve` startup, with no declaration, no marker, and no migration (Spec 067, which superseded a reverted pre-release declarative approach, preserved at tag `pre-067-revert`), and the rigorous, publishable launch benchmark study that makes `docs/benchmarks/digital_bank` a pre-registered experiment validating the three about-page hypotheses against three idiomatic contenders (FastAPI, NestJS, Spring Boot), re-run on a dedicated VM after Spec 067 so it exercises the auto-indexing runtime (Spec 066), and — after the launch-governance and security passes (Spec 075 contribution flow, Spec 076 db identifier hardening) — query and header input schemas, where a `schema` declares query/header inputs through a per-binding `take ... as` (the route-level `as` removed), with two `take` layouts (inline / multi-line, no hybrid), flat-only validation and string coercion (exact match for query, `_`/`-` case-insensitive convention for headers), named/typed OpenAPI parameters (the misleading `deepObject` query parameter removed), and schema field defaults deferred to a named follow-up so the persistent model is untouched (Spec 077). Runtime package version remains `0.2.0`.

> Detailed per-spec session-log entries for the work delivered after v0.16 were intentionally not backfilled. See the specs under `docs/spec/` (050-060) and the git history for specifics.

---

## Session Log

### 2026-06-15 — Spec 077 Query and Header Input Schemas (declare/validate/coerce non-body inputs)

- [x] `spec/077_QUERY_HEADER_INPUT_SCHEMAS.md` — query and headers were the only request inputs with no declaration, validation, or coercion, and the generated OpenAPI was blind and wrong (a single generic query parameter with `style: deepObject`, describing `?query[term]=` while the runtime reads flat `?term=`). A `schema` can now be bound to query and headers, mirroring `take payload as Schema`. **Per-binding `as`**: `TakeBinding` became a struct (`kind`/`name`/`schema`) and the route-level `schema` field was **removed** from `Route`/`RouteDefinition`, so every reader had to move to the payload binding — compile-enforced rather than coverage-dependent (the design-gate edge). The payload-schema readers (payload validation in `server.rs`, request body in `openapi.rs`) now resolve via `ast::payload_schema(&take)`; the other three `as` uses (queue/topic consumer, task param, `reply as`) are untouched.
- [x] **Two `take` layouts**: inline (single `take`, comma-separated) or multi-line (N leading indented `take` lines), with no-hybrid and takes-before-logic as parse errors. **Coercion** (`validator.rs::coerce_scalar_input`): text→type, boolean `true`/`false` only, `list of <scalar>` fed by a repeated key, empty value treated as absent; query matches the parameter name **exactly** (case-sensitive) while headers map by a case-insensitive `_`/`-` convention. **Flat-only** (`first_non_flat_field`): a schema bound to query/headers may use only scalars and lists of scalars — a schema reference, `list of <Schema>`, or map is a load-time error (`file_loader`) and a `non_flat_input_schema` lint. **OpenAPI**: named/typed parameters per field for a bound schema (list → array, `required` from optional), a raw bind emits no parameters, the `deepObject` parameter removed. `RawQuery` was threaded through `execute_route` and the scenario runner so a repeated-key list coerces identically in-memory and on the live server.
- [x] **Defaults deliberately cut** from this round (the only thing that would touch the shared schema model and ripple into the persistent path/`migrate`); they become a named follow-up (DB column default + migrate default-drift on top of the delivered Spec 073). Two review gates: a multi-round design brainstorm (reuse `schema`, per-binding `as`, the four product decisions, no-hybrid, the deepObject removal, defaults cut) and a code review approved with no findings, verified against the diff.
- [x] Coverage: unit tests (coercion, flat-check, parser layouts, lint, OpenAPI); `functional_tests` section 68 (26 asserts incl. decimal/instant, enum, list, required header, case-insensitive mapping, the three-typed combo) for **601/0**; e2e **68 scenarios + 41 live smoke** including the served `/openapi.json`. Gates green: `fmt`, `clippy -D warnings`, full suite, `functional_tests` 601/0, `migrations_functional` PASS **unchanged** (proving the persistent model was untouched), `e2e`, `vsce package`. No-regression proven live against the example apps (smart_inventory 30/0, omni_hub 20/0, ecommerce 40/0) plus the digital_bank app and the `init` scaffold loading. Docs: a new `how-to/read-request-inputs.md` (every take variation + how to read each input) plus `concepts/schemas`, `validate-a-payload`, `openapi-docs`, `conventions`, `reference/lint`, and the README — every example run against a served project. Post-merge follow-ups: the `docs/guide` site sync and the TM Dev Lab launch-post review.

### 2026-06-13 — Spec 076 Db Identifier Hardening (runtime guard for the SQL identifier vector)

- [x] `spec/076_DB_IDENTIFIER_HARDENING.md` — closed the SQL identifier injection vector (CONCERNS.md item 6, the last named security follow-up from 071). Filter values are parameterized, but identifiers (`order_by`, `select` columns, filter columns) were concatenated verbatim into the SQL, so `db.products >> order_by(query.sort) >> fetch` with request input was injectable. The 071 lint only warned at dev time and is suppressable; this adds the runtime guard in the query builder (`src/db/query_builder.rs`), layered because `db.<table>` works with or without a declared schema: a **universal syntactic floor** validates every identifier to `name`/`table.column` and emits it double-quoted (`invalid_identifier` otherwise, so injection is structurally impossible even for a schema-less table), and an **optional schema layer** (`known_columns: Option<HashSet>` threaded onto `QueryState`, populated from the persistent schema) rejects an unknown column (`unknown_column`) when a `db:` schema declares the table.
- [x] `order_by` is a `column [asc|desc]` comma-separated mini-parser, so **dynamic sort stays first-class** (`order_by(query.sort)` with a real column sorts safely). `select` takes bare column identifiers only (computed aliases were deferred and unimplemented per Spec 009; the runtime dropped the alias name with no `AS`), and `SPEC.md` section 4.3 was synced to match. Two implementation decisions recorded in the spec: the count terminal renders `COUNT(*)` as trusted SQL via a `count: bool` flag (so the only writer of `select_cols` is user input), and a rejected identifier is a client 400 that is **not** logged as an uncaught runtime error (consistent with 422 input validation; the security consequence — injection attempts are not in the event log, attack-attempt observability is a separate concern — is recorded deliberately).
- [x] Both rejections (`invalid_identifier`, `unknown_column`) map to 400 through the clean error path, never leaking SQL. Defense in layers with the 071 lint: the lint warns at dev time, the guard bars at runtime. Pure classifiers (the floor, the order_by parser) tested in isolation (the 067/071 security pattern); functional coverage in `functional_tests` section 18D exercises the four vectors against real Postgres (injection in `order_by`/`like`/`in`/`select` rejected, dynamic sort survives, `unknown_column` on a schema-backed table), with the intentional runtime identifiers dogfooding the 071 suppression valve. Two review gates (a design brainstorm that resolved the layered floor-plus-schema design and corrected the select sharp corner, and a code review that recorded the count and log-filter decisions). Gates green: core, runtime (functional 575/0, migrations, e2e). Docs (`db.md`, `errors.md`) synced to the site.

### 2026-06-12 — Spec 075 Contribution Flow (tiered SDD, governance surface for launch)

- [x] `spec/075_CONTRIBUTION_FLOW.md` — the repo had build gates but no contribution flow: nothing told a stranger how a change gets accepted, issues were free-form, the spec discipline was invisible, and there was no security-reporting path. Pulled from the stealth parking lot and made public: a **tiered model** (trivial straight to PR, substantial proposal-first and spec-driven, bug through a structured report), carried by a root `CONTRIBUTING.md` (the tiered model, the focused-by-design scope clause, the development prerequisites/layout/gates moved out of the README, the MIT inbound-equals-outbound line, honest review expectations), GitHub issue forms (`proposal.yml` mirroring the spec template, `bug_report.yml` with version/platform-incl-WSL/repro/expected-vs-actual/not-a-security checkbox, `config.yml` with blank issues off and links to Discussions and the private advisory), a public canonical `docs/spec/TEMPLATE.md` (the frozen 071-074 format, a fillable skeleton with the three named coverage axes), and `SECURITY.md` (private vulnerability reporting plus `contact@marreta.dev`).
- [x] **Code of Conduct** (owner decision: adopted): the verbatim Contributor Covenant 2.1, fetched from contributor-covenant.org with the contact filled. The review replaced an initial clean-worded paraphrase (a workaround for a generation-time content filter) because adopting a recognized document only has value if it is recognizable.
- [x] **PR template reworked** to the escalated shape that distills what specs 071-074 taught: single auto-applied template, trivial fills Summary + Type and stops (HTML comment guide); the substantial path adds a `Linked issue, proposal, or spec` field (the design gate as a field that bounces when empty, covering all three tiers), Tests with functional coverage of new behavior and e2e for language features and do-not-weaken-tests, a Coverage analysis of one line per axis with a mandatory reason on the negative (defeats the reflexive tick), and a conditional live-proof line for changes that only prove post-merge. Vocabulary aligned with `TEMPLATE.md`; section renamed to Docs of record.
- [x] **Repo settings** enabled via `gh` and confirmed by read-back before the merge (the order-gate condition, so the `config.yml` and `SECURITY.md` links resolve on push): `has_discussions = true`, private vulnerability reporting `enabled = true`. Two review gates: a design brainstorm (CoC adopt + contact decisions, the repo-settings conditions, TEMPLATE fillable + named axes) and the PR-template brainstorm (single template, line-with-reason coverage, the design gate as a field, the live-proof line), plus a code review that shipped the verbatim CoC, fixed the how-to-style link gaps, and broadened the Linked field to the bug tier. Governance docs only, no code. Issue-form render and PR-template application are post-merge live proofs (YAML forms only render on GitHub). With 075 the stealth parking lot is empty and the launch governance surface is complete.

### 2026-06-12 — Spec 074 Editor Extension Marketplace Listing (user-first README, CLI up front)

- [x] `spec/074_EDITOR_EXTENSION_MARKETPLACE_LISTING.md` — the VS Code extension's `README.md` is what the VS Code Marketplace and Open VSX render as the detail page, and it read as a source-tree note: it opened with "This folder contains the bundle", listed features as an implementation inventory, shipped the publishing instructions (PAT/`vsce`) and a maintainer checklist to the public page, and buried the one fact that decides the first run (the extension is a thin client and needs the `marreta` CLI, or nothing works). Rewritten user-first: value proposition, the **CLI requirement above the fold** linking the canonical how-to (`marreta.dev/docs/how-to/install-the-editor-extension`, no second copy of install steps), features by outcome, settings, a one-paragraph "how it works" (thin client as a property, scope in one line), and links. Text-only, no emoji or badges (the marketplace renders its own chrome, so a badge there is a duplicate). The "Ruby-inspired" framing was removed at the owner's correction as inaccurate.
- [x] **Screenshot** (`images/completion.png`, a completion popup with inline docs) referenced by an absolute `raw.githubusercontent.com` URL pinned to `main`, because the marketplace rewrites relative image URLs (and Open VSX diverges), so a relative path that previews fine locally renders broken live (the Spec 070 `--latest` class of bug). **Maintainer content** moved to `PUBLISHING.md` (publishing via Spec 070's `release-vscode.yml` referenced not duplicated, the namespace prerequisite, the language-surface checklist, and the grammar/scope inventory as a coverage record), excluded from the VSIX via `.vscodeignore`. `package.json`: search-blurb `description`, `version` 0.2.18 to 0.2.19, `Formatters` category, discoverability keywords.
- [x] **Reviewer brainstorm + two gates.** Design answered five questions (one static screenshot as the floor with GIFs deferred; strictly text on principle; scope folded to one line; `PUBLISHING.md` co-located; the agreed blurb) and four findings, the structural one being "do not create a third source of install instructions" (honored by linking the canonical how-to). The diff review caught a broken link (the how-to URL omitted `/docs/`, diverging from the canonical path the root README and 070 release notes use, the exact link a user without the CLI clicks); both the relative-image-URL rewrite and the link path are invisible to a local Markdown preview, so they were verified against the canonical URLs. Extension tier gate green (`node --check` + `vsce package`, VSIX ships only `readme.md`). The listing goes live on the next `release-vscode.yml` publish (tag == version 0.2.19), not on merge.

### 2026-06-12 — Spec 073 Migrate Roundness Pass (replay tolerance, drift report, collision guard)

- [x] `spec/073_MIGRATE_ROUNDNESS_PASS.md` — closed the two day-to-day `marreta migrate` gaps the reviewer found by probing live Postgres. **The trap (2.1):** the replay that derives schema state for `diff`/`generate` (`apply_local_migration_to_schema`) accepted only `CREATE TABLE`/`ALTER TABLE` and hard-errored on anything else, so a hand-written `CREATE INDEX` (applied in production) broke `generate` forever with no non-destructive exit. Now three tiers: auto-tolerate schema-neutral classes (`CREATE/DROP INDEX`, `INSERT/UPDATE/DELETE`, `WITH`), an explicit `-- marreta: skip-replay` marker, and still-rejected column-mutating DDL with an actionable error (file, statement, rule, two options). The statement splitter is string-, dollar-quote-, and comment-safe; a parenthesis-aware column-def splitter replaced the per-line parse.
- [x] **The silence (2.2):** the additive-only planner ignored unsupported changes, so a column type change reported "up to date". `DatabaseColumn` now carries `rendered_type`/`nullable` (both `Option`, precision over recall), captured during replay; `detect_schema_drift` (a sibling of `plan_migration`) reports type/nullability changes, removed fields, and removed tables without producing an operation; `diff`/`generate` print the doctor-style block. Four pins keep the best-effort capture trustworthy: parenthesis-aware capture (`NUMERIC(10, 2)`), alias normalization capped to the `postgres_type()` set (`int8`=`bigint`, `timestamptz`=`timestamp with time zone`, ...), `PRIMARY KEY` implies `NOT NULL` (mirrored on the desired side), and an unresolved type is skipped silently.
- [x] **Roundness (2.3):** `write_migration_files` refuses a same-second version collision instead of clobbering.
- [x] **Two review gates** (design pulled implementation-ready; the diff review found a third gap the original probes missed — an apostrophe or `;` inside a comment in hand-written SQL, which broke the splitter and corrupted the type capture — fixed by making the splitter and comment strip string-safe and comment-aware). Functional coverage: `migrations_functional` extended with the section-6 probe as live assertions (the trap made green, skip-replay, the rejection error, the drift report) against real Postgres; also validated manually end-to-end via `init --with db`. Docs: the migrations how-to (hand-written SQL contract, drift report, troubleshooting, discard wording) and `cli.md`'s `diff`/`generate` rows. Gates green: core (fmt, clippy, full suite), runtime (functional 567, migrations functional, e2e). No extension/e2e language change (migrate has no editor surface).

### 2026-06-12 — Spec 072 Fmt Consistency Pass (shared discovery, blank/newline/comment normalizations)

- [x] `spec/072_FMT_CONSISTENCY_PASS.md` — grew `marreta fmt` from a per-line normalizer into a project-consistent one. **Correctness fix (2.1):** `formatter::discover_project_files` walked a fixed four-directory list (`routes`/`schemas`/`tasks`/`tests`) and silently skipped files the runtime loads in custom folders (`auth/`, any custom dir). It now delegates to the loader's recursive `collect_marreta_files` (`src/file_loader.rs`, made `pub(crate)`), the single source of project discovery, pinned by an invariant test (`fmt_discovery_equals_loader_discovery_plus_entrypoint`). Lint discovery was already recursive, verify-only.
- [x] **Normalizations (2.2-2.5):** runs of 2+ blank lines collapse to one, blanks stripped at both file edges, exactly one final newline, and a leading `#` followed by a non-space, non-`#` char gains one space (`#comment` to `# comment`; `##` and bare `#` untouched). Unit tests per rule.
- [x] **Foundation touch, recorded.** The final-newline rule (2.3) is not "safe by construction" as the parked spec claimed: `significant_tokens` snapshots `Newline`, and at the end of an indented file the file-terminal `Newline` sits behind synthesized `Dedent`s (`... Newline Dedent* Eof` with a final `\n` vs `... Dedent* Eof` without). The snapshot now walks back over the terminal `Dedent` run and drops the single `Newline` behind it on both sides, so the normalization never trips the token-stream divergence guard while every interior `Newline` and every `Dedent` stays protected. Surfaced explicitly in review (not embedded), which let the diff review catch the gap with a live probe; two unit tests guard it (the corpus cannot, every corpus file already ends in a newline).
- [x] **Docs + stance (2.6):** `reference/conventions.md` gained the comment-spacing rule and a Formatting section with the deliberate-non-goals stance block (no wrapping, no alignment, no sorting, no reflow, with the semantic-token rationale); `reference/cli.md`'s fmt row states it formats every file the project loads. CLI integration tests over the real binary (non-canonical dirs, `--check` flip, stdin normalizations). Corpus reformatted (`docs/examples` + `e2e`); the `functional_tests` files carried pre-existing manual alignment the 071 binary already flagged.
- [x] Two review gates passed (design pulled implementation-ready from the parking lot, then a diff review that found and proved the terminal-newline gap, F1 fixed with two tests + F2 spec-text correction). Gates green: core (fmt, clippy, full suite), runtime (functional 567/567, migrations, e2e). No extension change (per-file `--stdin` provider unaffected).

### 2026-06-12 — Spec 071 Lint DX Pass (six new rules on a catalog, suppression, docs, editor links)

- [x] `spec/071_LINT_DX_PASS.md` — grew `marreta lint` from 8 rules to 15. Infrastructure first: a rule **catalog** as the single source (code, default severity, summary), enforced by a `LintDiagnostic::new` debug assertion (no un-catalogued code) and a catalog-to-docs invariant test (every code has a `### <code>` anchor on the reference page, the Spec 068 drift-proofing pattern); and **inline suppression** (`# marreta: allow <code>`, string-aware, standalone silences the next line, trailing its own).
- [x] **Rules** (all warning): `shadows_injected_binding` (scope-aware, route-live bindings), `route_without_response` (a route that silently 204s), `match_without_fallback` (value-consumed only), `non_literal_sql_identifier` (a db `order_by`/`select`-alias/`like`/`in` identifier from a runtime value, hardened to catch interpolation), `unused_schema` (persistent excluded), `unused_auth_provider`. Each is backed by a verified runtime fact or a recorded debt.
- [x] **The rescue runtime fact, vetted before coding, inverted a design**: `rescue` is not a route-level handler, only an expression modifier and a pipeline stage, so `route_without_response` treats a rescue body as a value, not a path (a `fail` reachable only on the recovery path does not save the happy path). The deciding case is the corpus's `/errors/rescue_block`.
- [x] **Docs + editor**: a `reference/lint` page (section per code, anchored); the VS Code extension links each diagnostic code to its docs anchor and offers a suppress quick-fix (after any real fix, never preferred). **Corpus** made lint-clean of the new rules: two orphan schemas removed, and the deliberate `/errors/rescue_block` fixture suppressed with a comment, dogfooding the valve.
- [x] Closes the `shadows-injected-binding` follow-up registered since Spec 068; opens `db identifier hardening` as a named security follow-up (the runtime guard for the SQL identifier vector). Gates green: core (fmt, clippy, suite +~30 lint tests), runtime (functional 567/567, migrations, e2e), extension (node --check + VSIX).

### 2026-06-11 — Spec 070 VS Code Extension Release Workflow (versioning + VSIX GitHub release)

- [x] `spec/070_VS_CODE_EXTENSION_RELEASE_WORKFLOW.md` — the extension's manual release path in the monorepo. `.github/workflows/release-vscode.yml` (`workflow_dispatch`): tag-equals-`package.json` guard before packaging, create-tag-if-missing, single runner, VSIX via `vsce package`, GitHub Release with `make_latest:false` under a `vscode-v*` tag namespace (so it never steals the runtime's `releases/latest` that `install.sh` resolves), and Open VSX + MS Marketplace publishes each gated on a secret with a loud per-channel run summary. A self-verify step asserts the release is not the API `releases/latest` and re-downloads the published VSIX to check its version.
- [x] **Dry-run proof (AC6):** dispatched against `vscode-v0.2.18`, green. The verify confirmed the API `releases/latest` does not point at the extension tag and the published VSIX version matched. One fix landed during the proof: the verify used the `releases/latest` API endpoint instead of a non-existent `gh release view --json isLatest` field. The UI "Latest" chip on the sole release is cosmetic and migrates to the first runtime release.
- [x] **Install how-to** (`docs/guide/how-to/install-the-editor-extension.md` + SUMMARY, mirrored to the site): binary-first, command-palette-first (`Extensions: Install from VSIX`), with the `marreta.path` settings detail and the registry sections framed as forthcoming until live.
- [x] **Curated release bodies** (`.github/release-notes/{runtime,extension}.md`): both releases get an authored body. The runtime dropped `generate_release_notes` (noisy "What's changed" + "Full Changelog" commits link on a first release; `CHANGELOG.md` is internal by its own charter). Decision (a): the first release ships with no changelog section, the second introduces hand-curated highlights in the body.
- [x] Two review gates passed (design + diff), plus a polish round (palette-first install, settings detail, lean runtime body). Core + extension gates green. Anti-squat: create the `MarretaTeam` namespace on Open VSX and the publisher on the MS Marketplace before enabling those channels. Follow-up: the second release introduces curated highlights.

### 2026-06-11 — Spec 069 Docs for Inferred Indexes and Reserved Words (guide pages for 067 + 068)

- [x] `spec/069_DOCS_FOR_INFERRED_INDEXES_AND_RESERVED_WORDS.md` — the hand-authored `docs/guide` pages for two features that shipped with only their docs of record, paying the guide debt that 067 and 068 deferred. Docs-only, no runtime change.
- [x] `reference/namespaces/doc.md` — an **Indexes** section: the mechanism (the runtime infers each collection's index from the `where`/`order` query surface and ensures it in the background at `marreta serve` start, no declaration / marker / migration, reported by `marreta doctor`) and the boundaries (the `like` / `doc.pipeline` / non-literal-field / builder-indirection exclusions, the background-ensure tradeoff, never-drop/orphan, and hand-made-index coexistence linking the `unique_violation` 409).
- [x] `reference/keywords.md` — the deferred two-layer model reframed as the page's organizing intro (the rule "namespaces are reserved, directives and vocabularies are contextual", both layers, the existing groups noted as Layer 1) with a runtime-verified name-position-vs-binder example.
- [x] `concepts/namespaces.md` — a sentence on why a native namespace is reserved and cannot be shadowed, linking the model. `reference/errors.md` — a `unique_violation` (409) row, a Validation 422 note, and a non-exhaustive intro that no longer overstates that the HTTP status is always developer-chosen.
- [x] Two review rounds (design: F1 errors.md gaps + F2 index boundaries, S1/S2 adopted; diff: builder terminated with `>> fetch`, non-exhaustive errors intro, builder-indirection exclusion, exact `marreta serve` trigger). Examples verified against `docs/examples` and the runtime. Core gates green; no functional/migrations/e2e needed (no runtime change). Closes the 067/068 guide deferral.

### 2026-06-11 — Spec 068 Reserved Word Normalization (reserve doc/feature/env at the lexer)

- [x] `spec/068_RESERVED_WORD_NORMALIZATION.md` — `doc`/`feature`/`env` are reserved at the lexer (peers of `db`/`cache`/`queue`), closing the drift where SPEC.md documented `doc` as reserved but `keyword_lookup` did not, so a variable could shadow a documented namespace and the provider silently vanished from that scope. Done with a normalize-back parser: the new token does real work in exactly one place (blocking a binder), and every position downstream of declaration is unchanged.
- [x] **Dedicated error** (`src/error.rs`): new `MarretaError::ReservedWord` with a per-namespace message (`'doc' is a reserved word (the document database namespace); rename the variable.`), wired into `expect_identifier` so every binder fails uniformly. Blocked binders: assignment target, task name, task parameter, map/reduce block variable, schema name, auth provider name, consumer `take` binding, and the **route path parameter** — the last rejected at **load** (`route_loader`), since the name lives inside the route string literal and the lexer never emits the token there.
- [x] **Name-position tolerance** (`src/parser.rs`): one shared `expect_name` (identifier or any reserved word used as a name) now backs the after-`.`, map-key, schema-field, `db:`-table, and named-arg positions, and the type tokens normalize back in expression position so a `date`/`string` `select(...)` column parses. This also closed the pre-existing holes where each position carried its own hand-rolled keyword list (missing `db`, `date`, ...). A schema field named `doc`/`feature`/`env` is allowed; `db` stays unusable there because the `db:` directive claims that line.
- [x] **Drift-proofing + freeze** (`src/tooling/catalog.rs`, `src/parser.rs`): a catalog→token invariant test asserts every `CatalogKind::Namespace` has a lexer token (`env` excepted as a non-catalog accessor, tested directly), and a table test freezes every reserved token × every name position (positive) and every binder position (negative).
- [x] **Corpus + extension**: swept our own `.marreta` (examples/e2e/benchmark) and the `marreta init` templates — no `doc`/`feature`/`env` binder uses to migrate. The VS Code extension already tokenizes/colors the words (tmLanguage `namespaces`) and completes them (catalog-driven), so no extension change was needed.
- [x] Gates green: `fmt`, `clippy -D warnings`, the unit suite (1542), `functional_tests` 567/567, `migrations_functional` PASS, `e2e` PASS (+18 live smoke), extension `node --check` + VSIX package.
- [ ] Follow-up: Spec 069 (docs for 067+068, including the keywords-page two-layer reserved/contextual writeup) and the sister `shadows-injected-binding` lint.

### 2026-06-11 — Spec 066 Launch Benchmark Study (Digital Bank, post-067 re-run)

- [x] `spec/066_LAUNCH_BENCHMARK_STUDY.md` — the rigorous, publishable launch study in `docs/benchmarks/digital_bank`: four feature-identical apps (Marreta, FastAPI, NestJS, Spring Boot), a 120s JIT warmup + 300s steady-state window, three interleaved repetitions with median + CV and a consistency gate run over the full grid, fixed load levels (200/500/1000) plus a 250-step saturation ladder, and an objective DX measurement (total SLOC, dependencies/footprint, capability matrix, test-feedback time). MongoDB runs with headroom as the validity guard; only the apps are capped at 1 CPU / 1 GB.
- [x] **Re-run post-067** on a dedicated Azure VM so the study exercises the runtime that auto-indexes. Headline: Marreta is the only app that serves 1000 req/s cleanly on 1 CPU (p99 < 3ms, 0% error), sustains 1250 (CPU-bound), idle 4.3 MiB, 87 SLOC, 0 deps, test suite in 0.021s. The honest caveat is kept (Spring edges Marreta on p90 at 500 req/s).
- [x] **Method consolidated**: contender selection criterion with survey citations, the neutral no-manual-optimization statement, the re-run policy, an at/above-ceiling tail-variance rule (the two flagged cells marked, not hidden), the generator-ceiling note (the >1250 zero-throughput was the k6 VU pool, not an app collapse), 20ms TTFR, one SLOC rule. The business-vs-wiring split was dropped (subjective on single-file apps); the whole `results/` tree is git-ignored and regenerable, with `RESULTS.md`/`METHODOLOGY.md`/`DX.md` as the data of record. No `src/` change; `fmt` and `clippy` green. Closing review approved.

### 2026-06-10 — Spec 067 Inferred Document Indexes (infer from queries, ensure at serve startup)

- [x] `spec/067_INFERRED_DOCUMENT_INDEXES.md` — the document provider infers each collection's index from its query surface (the `where` equality/range and `order` sort of every `doc.<collection>` / `doc.query("col")` pipeline) by the ESR rule, deduplicated by prefix, and ensures it in MongoDB in the background at `serve` startup. No declaration, no `doc:` marker, no migration. A new query shape is ensured idempotently on the next redeploy.
- [x] **Rewind**: supersedes a reverted pre-release declarative approach (`index` / `index unique` / `doc:` plus relational migration indexes), preserved at tag `pre-067-revert`. The `unique_violation` 409 mapping and the doc-driver ensure machinery were cherry-picked from the tag with their hardened tests; the ensure signature was extended to carry index direction.
- [x] **Engine** (`src/doc/index_inference.rs`): total exhaustive AST walker (a new AST variant is a compile error, not a dropped shape), with named exclusions (`like`, `doc.pipeline`, non-literal fields, variable indirection). **Serve** (`src/main.rs`, `src/file_loader.rs`): plan inferred once at load, ensured concurrent with serving so it never delays the bind. **Doctor** (`src/doctor.rs`): a "Document indexes" section reports present / absent / orphan.
- [x] **Functional coverage** (AC9): the inferred composite `{ account_id: 1, _id: -1 }` is asserted physically present in real MongoDB by its owned name via `getIndexes`. Gates green: `fmt`, `clippy -D warnings`, the unit suite, `functional_tests` 567/567, `e2e`, `migrations_functional` (db unchanged).
- [ ] Cross-repo follow-up: update the "Spec 067" references in marreta-lang-stealth (security section, backlog) from the old declarative meaning to the inference meaning.

### 2026-06-08 — Spec 065 Developer Experience Polish (fmt spacing, parser error, serve logs, mongo healthcheck)

- [x] `spec/065_DEVELOPER_EXPERIENCE_POLISH.md` — four mechanically independent developer-experience improvements.
- [x] **Token-aware `marreta fmt`** (`src/formatter.rs`): on top of the existing indentation/blank-line pass, a comment- and string-safe line scanner normalizes intra-line spacing (one space around binary operators and `=`, after `,`/`:`, inside `{ }`, tight `.`/`?`/calls, unary `-`/`+` tight, and keyword operators `and/or/not/in` spaced before groups). Safety is a **layout-aware token-stream guard** — the full token stream including `Indent`/`Dedent`/`Newline`, dropping only `Eof`, must be identical before and after (a `Program` `PartialEq` is unusable because the AST carries positions) — plus idempotency. A corpus test formats every `docs/examples/**/*.marreta` under that invariant.
- [x] **Parser over-indent error** (`src/error.rs`, `src/parser.rs`): new `MarretaError::UnexpectedIndentation { line }` renders as an `invalid indentation` diagnostic naming the line, instead of the generic `expected expression, got ''`. Dedent message unchanged.
- [x] **serve startup progress** (`src/main.rs`): an up-front "Connecting to providers" line and a per-provider "connecting" line before each attempt, for configured providers only.
- [x] **MongoDB healthcheck** (`src/init.rs`): the generated docker-compose mongo healthcheck authenticates, closing the `docker compose up --wait` race; init test asserts `--authenticationDatabase`.
- [x] VS Code extension: no change (thin CLI client; `fmt`/`lint` shell out). Cross-repo follow-up: re-sync the site initializer fixtures (Spec 008) since `marreta init`'s compose changed.
- [x] Two gates passed (spec review, then code review of the diff with two fix rounds). Gates all green: `cargo fmt --check`, `clippy -D warnings`, full unit suite, and the runtime tier (release rebuilt, `marreta-lang:dev` image rebuilt, `functional_tests`, `migrations_functional`, `e2e`).

### 2026-06-08 — Request/message log `duration_ms` rounded (no spec)

- [x] `duration_ms` in the request and message log events is rounded to 3 decimal places
  (`src/server.rs`, via a `round_ms` helper) instead of carrying raw f64 sub-microsecond noise
  (e.g. `1.832666` now reads `1.833`). Matches the value shape already shown in
  `docs/guide/how-to/observe-logs.md`. Small no-spec change at the owner's request.
- [x] Gates: fmt + clippy(`-D warnings`); full unit suite 1515 + 3 + 38 + 37, 0 failed; runtime
  smoke (served a generated project, confirmed rounded `duration_ms`). The docker-backed
  `functional_tests`/`migrations_functional` suites were not rerun: the change only affects log
  formatting and cannot alter response behavior. Release binary rebuilt and installed to
  `~/.local/bin`.

### 2026-06-05 — Spec 064 Documentation System (`docs/guide`, authored Diátaxis tree)

- [x] `spec/064_DOCUMENTATION_SYSTEM.md` — a single hand-authored Markdown tree under `docs/guide/` (Diátaxis: tutorials, how-to, reference, concepts) serving GitHub and the future site (Spec 065). The bootstrap generator and CI completeness gate from the first proposal were implemented, used once to seed the reference, then removed: no `src/docs.rs`, no `marreta tooling docs`, no `tests/docs_gate.rs`. Coverage is now kept current by process.
- [x] Reference: one page per catalog namespace (14), a Types overview plus per-type pages (string, integer, float, decimal, boolean, list, map, temporal — each "the type and its methods", restructured from `reference/methods/`), keywords, control flow and operators, full per-variable `configuration` tables (purpose, accepts, required, default), CLI, and error codes.
- [x] Tutorials (quickstart, save-and-read-data, relational-api-with-migrations, make-it-event-driven), 12 how-to recipes, and a Providers concept page. Every code example is lifted from, or verified against, a tested project under `docs/examples` (scenario or live HTTP).
- [x] Runtime fix: `given topic.publish … returns …` registered under the `topic` namespace while `ScenarioQueueDriver` resolves publish under `queue`, so the mock raised an unconfigured-call 500. Aligned to `queue` (`src/scenario_tests.rs`) with a regression scenario (`functional_tests/tests/queue/publish_test.marreta`).
- [x] Coverage by process: the marreta-spec delivery checklist gains a Documentation axis; `.github/PULL_REQUEST_TEMPLATE.md` carries the docs checklist; SPEC.md §1.3 records the docs DoD axis. The `explanation/` Diátaxis mode is deferred (`concepts/` covers the provider model for now).
- [x] Gates: fmt + clippy(`-D warnings`); suite 1515 + 3 + 38 + 37; `functional_tests` 566/566 (after rebuilding `marreta-lang:dev`); `migrations_functional` PASS; `e2e` scenarios + 18 live smoke.

### 2026-06-05 — Spec 063 Runtime Compatibility Manifest (`requires_marreta`)

- [x] `spec/063_RUNTIME_COMPATIBILITY_MANIFEST.md` — a project declares the minimum Marreta runtime it needs via a new `requires_marreta` field in the committed manifest (`app.marreta`); the runtime enforces it at load. Orthogonal to `project_version` (the product's own version); lives in the manifest, never in the gitignored `marreta.env`.
- [x] `version.rs`: `COMPAT_FLOOR` (last breaking version; today `0.2.0`) + `parse_version`/`parse_requires_marreta` (frozen v1 format `>=X.Y.Z`, outer trim, zero-or-one space after `>=`; rejects non-string, multi-space/tab, `v` prefix, prerelease/build, missing components, other operators/ranges).
- [x] `error.rs`: new `IncompatibleRuntime { required, actual }` → `config_error`. `file_loader::validate_entrypoint_metadata` reads `requires_marreta`: absent → no check (backward compatible); non-string or malformed → `io_error` (sibling of the `project_name`/`project_version` checks); runtime below the minimum → `IncompatibleRuntime`. The check is at load, so `serve`/`test`/`doctor`/`migrate` inherit it.
- [x] `init` stamps `requires_marreta = ">=<COMPAT_FLOOR>"`; `doctor` shows it (with the running runtime) when the project loads; `lint` allowlists it. SPEC.md §1.5 records the Language Versioning Policy (breaking bumps minor in 0.x / major post-1.0 and advances `COMPAT_FLOOR`).
- [x] Dogfood: every example/benchmark/e2e `app.marreta` declares `requires_marreta = ">=0.2.0"`; `init_functional` asserts the stamp. Reverse axis (old app on newer runtime), `created_with_marreta` provenance, and rich ranges are out of scope.
- [x] Gates: fmt + clippy(`-D warnings`); suite 1515 + 3 + 38 + 37; `functional_tests` 566/566 (after rebuilding `marreta-lang:dev`); `migrations_functional` PASS; `e2e` 60 + 18; `init_functional` PASS; doctor/lint clean on every example.

### 2026-06-05 — Spec 062 Schema Reference Cycle Enforcement (Validator + Loader)

- [x] `spec/062_SCHEMA_REFERENCE_CYCLE_ENFORCEMENT.md` — persistent (`db:`) schemas must work as API contracts even in a relation cycle (deliberate language design), but the payload validator recursed into every reference and the Spec 006 circular check was dead on the live load path.
- [x] Validator (`src/validator.rs`) is relation-aware: a `Reference` to a persistent schema is a foreign-key relation (Spec 025) and is let through (accepted as-is, not recursed), so `take payload as User` terminates even when `User <-> Order` is a relation cycle — own fields validated, relations let pass. A value reference already on the validation stack is reported as an infinite-cycle error; `MAX_DEPTH` stays as a final backstop.
- [x] Shared rule in new `src/schema_cycle.rs` (`find_disallowed_cycle`): a cycle is disallowed only when it lies entirely within value schemas (DFS from value schemas, cutting edges into persistent schemas, mirroring the validator). `file_loader::detect_schema_cycle` (live load path → `CircularSchemaReference`) and `lint_schema_cycles` both call it; the dead `route_loader::detect_circular_references` + helpers/tests were removed.
- [x] Net behavior: all-value cycles (incl. value self-reference) fail at load; cycles through any persistent schema — all-persistent relations, a value schema referencing into a relation cycle, persistent self-reference trees — load and validate. Supersedes the Spec 061 relation-aware lint rule (flagged any value-touching cycle) and two interim review rules.
- [x] Tests: `schema_cycle` (8), `validator` (persistent-cyclic contract validates; value cycle errors), `file_loader` load tests, `lint` (mixed-through-persistent allowed), and `functional_tests` section 37 (7 live HTTP scenarios). The "❌ load error" cases stay in unit tests (a served project would not load).
- [x] Gates: fmt + clippy(`-D warnings`) clean; suite 1506 + 3 + 38 + 37; `functional_tests` 566/566 (after rebuilding `marreta-lang:dev`); `migrations_functional` PASS; `e2e` 60 + 18; lint/doctor clean on every example.

### 2026-06-04 — Spec 061 File-Name Namespaces for Exported Tasks

- [x] `spec/061_FILE_NAMESPACES_FOR_TASKS.md` — each `.marreta` file is an inferred namespace (its stem); an exported task is reached cross-file only as `file.task()`, consistent with the built-in namespaces (`db.find`). No parser change (`billing.charge(x)` already parses like `db.find(x)`). Pre-release breaking change: a bare cross-file call no longer resolves.
- [x] Loader (`src/file_loader.rs`): `ProjectRuntime` gains `task_namespaces: stem → { task → Task }`; non-entrypoint exported tasks register there instead of `global_env` (vars/entrypoint tasks stay global/bare). Load errors enforced: same-stem collision among exporters, stem equal to a built-in namespace, the reserved `app` stem, non-identifier stem, and a duplicate exported task within one file. The global `exported_names` dedup is relaxed to per-namespace for tasks (same name across files allowed; schemas/vars keep global dedup).
- [x] Interpreter (`src/interpreter.rs`): `MethodCall` intercepts `Identifier(ns).method()` before evaluating the object (mirroring built-in namespaces); a known namespace with no such exported task reports `task 'ns.method' is not defined` (not "variable not defined"); a bound variable shadows a namespace. Pipeline (`>> file.task`, incl. list iteration) and broadcast (`-> file.task`, incl. the pure fast path) resolve namespaced tasks via the registry.
- [x] Doctor: informational `Modules` section (each file-namespace → its exported tasks); never fails. Lint: new `unused_exported_task` (counts a bare call in the file or a `file.task` anywhere as a use); fixed a pre-existing gap where bare pipeline/broadcast targets were not counted as task uses; and fixed a pre-existing `circular schema reference` false positive — a cycle composed entirely of persistent (`db:`) schemas is a relational graph (foreign keys, resolved lazily) the loader already accepts, so it is no longer flagged, while any cycle involving a value schema (a real infinite-embed) still errors.
- [x] Tooling + VS Code extension (thin CLI client): `ToolingSymbol` carries `exported` + `namespace`; completions after `file.` offer only that file's exported tasks; go-to-definition resolves `file.task` to the right file when names repeat; a new semantic-tokens provider colors file-namespaces (and their exported-task methods) with the same scopes as the built-in namespaces. Spec 060's `topic` surface was already complete in the extension.
- [x] Migrated all repo `.marreta` cross-file bare task calls to `file.task()` (functional_tests, e2e, ecommerce, omni_hub, smart_inventory, digital_bank benchmark, `marreta init` scaffold); added e2e coverage (a `text.shout`/`text.wrap` file-namespace, incl. a private same-file helper and a `>> text.shout` pipeline stage) and wired three previously-dead functional_tests exported tasks (`count_chars`, `to_lower`, `interval_days`) into live routes.
- [x] Gates: fmt + clippy(`-D warnings`) clean; suite 1493 + 3 + 37 + 37; `functional_tests` 559/559 (after rebuilding `marreta-lang:dev`); `migrations_functional` PASS; `e2e` 60 scenarios + 17 live smoke + 0 lint diagnostics; extension `node --check` + clean VSIX (26 files).
- [x] Review follow-up: (1) `unused_exported_task` no longer has a false negative — bare calls are now indexed per file-namespace, so a bare `task()` in another file (which no longer resolves) does not suppress the warning; only a bare call in the declaring file or a `file.task` reference anywhere counts. (2) The reserved-namespace set is now derived from the catalog (`CatalogKind::Namespace`) instead of a hardcoded list, with a guardrail test, so it cannot drift when a native namespace is added. Removed two dead exported tasks the corrected lint surfaced in the ecommerce example. Gates re-run green (suite 1498 + 3 + 38 + 37; functional 559/559; migrations PASS; e2e 60 + 17).

### 2026-06-04 — Spec 060 Topic Publish Namespace

- [x] `spec/060_TOPIC_PUBLISH_NAMESPACE.md` — topics publish via `topic.publish`; queues keep `queue.push`; `queue.publish` removed (pre-release cutover), making the producer side symmetric with `on topic` / `on queue`.
- [x] AST `QueuePublish` → `TopicPublish`; parser gains a `topic.publish` producer arm (statement + pipeline) and drops `queue.publish`; interpreter `eval_topic_publish` + all operation/error strings read `topic.publish`.
- [x] Scenario `given` mock key moved to `topic`/`publish` (shared `expression_to_matcher` keeps producer and given consistent). Catalog already advertised `topic.publish`/`topic` — now correct against the parser.
- [x] Migrated `.marreta` sources (functional_tests, omni_hub, smart_inventory) and authoritative docs (013_QUEUE.md, 023_TESTING_DSL.md, SPEC.md); queue/parser publish unit tests migrated.
- [x] Semantics certified against real RabbitMQ: a functional test (two consumers on `ft.fanout` vs two on `ft.compete`) proves `topic.publish` fans out (N publishes → 2N receipts) while `queue.push` is point-to-point (N pushes → N receipts).
- [x] Gates: fmt + clippy(`-D warnings`) clean; suite 1480 + 3 + 35 + 37; `functional_tests` 557/557 (after rebuilding `marreta-lang:dev`); `migrations_functional` PASS; `e2e` green.

### 2026-06-03 — Spec 059 VS Code Extension Fixes and Enrichment

- [x] `spec/059_VSCODE_EXTENSION_ENRICHMENT.md` — fix + enrich the editor extension; extension stays a thin CLI client.
- [x] CLI contract: `src/lint.rs` interpolation unused-variable fix + diagnostic spans (`end_line`/`end_column`); `src/tooling/symbols.rs` emits `auth` providers + project-less source symbols; new `src/tooling/definition.rs` + `marreta tooling definition` (token-at-cursor + AST/project context, resolves task/schema/auth, null otherwise); `src/main.rs` wiring + project-less `tooling symbols` stdin fallback. Unit tests added.
- [x] Extension core (`docs/editors/vscode`): DefinitionProvider; `toolingContext` standalone-file support; one-time CLI-missing notification; full-token diagnostic spans; indentation fix (no false indent after `require/reject … else`).
- [x] Enrichment: purple mallet icon (`languages[].icon` + opt-in file icon theme, assets under `icons/`) + gallery icon; snippets (reply/fail/match/if-else/auth/take/pipeline/http_client/db); palette commands (Serve/Test/Doctor/Init/Format); CodeLens (run scenario / serve); "Remove unused variable" quick-fix (CLI-diagnostic driven); status bar (version + health); marketplace metadata.
- [x] Deferred (non-blocking): terminal-task `problemMatcher` (the diagnostics provider already feeds the Problems panel), and CLI fix-suggestions beyond "remove unused variable".
- [x] Gates: CLI — fmt + clippy clean, suite 1472 + 3 + 35 + 37, functional 548/548, migrations PASS. Extension — `node --check` on all JS, JSON/SVG valid, clean VSIX (24 files).

### 2026-06-03 — Spec 058 CLI Command Framing

- [x] `spec/058_CLI_COMMAND_FRAMING.md` — consistent frame for one-shot human-facing commands.
- [x] `src/cli_ux.rs` (new, bin-only): `begin`/`end`/`abort` print a header rule and a `<glyph> <summary> · <elapsed>` footer to `stderr` (cargo model), with `COLUMNS`-based width (clamped 24–60) and color only when `stderr` is a TTY and `NO_COLOR` is unset. `format_elapsed` moved here from `main.rs`.
- [x] `src/main.rs` — `fmt`, `lint` (human/non-JSON), `doctor`, `init`, `test`, `migrate` call `begin`/`end`; the `exit_with_*` helpers call `abort()` so hard errors also close the frame. `test`'s `Finished in …` lines removed (superseded by the footer). `serve`, `tooling`, machine modes, `--version`/`--help`, `tokenize`/`parse`, and bare `marreta` stay unframed.
- [x] Additive contract: the frame is on `stderr`, so every command's `stdout` body is byte-identical (the one exception is `test`'s removed timing line). `e2e/run.sh`, `functional_tests`, `migrations_functional` unaffected.
- [x] `tests/integration_tests.rs` — framing tests (frame on stderr, data on stdout, JSON frame-free, NO_COLOR plain); `--coverage` golden test reads to end of stdout.
- [x] Failure paths are framed too: the frame opens in the `main` arm before argument parsing (machine-mode pre-scan keeps `fmt --stdin` and `lint --format json` frame-free), so a parse failure also closes the frame via `abort()`.
- [x] Gates: `cargo fmt --check` + `cargo clippy --all-targets -- -D warnings` clean; suite 1462 + 3 + 35 + 37; `e2e` 59 + 17; `functional_tests` 548/548; `migrations_functional` PASS.

### 2026-06-03 — Spec 057 CLI Surface Trim

- [x] `spec/057_CLI_SURFACE_RUN_DEBUG_REPL.md` — trim the CLI to the API workflow.
- [x] `src/main.rs` — removed the `run` and `repl` subcommands and their helpers (`run_file`, `run_repl`, `execute_source`, `needs_continuation`, `print_repl_help`, REPL special commands); bare `marreta` now prints help; help text no longer lists `run`, `repl`, `tokenize`, or `parse`. `tokenize`/`parse` stay callable for engine debugging.
- [x] No runtime change: an output sink on the interpreter was briefly explored to keep the `run`-based test harness, then reverted — bending the runtime for tests was the wrong trade.
- [x] `tests/integration_tests.rs` — removed the 72 `run`-based tests and the `run_source`/`run_file` helpers, the run-only CLI tests, and the REPL banner test; added `tokenize`/`parse` smoke tests and a bare-invocation help test. Coverage audited as redundant with the interpreter unit tests (`src/interpreter/tests*.rs`, 536 tests) plus `e2e` and `functional_tests`.
- [x] Gates: `cargo fmt --check` + `cargo clippy --all-targets -- -D warnings` clean; suite 1462 lib + 3 bin + 35 HTTP + 31 integration; `functional_tests` 548/548; `migrations_functional` PASS.

### 2026-06-03 — Spec 056 Install Script and Install Validation

- [x] `spec/056_INSTALL_SCRIPT_AND_VALIDATION.md` — one-line installer plus a cross-platform install-validation workflow.
- [x] `install.sh` — POSIX installer at the repo root: host detection (`MARRETA_TARGET` override), latest via the `releases/latest/download` redirect (no GitHub API, no `jq`), pinned via argument or `MARRETA_VERSION`, install to `~/.local/bin` (`MARRETA_INSTALL_DIR` override), absolute-path `--version` verification, and a PATH hint that never edits a shell profile.
- [x] `.github/workflows/install.yml` — manual `tag` input; five-leg matrix (Linux x86_64/arm64, macOS x86_64 via Rosetta, macOS arm64, Windows via WSL); each leg runs the checked-out script into `$RUNNER_TEMP/marreta-bin` and asserts `"$MARRETA_INSTALL_DIR/marreta" --version` matches the tag, without a separate download step.
- [x] `README.md` — Installation section leads with the one-line installer; manual download kept as the alternative.
- [x] Validation: `sh -n` syntax check, `shellcheck` clean (one intentional SC2016 literal documented), host detection, and the missing-asset error path (readable message, non-zero exit). The download-install-verify happy path is validated by `install.yml` against a published release across the OS matrix.

### 2026-06-02 — Spec 054 Doctor Test Coverage Summary

- [x] `spec/054_DOCTOR_TEST_COVERAGE_SUMMARY.md` — delivered a consolidated, static test-presence section in `marreta doctor`.
- [x] `src/coverage.rs` — new shared module: `CoverageSummary` + `summarize` + `route_key`, so `marreta test --coverage` and doctor count presence one way.
- [x] `src/scenario_tests.rs` — exposed `plan_scenario_route_presence` (the one public matching helper) wrapping the private `scenario_plan` / `find_route`.
- [x] `src/main.rs` — `print_api_coverage` routes through the shared summarizer; `--coverage` output unchanged (regression-tested).
- [x] `src/doctor.rs` — new `Tests` section: consolidated counts only (scenarios declared, routes with/without a scenario, unmatched only when > 0), tolerant per-file scenario loading (a bad file is a soft `SKIP` note, never aborts), pointing to `marreta test --coverage`. Informational, never fails doctor.
- [x] Validation (clippy 1.96):
  - `cargo fmt --check` + `cargo clippy --all-targets -- -D warnings` → **clean**
  - `cargo test` → **1459 lib + 3 bin + 35 HTTP + 98 integration passed**
  - `examples/functional_tests` → **548/548 passed**
  - `examples/migrations_functional` → **PASS**

### 2026-05-24 — v0.16 Runtime Hot Path Profiling

- [x] `docs/spec/049_RUNTIME_HOT_PATH_PROFILING_AND_OPTIMIZATION.md` — delivered as runtime hot-path profiling only, with execution templates split into Spec 050 in the spec repository.
- [x] Added `MARRETA_RUNTIME_PROFILE=hot_path` profiling mode.
- [x] Added route-level aggregated shutdown output in JSON for hot-path phases:
  - `http_total`
  - `handler_total`
  - `route_clone`
  - `auth_eval`
  - `env_setup`
  - `request_binding`
  - `schema_coercion`
  - `ast_execute`
  - `interpolation`
  - `json_serialize`
  - `response_build`
  - `total_execute_route`
- [x] Profiling is disabled by default and avoids timestamp capture/allocation on the disabled path.
- [x] Runtime validation:
  - `cargo fmt --check` → **passed**
  - `cargo check` → **passed**
  - `cargo test` → **1452 lib + 3 bin + 35 HTTP integration + 96 integration passed**
  - `examples/functional_tests` → **573/573 passed**
  - `examples/smart_inventory` → **30/30 passed**
  - `examples/ecommerce` → **40/40 passed**
  - `examples/migrations_functional` → **passed**
  - `examples/init_functional` → **passed**
- [x] Profiler smoke validation confirmed shutdown JSON output with `http_total.count=1`.
- [x] Post-049 in-memory HTTP comparative baseline recorded in `marreta-lang-performance`:
  - `LOAD_TEST_POST_049_IN_MEMORY_HTTP_20260524.md`
  - Marreta avg across 3 runs: **0.298ms**
  - Marreta p95 across 3 runs: **0.481ms**
  - Marreta p99 across 3 runs: **0.577ms**
  - Marreta peak CPU mean: **18.74%**
  - Marreta peak memory mean: **33.24 MiB**

### 2026-05-20 — v0.15 Runtime Versioning

- [x] `Cargo.toml` package version set to `0.2.0` as the current runtime/CLI version.
- [x] Added shared runtime version metadata in `src/version.rs`.
- [x] Replaced hardcoded CLI version output in:
  - `marreta --version`
  - `marreta --help`
  - `marreta repl`
- [x] Server startup output now uses the same runtime version source while keeping `project_version` as application metadata.
- [x] Tests now derive runtime version expectations from `env!("CARGO_PKG_VERSION")` instead of fixed strings.
- [x] Validation:
  - `cargo fmt` → **passed**
  - `cargo check` → **passed**
  - `cargo test` → **1449 lib + 3 bin + 35 HTTP integration + 96 integration passed**
  - `cargo build` → **passed**
  - `cargo build --release` → **passed**
  - `target/debug/marreta --version` → `MarretaLang v0.2.0`
  - `target/debug/marreta --help` → header uses `MarretaLang v0.2.0`
  - `printf '.exit\n' | target/debug/marreta repl` → banner uses `MarretaLang v0.2.0`

### 2026-05-20 — v0.15 Repository Split

- [x] Created sibling local directories for the planned repository split:
  - `marreta-lang-spec`
  - `marreta-lang-vscode`
  - `marreta-lang-brand`
  - `marreta-lang-examples`
  - `marreta-lang-performance`
- [x] Copied extracted content out of the runtime repo:
  - `docs/spec` → `marreta-lang-spec`
  - `docs/vscode-marreta` → `marreta-lang-vscode`
  - `docs/logo` → `marreta-lang-brand`
  - `examples` → `marreta-lang-examples`
  - `docs/performance` + `tests/load` → `marreta-lang-performance`
- [x] Converted the remaining runtime integration tests that depended on
  `examples/snippets` into inline temporary-source tests.
- [x] Removed extracted directories from `marreta-lang`.
- [x] Removed root-level app/developer artifacts from the runtime repo:
  - `Dockerfile`
  - `marreta.env.example`
  - `CONVENTIONS.md` moved to `marreta-lang-spec`
  - local `.venv/` and `.claude/` removed and ignored
- [x] Runtime validation after split:
  - `cargo test --lib` → **1448 passed**
  - `cargo test --test integration_tests` → **95 passed**
  - `cargo test --bin marreta` → **3 passed**

### 2026-05-20 — v0.14c Editor Tooling / LSP

- [x] `docs/spec/045_EDITOR_TOOLING_LSP.md` — delivered CLI-backed editor intelligence with VS Code integration.
- [x] `marreta tooling catalog --format json` — added a versioned built-in catalog generated from the Rust core, including signatures, snippets, hover documentation, examples, and warnings.
- [x] `marreta tooling symbols --format json` — added static project symbol discovery for routes, schemas, tasks, consumers, and scenarios without executing startup/application code.
- [x] `marreta tooling completions --stdin --file ... --line ... --column ... --format json` — added completion support for namespaces, built-ins, project tasks, and project schemas.
- [x] `marreta tooling hover --stdin --file ... --line ... --column ... --format json` — added hover documentation for built-ins and project symbols.
- [x] `marreta lint --stdin --file ...` — fixed editor semantics so stdin buffers are overlaid onto the full project context when `app.marreta` is present, preventing false `unknown_schema_reference` diagnostics for schemas declared in other files.
- [x] `docs/vscode-marreta/` — upgraded the VS Code bundle from syntax-only support into a thin CLI-backed client for:
  - completions
  - hover
  - diagnostics
  - format document
  - document symbols
  - workspace symbols
  - snippets
- [x] VS Code diagnostics now filter project-wide lint output by document path, so diagnostics from another file are not rendered in the active editor.
- [x] Validation:
  - `cargo test --lib tooling::` → **10 passed**
  - `cargo test --lib lint::tests::stdin_overlay_uses_project_schema_context` → **passed**
  - `cargo test --test integration_tests test_tooling_catalog_symbols_completions_and_hover` → **passed**
  - `cargo test --test integration_tests test_lint_stdin_uses_project_schema_context` → **passed**
  - `node --check` for VS Code extension files → **passed**
  - `npx vsce ls` → **passed**
  - manual VS Code validation with temporary project → **passed after reload**

### 2026-05-20 — v0.14b Lint

- [x] `docs/spec/044_LINT.md` — delivered the first static source-quality linter.
- [x] `marreta lint` — added project-aware linting anchored at `app.marreta`.
- [x] `marreta lint --format json` — added editor/CI-friendly diagnostic output.
- [x] `marreta lint --stdin --file <path>` — added editor-oriented diagnostics for unsaved buffers.
- [x] Initial rules cover deterministic issues only:
  - source load errors
  - duplicate routes
  - unknown schema references
  - schema cycles
  - unreachable statements
  - invalid feature flag names
  - unused variables
  - unused private tasks
  - direct self-recursion
- [x] Validation at delivery included unit and integration coverage for clean generated projects, JSON diagnostic shape, stdin diagnostics, strict mode, duplicate routes, unused variables, and warning exit behavior.

### 2026-05-20 — v0.14d OpenAPI Docs Refinement

- [x] `docs/spec/046_OPENAPI_DOCS_REFINEMENT.md` — delivered trustworthy `/docs` and `/openapi.json` generation.
- [x] OpenAPI output now avoids broken `$ref` trees by including schemas referenced by public routes even when those schemas are private in Marreta source visibility.
- [x] Dynamic response statuses no longer get represented as a misleading fixed `200`; unknown dynamic status paths use OpenAPI `default` plus Marreta metadata.
- [x] Request bodies for `take payload` without a schema use a free-form JSON object fallback.
- [x] `take raw` is represented as `text/plain`.
- [x] Shallow literal response inference reduces noisy generic object schemas in Swagger UI.
- [x] Removed inappropriate `additionalProperties` noise from inferred response schemas, preventing Swagger UI from showing confusing `additionalProp1` placeholders.
- [x] Functional validation included running `examples/functional_tests`, opening `/docs`, inspecting `/openapi.json`, and confirming Swagger UI readability after fixes.

### 2026-05-18 — v0.14a Formatter

- [x] `docs/spec/043_FORMATTER.md` — approved and delivered the first canonical formatter for Marreta source files.
- [x] `marreta fmt` — added project-aware in-place formatting anchored at `app.marreta`, covering:
  - `app.marreta`
  - `routes/**/*.marreta`
  - `schemas/**/*.marreta`
  - `tasks/**/*.marreta`
  - `tests/**/*.marreta`
- [x] `marreta fmt <file|dir ...>` — added explicit path formatting for files or directories outside a project root.
- [x] `marreta fmt --check` — added CI-friendly check mode that lists unformatted files and exits with status 1 without writing changes.
- [x] `marreta fmt --stdin --file <path>` — added editor/LSP-oriented formatting for unsaved buffers without touching disk.
- [x] Formatter behavior:
  - normalizes indentation to four spaces per block level
  - removes trailing whitespace
  - preserves comments
  - preserves multiline map/list shape
  - inserts blank lines between top-level declarations
  - parses before and after formatting before writing files
  - includes the failing file path in formatter errors
- [x] Coverage:
  - unit tests for formatting idempotency, comment preservation, multiline shape, top-level spacing, project discovery, explicit path discovery, and parse failures
  - integration tests for `fmt`, `fmt --check`, `fmt --stdin`, invalid-file preservation, and `init -> fmt -> marreta test`
- [x] Validation:
  - `cargo test --lib formatter:: -- --nocapture` → **11 passed**
  - `cargo test --test integration_tests` → **85 passed**
  - `cargo test --bin marreta` → **3 passed**
  - `git diff --check` → **passed**

### 2026-05-15 — v0.13r Schema Constructors + HTTP Client Response Schemas

- [x] `docs/spec/041_SCHEMA_CONSTRUCTORS_AND_HTTP_CLIENT_SCHEMAS.md` — approved and delivered schema constructors plus HTTP client response schema validation.
- [x] `src/ast.rs` + `src/parser.rs` — added:
  - `SchemaName { ... }` constructor expressions
  - `http_client.*(...) as Schema` response validation expressions
- [x] `src/interpreter.rs` — added constructor evaluation with strict developer-authored validation:
  - required fields enforced
  - type coercion through the existing schema validator
  - undeclared fields rejected, including nested schema references
  - persistent schemas may omit `id` for creation flows
- [x] `src/interpreter.rs` — added `http_client.*(...) as Schema` semantics:
  - validates/coerces `response.body`
  - preserves `response.status` and `response.headers`
  - preserves extra upstream fields to match route payload ingress semantics
  - keeps operation context as `http_client.<verb>` for integration debugging
- [x] `src/doctor.rs` + `src/route_loader.rs` — updated expression visitors so constructors and HTTP response schema expressions participate in existing project analysis paths.
- [x] `examples/functional_tests/` — added functional coverage proving constructed schema maps work across:
  - `reply as`
  - cache transport
  - relational `db.*.save`
  - document `doc.*.save`
  - queue point-to-point and topic producers
  - HTTP client request construction and response validation
- [x] Validation:
  - `cargo test --lib` → **1387 passed**
  - `cargo test --test integration_tests` → **78 passed**
  - `cargo test --test http_integration_tests` → **35 passed**
  - `cargo test --bin marreta` → **3 passed**
  - `../../target/debug/marreta test` in `examples/functional_tests` → **34 passed**
  - `bash examples/functional_tests/test.sh` → **549 passed, 0 failed**

### 2026-05-14 — v0.13q Project Init Local Services

- [x] `docs/spec/040_PROJECT_INIT_LOCAL_SERVICES.md` — approved and delivered the reduced-scope project initializer extension.
- [x] `marreta init --with ...` — added local-service provisioning for selected infrastructure services without generating application tutorial code.
- [x] Generated projects keep the starter app small:
  - browser-friendly `GET /greetings`
  - no generated CRUD, migrations, consumers, or domain-specific examples
  - selected services documented in README with small namespace examples
- [x] Generated service scaffolding now includes:
  - `docker-compose.yml` only when local services are selected
  - functional `marreta.env` for local development
  - sanitized `marreta.env.example` with `change-me` placeholders for safe versioning
  - Redis local auth configured through `MARRETA_CACHE_PASSWORD`
- [x] README flow was simplified around the intended inner loop:
  - start selected local services with Compose
  - run the app on the host with `marreta serve`
  - open `/greetings`
  - run tests
  - stop services with `docker compose down`
- [x] Validation included generated basic, service-specific, and full local-service scaffolds plus scenario tests and endpoint checks.

### 2026-05-13 — v0.13p Feature Flags

- [x] `docs/spec/039_FEATURE_FLAGS.md` — approved and delivered process-wide boolean feature flags.
- [x] `src/feature_flags.rs` + config wiring — added strict parsing for `MARRETA_FEATURE_*` environment variables:
  - boolean-only values
  - missing flags evaluate to `false`
  - invalid names/values fail startup for `serve` and `test`
  - one immutable snapshot per process
- [x] `src/interpreter.rs` — added the minimal runtime surface:
  - `feature.enabled("flag_name")`
  - no `feature.disabled`, `feature.require`, dynamic values, remote sync, or runtime mutation
- [x] `src/doctor.rs` — added a dedicated `Feature Flags` section showing configured flags as normalized `enabled` / `disabled` states and reporting invalid config using human-readable guidance instead of raw regexes.
- [x] `examples/functional_tests/` — added route and scenario coverage for enabled and missing flags.
- [x] Validation at delivery:
  - `cargo test --lib` → **1376 passed**
  - `cargo test --test http_integration_tests` → **35 passed**
  - `cargo test --test integration_tests` → **76 passed**
  - `bash examples/functional_tests/test.sh` → **542 passed, 0 failed**

### 2026-05-12 — v0.13o Project Init

- [x] `docs/spec/038_PROJECT_INIT.md` — approved and delivered the first project initializer.
- [x] `marreta init <project-path>` — added filesystem-only project scaffolding with deterministic output and typed init errors.
- [x] Generated scaffold includes:
  - `app.marreta`
  - `schemas/greetings.marreta`
  - `tasks/greetings.marreta`
  - `routes/greetings.marreta`
  - `tests/greetings_test.marreta`
  - container-oriented deployment files from the initial 038 design
- [x] Project name validation was hardened so generated Docker image names cannot start with separators.
- [x] Functional validation confirmed:
  - generated project loads with `marreta doctor`
  - generated scenario tests pass with `marreta test`
  - generated container image can run and answer the greeting endpoint
- [x] Validation at delivery:
  - `cargo test --lib` → **1361 passed**
  - `cargo test --test integration_tests` → **76 passed**
  - `cargo test --bin marreta` → **3 passed**
  - `bash examples/init_functional/test.sh` → **12 checks passed**

### 2026-05-07 — v0.13n Runtime Event Log Contract

- [x] `docs/spec/037_RUNTIME_EVENT_LOG_CONTRACT.md` — approved and delivered the stable runtime JSON event contract for:
  - `kind: "app_log"`
  - `kind: "request"`
  - `kind: "consumer"`
  - `kind: "runtime_error"`
- [x] `src/interpreter.rs` — changed `log.*` output to always include `kind: "app_log"` and a uniform `data` field.
- [x] `src/server.rs` — added automatic consumer lifecycle events gated by `MARRETA_REQUEST_LOG`, mirroring request lifecycle logging for async work.
- [x] `src/server.rs` + runtime error boundaries — added compact `runtime_error` JSON event summaries while preserving Marreta-native stderr traces from specs 007/022/022b.
- [x] Follow-up `76b856c` — aligned edge cases:
  - schema rejection emits only `kind: "consumer"` with `status: "schema_rejected"`
  - string app logs use `data: "..."`, not a parallel `message` field
  - request-scoped runtime errors carry explicit `http_status`
- [x] Validation:
  - `cargo test --lib` → **1356 passed**
  - `bash examples/functional_tests/test.sh` → **538 passed, 0 failed**
  - `cargo check` → **passed**

### 2026-05-07 — v0.13m Async Trace Propagation

- [x] `docs/spec/036_ASYNC_TRACE_PROPAGATION.md` — approved and delivered W3C trace propagation across queue producers and consumers.
- [x] `src/queue/driver.rs` and queue drivers — introduced `QueueMessage { payload, metadata }` so trace metadata travels through transport metadata, not user payloads.
- [x] `src/interpreter.rs` — reused the 035 trace context outbound child path for `queue.push` and `queue.publish`.
- [x] `src/server.rs` — restored trace context inside `on queue` and `on topic` consumers, including orphan root creation when inbound metadata is missing or invalid.
- [x] `src/queue/rabbitmq.rs` — mapped trace metadata to AMQP headers and accepted string headers plus UTF-8 byte-array headers for interop with non-Marreta producers.
- [x] Functional validation covered:
  - HTTP request → queue producer → consumer log with same `trace_id`
  - consumer → outbound `http_client.*` with new child `span_id`
  - topic fan-out with one span per subscriber

### 2026-05-06 — v0.13l W3C Trace Context

- [x] `docs/spec/035_W3C_TRACE_CONTEXT.md` — delivered runtime-only W3C Trace Context support with no Marreta Lang code surface.
- [x] `src/trace_context.rs` — added W3C `traceparent` parsing/generation, trace ID/span ID generation, `tracestate` validation, and outbound child context handling.
- [x] `src/server.rs` — added HTTP middleware to accept or create trace context per request and attach trace fields to runtime request logs.
- [x] `src/interpreter.rs` — attached `trace_id` and `span_id` to `log.*` output when a request/consumer trace context is active.
- [x] `src/http_client/*` — propagated W3C trace headers through outbound `http_client.*` calls unless the user explicitly supplied trace headers.
- [x] Follow-up `53e0ff2` — made `tracestate` sanitization lenient per W3C by ignoring malformed members while preserving valid members.

### 2026-05-05 — v0.13k UUID Namespace

- [x] `docs/spec/034_UUID_NAMESPACE.md` — delivered the native UUID namespace with intentionally small RFC-vocabulary surface.
- [x] `Cargo.toml` — added the `uuid` dependency for RFC UUID generation.
- [x] `src/token.rs` + `src/parser.rs` + `src/value.rs` — added the native `uuid` namespace.
- [x] `src/interpreter.rs` — added:
  - `uuid.v4()`
  - `uuid.v7()`
- [x] Runtime contract:
  - canonical lowercase hyphenated UUID strings
  - no custom UUID runtime type
  - no seed/deterministic generation knobs
  - usable in normal expressions and API payloads

### 2026-05-02 — v0.13j Request Logging

- [x] `src/server.rs` — added runtime access logging middleware for `marreta serve`, emitting one JSON Lines event per handled HTTP request with:
  - `timestamp`
  - `kind: "request"`
  - `method`
  - `path`
  - `route` when resolvable
  - `status`
  - `duration_ms` as float
- [x] `src/server.rs` — added route resolution semantics for request logs:
  - resolved declared routes log `route`
  - unmatched `404` requests omit `route`
  - declared routes that return `4xx/5xx` preserve `route`
- [x] `src/main.rs` — added `MARRETA_REQUEST_LOG` handling for `marreta serve`, defaulting request logging to enabled for server runs
- [x] `tests/http_integration_tests.rs` — aligned server test fixtures with the new `ServerConfig` field and fixed two stale schema-name fixtures (`ItemPayload`) surfaced by the stricter validation run
- [x] `examples/functional_tests/marreta.env` + `examples/functional_tests/test.sh` — added end-to-end validation of:
  - matched request log output with `route`
  - declared-route `400` output with `route`
  - unmatched `404` output without `route`
- [x] Validation:
  - `cargo build` → **passed**
  - `cargo test --lib` → **1330 passed**
  - `cargo test --test http_integration_tests` → **35 passed**
  - `bash examples/functional_tests/test.sh` → **512 passed, 0 failed**

### 2026-05-01 — v0.13i Log Namespace

- [x] `src/token.rs` + `src/parser.rs` + `src/value.rs` — added the native `log` namespace as a reserved language surface, including parser recognition, runtime identity, and safe reuse as a bare map key.
- [x] `src/interpreter.rs` — added native logging semantics for:
  - `log.info(value)`
  - `log.warn(value)`
  - `log.error(value)`
  - `log.debug(value)`
- [x] `src/interpreter.rs` — added direct support for `log.*` inside:
  - standard calls
  - pipeline `>>`
  - parallel broadcast `*>>`
  without wrappers or user-level glue code.
- [x] `src/interpreter.rs` — implemented the first-cut runtime contract:
  - `MARRETA_LOG_LEVEL` filtering
  - JSON Lines emission to `stdout`
  - pass-through return semantics
  - `message` field for string input
  - `data` field for structured non-string input
  - inert handling of logged strings with no remote lookup or dynamic resolution
- [x] `examples/functional_tests/routes/log.marreta` + `examples/functional_tests/test.sh` — added functional validation of logging behavior across:
  - direct route-side logging
  - pipeline taps
  - broadcast branches
  - queue consumer logging
  - rescue on unsupported runtime values
  - stdout log assertions for `info`, `debug`, `warn`, and `error`
- [x] `examples/functional_tests/tests/log/*.marreta` — added native scenario coverage for:
  - request body
  - request headers
  - scenario-level verification of log-friendly request flows
- [x] `docs/vscode-marreta/` — updated the VS Code bundle to recognize the delivered `log` namespace and packaged a new `.vsix`
- [x] Validation:
  - `cargo test --lib` → **1325 passed**
  - `bash examples/functional_tests/test.sh` → **506 passed, 0 failed**

### 2026-05-01 — v0.13h Base64 Namespace

- [x] `Cargo.toml` — added the `base64` crate at the latest stable release used by the feature implementation.
- [x] `src/token.rs` + `src/parser.rs` + `src/value.rs` — added the native `base64` namespace as a reserved language surface, including parser recognition, runtime identity, and safe reuse as bare map keys.
- [x] `src/interpreter.rs` — added native Base64 semantics for:
  - `base64.encode(text)`
  - `base64.decode(text)`
  - `base64.encode(text, url_safe: true)`
  - `base64.decode(text, url_safe: true)`
- [x] `src/interpreter.rs` — added direct support for `base64.*` inside:
  - standard calls
  - pipeline `>>`
  without task wrappers or user-level glue code.
- [x] `src/interpreter.rs` — implemented the first-cut runtime contract:
  - standard Base64 by default
  - URL-safe mode via `url_safe: true`
  - permissive decode without required padding
  - strict alphabet matching per mode
  - explicit runtime failure on malformed input
  - explicit runtime failure when decoded bytes are not valid UTF-8
- [x] `examples/functional_tests/routes/base64.marreta` + `examples/functional_tests/test.sh` — added functional validation of Base64 behavior across:
  - Basic-style header construction
  - request-header decoding
  - URL-safe round-trip
  - decode without padding
  - rescue on invalid input
  - filesystem round-trip
  - cache round-trip
  - queue round-trip
  - HTTP client round-trip
- [x] `examples/functional_tests/tests/base64/*.marreta` — added native scenario coverage for:
  - request headers
  - request body
- [x] `docs/vscode-marreta/` — updated the VS Code bundle to recognize the delivered `base64` namespace and packaged a new `.vsix`
- [x] Validation:
  - `cargo test --lib` → **1314 passed**
  - `bash examples/functional_tests/test.sh` → **489 passed, 0 failed**

### 2026-04-30 — v0.13g JSON Namespace

- [x] `Cargo.toml` — added `serde_json` with `preserve_order` and `indexmap` so native map-backed JSON parsing/serialization can preserve insertion order deterministically.
- [x] `src/token.rs` + `src/parser.rs` + `src/value.rs` — added the native `json` namespace as a reserved language surface, introduced `Value::JsonNamespace`, and migrated runtime map storage to `ValueMap`/`IndexMap` so parse/stringify preserve stable key order across runtime boundaries.
- [x] `src/interpreter.rs` — added native JSON semantics for:
  - `json.parse(text)`
  - `json.stringify(value)`
  - `json.pretty(value)`
- [x] `src/interpreter.rs` — added direct support for `json.*` inside:
  - standard calls
  - pipeline `>>`
  without task wrappers or special-case user code.
- [x] `src/interpreter.rs` + `src/value.rs` — implemented strict JSON serialization rules for:
  - native primitives
  - lists
  - maps
  - relational records
  - temporal values (`instant`, `date`, `time`, `duration`, `interval`)
  while explicitly rejecting unsupported runtime values such as namespaces, tasks, and query builders.
- [x] `src/main.rs` + supporting runtime/doc/server/scenario files — completed the `ValueMap` migration so insertion-order semantics hold across CLI/server/runtime entrypoints, BSON/doc transport, response serialization, scenario mocks, and DB row coercion.
- [x] `examples/functional_tests/routes/json.marreta` + `examples/functional_tests/test.sh` — added functional validation of JSON behavior across:
  - raw-body parsing
  - rescue on malformed JSON
  - compact and pretty serialization
  - filesystem round-trip
  - cache round-trip
  - queue round-trip
  - HTTP client round-trip
- [x] `examples/functional_tests/tests/json/raw_parse_test.marreta` + `examples/functional_tests/tests/http_client/verbs_test.marreta` — expanded native scenario coverage so `.marreta` tests now exercise:
  - POST with body
  - GET with path params
  - PUT with body + path
  - PATCH with body + path
  - DELETE
  - request headers
  - query params
  - raw JSON parsing through the new namespace
- [x] `docs/vscode-marreta/` — updated the VS Code bundle to recognize the delivered `json` namespace and bumped the extension package to `0.2.10`
- [x] Validation:
  - `cargo test --lib` → **1305 passed**
  - `bash examples/functional_tests/test.sh` → **476 passed, 0 failed**

### 2026-04-30 — v0.13f Filesystem Namespace

- [x] `src/token.rs` + `src/parser.rs` + `src/value.rs` — added the native `fs` namespace as a reserved language surface, including runtime identity support and parser recognition for namespaced filesystem calls while preserving bare `fs:` map keys.
- [x] `src/interpreter.rs` — added native filesystem semantics for:
  - `fs.read(path)`
  - `fs.write(path, value)`
  - `fs.append(path, value)`
  - `fs.exists(path)`
  - `fs.delete(path)`
- [x] `src/interpreter.rs` — kept the filesystem surface text-first and UTF-8-only:
  - `fs.read(...) -> string`
  - `fs.write(...) -> written string`
  - `fs.append(...) -> appended string`
  - `fs.exists(...) -> boolean`
  - `fs.delete(...) -> boolean`
  - invalid UTF-8 on read fails explicitly
  - missing file on delete returns `false`
  - no trimming, newline normalization, or implicit directory creation
- [x] `src/interpreter.rs` — added direct support for `fs.*` inside:
  - standard calls
  - pipeline `>>`
  without requiring task wrappers or special-case grammar
- [x] `examples/functional_tests/routes/filesystem.marreta` + `examples/functional_tests/test.sh` — added functional validation of filesystem behavior across:
  - write + read round-trip
  - append
  - exists
  - idempotent delete
  - pipeline pass-through
  - `if/else` integration
  - missing-file read error
  - string-only write rejection
  - `rescue` capture of filesystem I/O errors
- [x] `docs/vscode-marreta/` — refreshed the VS Code bundle to recognize the delivered `fs` namespace and packaged `marretalang-0.2.9.vsix`
- [x] Validation:
  - `cargo test --lib test_fs_ -- --nocapture` → **7 passed**
  - `cargo test --lib` → **1296 passed**
  - `bash examples/functional_tests/test.sh` → **466 passed, 0 failed**

### 2026-04-29 — v0.13e Math Namespace

- [x] `src/token.rs` + `src/parser.rs` + `src/value.rs` — added the native `math` namespace as a reserved language surface, with runtime identity support and parser recognition for namespaced math calls
- [x] `src/interpreter.rs` — added native math semantics for:
  - `math.abs(...)`
  - `math.floor(...)`
  - `math.ceil(...)`
  - `math.round(...)`
  - `math.round(..., places: n)`
  - `math.min(...)`
  - `math.max(...)`
  - `math.clamp(..., min:, max:)`
- [x] `src/interpreter.rs` — added direct support for `math.*` inside:
  - standard calls
  - pipeline `>>`
  - parallel broadcast `*>>`
  without requiring task wrappers
- [x] `examples/functional_tests/` — expanded functional validation of `math` across:
  - core/runtime routes
  - contracts
  - cache
  - HTTP client
  - iteration/reduce
  - parallel broadcast
  - queue
  - auth
- [x] `examples/functional_tests/routes/core.marreta` — adjusted one legacy map key from `math:` to `equation:` after `math` became a reserved namespace token
- [x] Validation:
  - `cargo test --lib interpreter::tests::test_math_ -- --nocapture` → **17 passed**
  - `cargo test --lib` → **1289 passed**
  - `bash examples/functional_tests/test.sh` → **456 passed, 0 failed**

### 2026-04-28 — v0.13d Time API

- [x] `src/ast.rs` + `src/token.rs` + `src/parser.rs` — added schema/runtime language support for `instant`, `date`, `time`, `duration`, and `interval`, plus the `time` namespace and `value.on(date)` method parsing.
- [x] `src/value.rs` + `src/interpreter.rs` — added native runtime values and semantics for:
  - `time.now()`
  - `time.today()`
  - `time.parse(...)`
  - `time.date(...)`
  - `time.at(...)`
  - `time.instant(...)`
  - `time.days(...)`, `time.hours(...)`, `time.minutes(...)`, `time.seconds(...)`
  - `time.interval(...)`
  - `time.contains(...)`
  - `time.overlaps(...)`
  - `time.format(...)`
  - `time.from_unix(...)`
  - `time.unix(...)`
- [x] `src/interpreter.rs` + `chrono-tz` — added timezone-aware local behavior using `MARRETA_TIMEZONE` for:
  - `time.today()`
  - `instant.year/month/day/hour/minute/second/weekday/date/time`
  - `date.start_of_day`
  - `date.end_of_day`
  - `time.on(date)`
- [x] `src/validator.rs` + `src/server.rs` — added schema coercion for temporal payloads in route bindings and task arguments, including interval validation and type compatibility checks.
- [x] `src/db/postgres.rs` + `src/interpreter.rs` + `src/migrations.rs` — added relational support for temporal fields:
  - `instant -> TIMESTAMPTZ`
  - `date -> DATE`
  - `time -> TIME`
  - `duration -> BIGINT`
  - `interval -> JSONB`
  - DB row retyping back into native temporal values for persistent schemas
- [x] `src/doc/bson.rs` — added BSON transport rules for temporal values
- [x] `src/openapi.rs` — added OpenAPI schema mapping for temporal fields
- [x] `examples/functional_tests/` — expanded functional validation of `time` across:
  - runtime/core routes
  - contracts
  - cache
  - doc
  - db
  - HTTP client
  - iteration
  - parallel
  - queue
  - auth
- [x] `examples/functional_tests/migrations/20260428_145249_create_time_entries.*.sql` — generated via CLI to validate temporal persistence through the actual migration workflow
- [x] Validation:
  - `cargo test --lib test_time_ -- --nocapture` → **13 passed**
  - `cargo test --lib openapi::tests::test_temporal_schema_types_map_to_openapi_shapes -- --nocapture` → **1 passed**
  - `cargo test --lib migrations::tests::test_schema_type_to_sql_maps_temporal_types -- --nocapture` → **1 passed**
  - `cargo test --lib doc::bson::tests::test_value_to_bson_temporal_values -- --nocapture` → **1 passed**
  - `cargo test --lib interpreter::tests::test_db_row_to_runtime_value_coerces_temporal_schema_fields -- --nocapture` → **1 passed**
  - `cargo test --lib interpreter::tests::test_time_properties_respect_configured_timezone -- --nocapture` → **1 passed**
  - `cargo test --lib` → **1272 passed**
  - `bash examples/functional_tests/test.sh` → **443 passed, 0 failed**
- [x] `examples/omni_hub/` — refreshed the example to match the delivered language surface:
  - replaced cache read-through workaround with native `if/else`
  - replaced synthetic string timestamps with `time.now()` and `instant` fields
  - generated a follow-up migration for `orders.created_at` / `orders.completed_at`
  - updated the consolidated implementation review to mark `if/else` and `time` as resolved
- [x] Validation:
  - `bash examples/omni_hub/test.sh` → **20 passed, 0 failed**
  - `bash examples/omni_hub/test_migrations.sh` → **PASS**
- [x] `docs/vscode-marreta/` — improved the VS Code bundle to better match the current language:
  - stronger TextMate highlighting for `auth`, `scenario`, `on queue`, `on topic`, `time`, and temporal types
  - better indentation rules for block-oriented syntax
  - added a bundle README
  - packaged `marretalang-0.2.7.vsix`

### 2026-04-27 — v0.13c If/Else Blocks

- [x] `src/ast.rs` + `src/parser.rs` — added block `if/else` expressions, `else if` chaining, and parser precedence so pipelines aligned after an `if` consume the result of the full conditional expression.
- [x] `src/interpreter.rs` — added runtime evaluation for `if/else`, guaranteed `null` when `else` is omitted, preserved `reply` / `fail` early-return semantics inside branches, and isolated branch scope so neither new bindings nor reassignment of outer bindings leak across the branch boundary.
- [x] `src/route_loader.rs` + `src/doctor.rs` — taught project analysis walkers to traverse `if` expressions and their branch bodies.
- [x] `examples/functional_tests/routes/core.marreta` + `examples/functional_tests/test.sh` — added real route-level coverage for:
  - branch selection
  - `else if`
  - `if` without `else` returning `null`
  - branch-local scope isolation
  - `reply` early return from inside a branch
  - pipeline application to the full `if` result
- [x] Validation:
  - `cargo test --lib if_expression -- --nocapture` → **passed**
  - `cargo test --lib test_if_expression_branch_scope_does_not -- --nocapture` → **2 passed**
  - `cargo test --lib` → **passed**
  - `bash examples/functional_tests/test.sh` → **414 passed, 0 failed**

### 2026-04-27 — v0.13b Persistence by Convention + Omni Hub validation

- [x] `examples/omni_hub/` — added a new integrated functional project exercising `db`, `doc`, `cache`, `queue`, and migrations together through a realistic service-order flow:
  - `POST /customers`
  - `POST /orders`
  - `GET /orders/:id` with read-through cache
  - `PATCH /orders/:id/complete` with audit snapshot + billing queue
  - `GET /audits/orders/:id`
- [x] `examples/omni_hub/test.sh` — added end-to-end validation against real Postgres, MongoDB, Redis, and RabbitMQ with direct infra inspection for:
  - relational persistence
  - topic delivery
  - cache hit/miss and invalidation
  - audit snapshot creation and immutability
  - queue message retention
- [x] `src/queue/rabbitmq.rs` — fixed `queue.push` so point-to-point publish declares the named durable queue before publishing; `examples/omni_hub` no longer predeclares `process_billing` manually
- [x] `src/main.rs` + `src/migrations.rs` — made `marreta migrate diff` and `marreta migrate generate` source-first by reconstructing the current schema from local migration files instead of introspecting a live database
- [x] `src/db/driver.rs` + `src/db/postgres.rs` + `src/interpreter.rs` + `src/scenario_tests.rs` — changed `db.update(id, data)` to return `null` when the target row does not exist; aligned the runtime, Postgres driver, interpreter, and scenario/mock layer to the same semantics
- [x] `examples/omni_hub/routes/customers.marreta` — switched a real route to depend on `db.update(...) -> null -> require -> 404`, proving the new behavior end-to-end
- [x] `examples/omni_hub/IMPLEMENTATION_REVIEW.md` — consolidated findings from the functional implementation, separating fixed bugs from still-missing language features
- [x] Validation:
  - `cargo test --lib` → **1236 passed**
  - `cargo test --lib migrations::` → **15 passed**
  - `cargo test --lib interpreter::` → **436 passed**
  - `cargo test --lib scenario_tests::` → **21 passed**
  - `cargo test --lib db::` → **65 passed**
  - `bash examples/omni_hub/test.sh` → **19 passed, 0 failed**
  - Manual `marreta migrate diff` in `examples/omni_hub` with DB offline → **passed**
  - Manual `marreta migrate generate` in a temporary Omni Hub copy without `migrations/` and with DB offline → **passed**

### 2026-04-18 — v0.13 Security

- [x] `src/ast.rs` + `src/parser.rs` + `src/route_loader.rs` + `src/file_loader.rs` — added project-wide `auth jwt` / `auth api_key` providers, route-level `require auth <provider>` and `allow <expr>`, canonical auth clause ordering, unknown-provider validation, `allow` without auth rejection, and public-route `auth` access rejection.
- [x] `src/auth.rs` — added runtime auth registry/config validation for JWT and API key providers, including env-backed fields, JWT validation source exclusivity, algorithm/source compatibility, API key secret-source validation, and safe defaults for claim mappings/cache/skew.
- [x] `src/server.rs` — added auth runtime execution for protected routes, sanitized lowercase `401` / `403` responses, automatic `auth` context injection, constant-time API key comparison, Argon2id and explicit SHA-256 API key hash verification, HMAC/public-key JWT validation, OIDC discovery, explicit JWKS fetch/cache, `kid` key selection, `nbf` validation, and algorithm-confusion protection.
- [x] `src/auth.rs` — added auth config validation for API key header names, `secret_hash` formats, explicit `jwks_url` syntax, fixed public key PEM parsing, and file-backed public keys via `public_key_pem_file`.
- [x] `src/openapi.rs` — added OpenAPI security schemes for JWT bearer and header API key providers, plus route-level security requirements for protected routes.
- [x] `src/doctor.rs` — added auth provider config reporting and validation through `marreta doctor`, including JWT validation source details, readable `public_key_pem_file` reporting, and redacted secret/hash reporting.
- [x] `src/main.rs` — loads `marreta.env` values before project loading while preserving process/CI environment overrides, so auth provider fields can safely use `env.MARRETA_AUTH_*`.
- [x] `src/scenario_tests.rs` — added `given auth.<provider>` to short-circuit token validation at the provider boundary while preserving real `require auth` and `allow` enforcement.
- [x] `examples/functional_tests/routes/auth.marreta` — added API key routes plus JWT provider-shape examples for issuer-derived OIDC discovery, explicit JWKS, fixed public key file, and fixed HMAC.
- [x] `examples/functional_tests/tests/auth/` — added API scenario tests for protected JWT routes using auth provider mocks.
- [x] `tests/fixtures/auth/` — added local RSA, EC, HMAC, JWKS, and OIDC discovery fixtures used by deterministic auth tests; no real IdP or external network dependency is required.
- [x] Validation:
  - `cargo build` → passed
  - `cargo test --lib auth::tests -- --test-threads=1` → **14 passed**
  - `cargo test --lib -- --test-threads=1` → **1225 passed**
  - `cargo test --bin marreta` → **3 passed**
  - `examples/functional_tests/test.sh` → **373 passed, 0 failed**
  - Manual `marreta doctor` in `examples/functional_tests` → **passed**
  - Manual `marreta test --filter "fixed public key"` in `examples/functional_tests` → **1 passed, 0 failed**

### 2026-04-17 — v0.12g API Scenario Testing

- [x] `src/token.rs` + `src/parser.rs` + `src/ast.rs` — added parser support for REST-first API scenarios using `scenario`, `given`, `when`, `then`, `returns`, status assertions, partial response assertions, request body/header blocks, and the `anything` matcher
- [x] `src/scenario_tests.rs` — added scenario file discovery under `tests/**/*_test.marreta`, duplicate-name validation per file, in-memory route execution, strict `given` consumption, partial response matching, and scenario-backed mocks for `db`, `doc`, `cache`, `queue`, and `http_client`
- [x] `src/main.rs` + `src/server.rs` — added `marreta test [path] [--list] [--filter text] [--coverage]` and exposed route execution internally so tests call the same runtime path as production routes without opening an HTTP port
- [x] `src/doctor.rs` + `src/interpreter.rs` — made scenario declarations visible to project analysis while keeping them no-op in normal runtime execution
- [x] `examples/functional_tests/tests/` — added scenario files covering real route execution with mocked `db`, `doc`, `cache`, `queue`, and `http_client` drivers, organized by production surface
- [x] `examples/functional_tests/test.sh` — added functional validation for test discovery, filtering, API coverage, explicit non-convention files, elapsed-time output, and unused-given diagnostics
- [x] Review hardening — scenario DSL words are contextual identifiers outside scenarios, loose statements inside scenarios are rejected, a `given` can match multiple calls, duplicate `given` declarations fail clearly, `returns anything` is rejected with a matcher-specific message, and `db.native_query(...)` uses the production call shape in scenarios
- [x] Validation:
  - `cargo test --lib` → **1172 passed**
  - `cargo test --bin marreta` → **3 passed**
  - `examples/functional_tests/test.sh` → **369 passed, 0 failed**
  - Manual `marreta test` in `examples/functional_tests` → **6 passed, 0 failed**

### 2026-04-18 — v0.12h API Scenario Testing Hardening

- [x] `src/scenario_tests.rs` — changed `given` lookup to most-specific-wins matching while preserving declaration-order ties; exact values now outrank `anything`
- [x] `src/scenario_tests.rs` — added a fake scenario `DbTx` so routes using `db.transaction` execute against given-backed DB mocks without opening a real database connection
- [x] `src/scenario_tests.rs` — expanded focused scenario tests from 9 to 18, covering matcher specificity, transaction execution, route matching, first failing `then`, nested `anything`, filter selection, request bindings, non-string headers, computed status, native query shape, duplicate `given`, and unused `given` diagnostics
- [x] `docs/spec/023_TESTING_DSL.md` + `docs/spec/023b_API_SCENARIO_TESTING_HARDENING.md` — documented the delivered 023b behavior and remaining deferred work
- [x] Validation:
  - `cargo test --lib scenario_tests::tests` → **18 passed**
  - `cargo test --lib` → **1181 passed**
  - `cargo test --bin marreta` → **3 passed**
  - `examples/functional_tests/test.sh` → **369 passed, 0 failed**

### 2026-04-17 — v0.15b Trace Performance and Ergonomics

- [x] `src/interpreter.rs` — replaced stringly trace labels with structured `FrameLabel` variants, added borrowed trace operation labels, introduced scoped trace guards (`enter_route`, `enter_task`, `enter_consumer`), removed the public manual push/pop/clear trace API, and added debug assertions for guard/frame kind consistency
- [x] `src/server.rs` — migrated route and consumer execution to scoped trace guards; added dependency-free TTY-only ANSI coloring for uncaught runtime error codes while keeping piped/CI stderr plain
- [x] `src/db/postgres.rs` — normalized table-less DB operation labels from `db.unknown.query` to `db.query`
- [x] `examples/functional_tests/test.sh` — updated trace assertions for `db.query` and preserved the success-path trace silence check
- [x] `tests/load/Dockerfile` — aligned the runtime image to `debian:trixie-slim` so release binaries built from the current Rust image run correctly in load-test containers
- [x] `docs/spec/022b_TRACE_PERF_AND_ERGONOMICS.md` + `docs/spec/SPEC.md` — updated the 022b delivery notes and documented the single-taxonomy convention for future specs
- [x] `docs/performance/LOAD_TEST_TRACE_022B_20260417.md` — recorded the ecommerce load-test run and compared it with the previous DB load-test run
- [x] Validation:
  - `cargo test --lib` → **1155 passed**
  - `cargo test --bin marreta` → **3 passed**
  - `examples/functional_tests/test.sh --docker` → **363 passed, 0 failed**
  - `tests/load/run.sh` → **PASS** (`3,316,314` requests, `4,806 req/s`, p95: `health=0.843ms`, `products=3.297ms`, `orders=4.998ms`)

### 2026-04-12 — v0.14b Project Doctor Command

- [x] `src/doctor.rs` + `src/lib.rs` — added the doctor module with project-intent discovery from the loaded AST/runtime model, intent-aware structured config validation, optional `--connect` live checks for `db`, `doc`, `cache`, and `queue`, and read-only migration summary support
- [x] `src/main.rs` — added `marreta doctor [--connect] [app.marreta]` to the CLI and help output; project resolution follows the same `./app.marreta` convention as other project commands
- [x] `examples/migrations_functional/test.sh` — extended the functional suite to cover `marreta doctor` and `marreta doctor --connect`, including migration-state summary after apply
- [x] `docs/spec/021_DOCTOR_COMMAND.md` — tightened the implementation draft around `--connect`, exit codes, read-only semantics, and intent discovery from the loaded project model
- [x] Validation:
  - `cargo test --lib` → **1143 passed**
  - `cargo test --bin marreta` → **0 failed**
  - `examples/migrations_functional/test.sh` → **PASS**
  - `examples/functional_tests/test.sh --docker` → **352 passed, 0 failed**

### 2026-04-12 — v0.14 Secret-Aware Infrastructure Config

- [x] `src/config.rs` — introduced structured runtime config objects for `db`, `doc`, `cache`, and `queue`; env loading now resolves host/port/name/user/password-style variables instead of URL-only project config; provider-specific advanced options are modeled explicitly (`db.ssl_mode`, `doc.auth_source`, `cache.user/db`, `queue.vhost`)
- [x] `src/db/mod.rs` + `src/doc/mongodb.rs` + `src/cache/mod.rs` + `src/queue/mod.rs` + `src/queue/rabbitmq.rs` — runtime engines now consume centralized structured config from `MarretaConfig`; direct `std::env` reads removed from cache and queue layers; driver URLs are now derived internally from structured config where needed
- [x] `src/main.rs` + `src/interpreter.rs` — startup/runtime messages and missing-config errors updated to reference structured env vars instead of `MARRETA_*_URL`
- [x] `examples/migrations_functional/`, `examples/functional_tests/`, `examples/ecommerce/`, and `tests/load/docker-compose.yml` — moved committed examples and test fixtures to structured env vars; `examples/functional_tests` now validates Redis auth via `MARRETA_CACHE_PASSWORD` and Mongo auth via `MARRETA_DOC_USER` / `MARRETA_DOC_PASSWORD` / `MARRETA_DOC_AUTH_SOURCE`
- [x] `docs/spec/SPEC.md` + `docs/spec/013_QUEUE.md` + `docs/spec/014_CACHE.md` + `docs/spec/020_SECRET_AWARE_CONFIG.md` — living docs updated to teach the structured provider model; `docs/spec/021_DOCTOR_COMMAND.md` added as the next-step draft for project-aware config/connectivity checks
- [x] Validation:
  - `cargo test --lib` → **1136 passed**
  - `cargo test --test http_integration_tests` → **35 passed**
  - `examples/migrations_functional/test.sh` → **PASS**
  - `examples/functional_tests/test.sh --docker` → **352 passed, 0 failed**

### 2026-04-11 — v0.13b Project Metadata Unification

- [x] `src/server.rs` — OpenAPI and built-in `/_health` now source metadata from `project_name` / `project_version`; `marreta serve` startup output now prints explicit application metadata separately from the Marreta runtime version
- [x] `src/file_loader.rs` + `tests/http_integration_tests.rs` — committed project fixtures migrated away from `api_name` / `api_version`; project-level metadata assertions now use `project_name` / `project_version`
- [x] `examples/functional_tests/`, `examples/ecommerce/`, `examples/snippets/` — current project examples and snippets updated to use unified `project_*` metadata
- [x] `docs/spec/019b_PROJECT_METADATA_UNIFICATION.md` — implementation draft added; current docs/spec snippets updated where `api_*` still appeared as active project metadata
- [x] Validation:
  - `cargo test --lib` → **1160 passed**
  - `cargo test --test http_integration_tests` → **35 passed**
  - `examples/migrations_functional/test.sh` → **PASS**

### 2026-04-11 — v0.13 Project Entrypoint Convention

- [x] `src/main.rs` — project commands (`serve`, `migrate diff|generate|status|list|discard|apply|rollback`) now resolve `./app.marreta` by convention when invoked from a project root; explicit entrypoint paths remain supported as an override
- [x] `src/file_loader.rs` — project entrypoints now require `project_name` and `project_version` as top-level string assignments
- [x] `examples/functional_tests/`, `examples/migrations_functional/`, `examples/ecommerce/` — example projects updated with required project metadata; runtime/test helpers updated to use the pathless project-command workflow
- [x] Validation:
  - `cargo test --lib` → **1160 passed**
  - `cargo test --test http_integration_tests` → **35 passed**
  - `examples/functional_tests/test.sh --docker` → **352 passed, 0 failed**
  - `examples/migrations_functional/test.sh` → **PASS**

### 2026-04-11 — v0.12b Migration Hygiene

- [x] `src/migrations.rs` — added first-class migration inventory/state modeling with `MigrationState`, `MigrationListEntry`, shared inventory classification, and `discard_pending_migration` for local pending-only deletion with pair integrity checks
- [x] `src/main.rs` — added `marreta migrate list <file>`, `marreta migrate explain [state]`, and `marreta migrate discard <version> <file>`; enriched `status` with suggested actions; centralized migration state loading for list/status/discard flows
- [x] `src/config.rs` — project-root config loading no longer mutates the process environment during `serve`/migration commands; project env vars are now merged deterministically with process env overrides
- [x] `examples/migrations_functional/test.sh` — extended functional validation to cover `list`, `explain`, `discard`, `changed`, and `missing_local`, while running against an isolated temporary project workspace so committed example files remain unchanged
- [x] `docs/spec/018b_MIGRATION_HYGIENE.md` — implementation-ready spec for migration hygiene, state-machine guidance, CLI help model, and validation plan
- [x] `docs/spec/019_PROJECT_ENTRYPOINT.md` — restored draft for the next project-entrypoint convention work, preserved alongside `018b`
- [x] Validation:
  - `cargo test --lib` → **1160 passed**
  - `cargo test --bin marreta` → **0 failed**
  - `examples/migrations_functional/test.sh` → **PASS**

### 2026-04-10 — v0.12 DB Migrations

- [x] `src/ast.rs` + `src/token.rs` + `src/lexer.rs` + `src/parser.rs` — persistent schema syntax implemented: `db: <table>`, `timestamp`, and field annotations `@primary`, `@generated`, `@unique`, `@default(...)`
- [x] `src/route_loader.rs` + `src/file_loader.rs` + `src/persistent_schema.rs` — `persistent_schemas` registry added; private persistent schemas preserved for migrations; persistent -> non-persistent references rejected with descriptive errors
- [x] `src/migrations.rs` — relational model, Postgres diff planner, SQL renderers for `up` and `down`, migration file discovery/writing, checksum generation, and status classification (`applied`, `pending`, `changed`, `missing_local`)
- [x] `src/db/postgres.rs` + `src/db/mod.rs` — Postgres introspection, `_marreta_migrations` management, apply/rollback helpers, and applied migration listing
- [x] `src/main.rs` + `src/config.rs` — `marreta migrate diff <file>`, `generate <file>`, `status <file>`, `apply <file>`, `rollback <file>` wired into the CLI, with config loaded from `<project-root>/marreta.env` and process env override support; `serve` now follows the same project-root config resolution
- [x] `Cargo.toml` — `sha2 = "0.10.9"` added for migration checksums
- [x] `examples/migrations_functional/` — dedicated functional validation suite for migrations: realistic project layout (`app.marreta`, `schemas/`, `routes/`, `marreta.env`, `migrations/`), isolated Postgres + compose-managed `marreta` service, real CLI flow (`diff/generate/status/apply/rollback`), drift detection, and runtime `db.*` verification after apply
- [x] Validation:
  - `cargo test --lib` → **1156 passed**
  - `cargo test --test integration_tests` → **73 passed**
  - `cargo test --test http_integration_tests` → **35 passed**
  - `cargo test --bin marreta` → **0 failed**
  - `examples/functional_tests/test.sh --docker` → **352 passed, 0 failed**
  - `examples/migrations_functional/test.sh` → **PASS**

### 2026-04-10 — v0.11 File Encapsulation & Module Runtime

- [x] `src/file_loader.rs` — replaced the old “export survives, private discards” multi-file loader with module-aware bootstrapping: `LoadedProject`, `ProjectRuntime`, `ModuleRuntime`, per-file `module_id`, persistent private env per file, final global-public propagation back into every module
- [x] `src/route_loader.rs` — `RouteDefinition` and `ConsumerDefinition` now carry stable `module_id` runtime identity alongside `source_file`
- [x] `src/interpreter.rs` + `src/value.rs` — `Value::Task` now carries `owner_module`; task calls resolve schemas in module context and exported tasks execute with access to private helpers/constants from their defining file
- [x] `src/server.rs` + `src/main.rs` — server boot now consumes `LoadedProject.runtime`; routes/consumers execute from module-scoped environments instead of a flattened global env
- [x] `tests/http_integration_tests.rs` — added coverage for same-file private task access, cross-file private task rejection, exported task using private helper, and same-file private schema validation
- [x] `examples/functional_tests/routes/iteration.marreta` — removed unnecessary `export` from recursive tasks and added a same-file private schema/private variable route
- [x] `examples/functional_tests/docker-compose.yml` — fixed `app` healthcheck to use `curl` against `/_health`
- [x] `examples/functional_tests/test.sh` — added functional assertions for the new private schema route
- [x] Validation:
  - `cargo test --lib` → **1131 passed**
  - `cargo test --test integration_tests` → **73 passed**
  - `cargo test --test http_integration_tests` → **35 passed**
  - `examples/functional_tests/test.sh --docker` → **352 passed, 0 failed**

### 2026-04-07 — v0.10 HTTP Client Module — Phases 1–5

- [x] **Phase 1 — Tokens, Value & Error Scaffolding**: `TokenKind::HttpClient`; `Value::HttpClientNamespace`; `MarretaError::HttpClientError` + `ErrorCode::HttpClientError` in `src/error.rs`
- [x] **Phase 2 — HttpClient Driver**: `src/http_client/driver.rs` — `HttpClient` trait, `HttpRequest`, `HttpResponse`, `HttpMethod`, `HttpClientDriverError`, `MockHttpClient`; `src/http_client/reqwest.rs` — `ReqwestDriver` with shared `reqwest::Client`, JSON auto-detect, lowercase headers; `src/http_client/mod.rs` — `HttpClientConfig::from_env()`, `HttpClientEngine`; `reqwest` moved to `[dependencies]`
- [x] **Phase 3 — Interpreter Integration**: `dispatch_http_client()` for all 5 verbs; named param extraction (`headers:`, `query:`, `timeout:`); pipeline injection in `apply_pipeline_value()` (body for POST/PUT/PATCH, query for GET/DELETE); response converted to `Value::Map { status, body, headers }`
- [x] **Phase 4 — Server Wiring**: `http_client_driver` field on `ServerConfig`; threaded through serve/register_route/execute_route/start_consumers; always initialized (no conditional unlike db/cache)
- [x] **Phase 5 — Functional Tests & Docs**: `routes/http_client.marreta` (Section 30, 30 routes — stubs + callers); `test.sh` Section 30 (25 test cases); `patch()` helper added to test.sh; `docs/spec/SPEC.md` §9 HTTP Client added; roadmap entry updated from `http.get` to `http_client.get`; `docs/spec/015_HTTP_CLIENT.md` marked ✅ Complete; `CHANGELOG.md` updated
- [x] Functional test coverage: all 5 verbs, `headers:`, `query:`, `timeout:`, response envelope (`.status`, `.body`, `.headers`), `match response.status`, rescue on connection failure, fire-and-forget, pipeline input (POST/PUT/PATCH/GET), pipeline output to cache (read-through pattern), pipeline map, multi-step chain — **334/334 functional tests passing**
- [x] `cargo test --lib`: **1114 passed** / 0 failed

### 2026-04-06 — v0.9 Cache Module — Phases 1–5

- [x] **Phase 1 — AST & Tokens**: `TokenKind::Cache`; `Value::CacheNamespace`; `as_integer()` helper on `Value`; no new AST nodes (reuses `MethodCall` dispatch like db/doc)
- [x] **Phase 2 — Cache Driver**: `src/cache/driver.rs` — `CacheDriver` trait (10 async operations), `CacheDriverError`, `MockCacheDriver`; `src/cache/redis.rs` — `RedisDriver` with `ConnectionManager`, transparent key prefixing, per-op timeout, JSON ser/de; `src/cache/mod.rs` — `CacheConfig::from_env()`, `CacheEngine::from_env()`; `Cargo.toml` — `redis = { version = "1.2", features = ["tokio-comp", "connection-manager"] }`
- [x] **Phase 3 — Interpreter Integration**: `dispatch_cache()` handles all 10 operations with named param extraction (`ttl:`, `only_if_absent:`, `by:`); `resolve_ttl()` respects `MARRETA_CACHE_DEFAULT_TTL`; `require_cache_driver()` fail-fast; 16 new unit tests
- [x] **Phase 4 — Server Wiring & Health**: `cache_engine` field on `ServerConfig`; `/_health` gains `cache` field; `main.rs` initialization; threaded driver+config through serve/register_route/execute_route/start_consumers
- [x] **Phase 5 — Functional Tests & Pipeline Fix**: `routes/cache.marreta` (15 routes); `test.sh` Section 29 (18 test cases); Redis 7 in docker-compose (port 6380); fixed parser bug where `PREC_CALL` prevented dot-access in pipeline stages (`x >> cache.set("k")` was parsed as `MethodCall(Pipeline(x, cache), set)` instead of `Pipeline(x, MethodCall(cache, set))`) — changed stage precedence to `PREC_CALL - 1`
- [x] **Queue Pipeline Support**: `queue.push`/`queue.publish` return value instead of null; payload optional in pipeline context; `eval_queue_push`/`eval_queue_publish` helpers; parser tests for pipeline-without-payload; `docs/spec/013_QUEUE.md` updated
- [x] `docs/spec/SPEC.md` — §8 completely rewritten with cache syntax, schema contracts, conventions; §7.3 queue pipeline support added; env vars expanded
- [x] `docs/spec/014_CACHE.md` — full 5-phase implementation plan
- [x] `cargo test --lib`: **1100 passed** / 0 failed

### 2026-04-05 — v0.8 Queue Module — Hardening & `require ... else nack`

- [x] `src/queue/rabbitmq.rs` — eliminated shared producer channel; every `push`/`publish` now creates a fresh lapin channel, publishes, and explicitly closes it. Fixes recurring `invalid channel state: Error (basic.publish)` caused by lapin 4 `ChannelState` not being safely composable across async tasks when the dropped `ConfirmationFuture` corrupts shared channel state. Consumer channels remain long-lived and owned by their spawn task, closed on exit.
- [x] `src/ast.rs` + `src/parser.rs` + `src/interpreter.rs` — implemented `require EXPR else nack [requeue]` (spec 013 §ack/nack examples): `Statement::Nack` gains optional `condition: Option<Expression>`; parser recognizes the new `else nack` branch in `parse_require`, wrapping the guard as `NOT(cond)` so the nack fires only when the requirement is falsy. Mirrors the existing `require X else raise` pattern. 2 new parser unit tests (`test_require_else_nack`, `test_require_else_nack_requeue`).
- [x] `examples/functional_tests/routes/queue.marreta` — added `/queue/push-typed-invalid` and `/queue/publish-topic-invalid` producer routes exercising consumer-side schema mismatch (producer schema A, consumer schema B → consumer nacks without requeue, server stays up); `ft.rejected` / `ft.requeue` consumers rewritten to use the new guarded `require ... else nack [requeue]` form.
- [x] `examples/functional_tests/test.sh` — 2 new Section 28 test cases for consumer-side schema mismatch; defensive cleanup of stale `marreta serve` processes and app containers before startup to prevent zombie-process port conflicts between runs.
- [x] `docs/spec/013_QUEUE.md` — marked all 6 phases complete; top-of-file status header set to ✅ Complete for v0.8.0.
- [x] `cargo test --lib`: **1081 passed** / 0 failed. `bash test.sh`: **289 passed** / 0 failed.

### 2026-04-04 — v0.8 Queue Module — Phase 6: Examples & Docs

- [x] `examples/functional_tests/routes/queue.marreta` — Section 28: all queue features (`queue.push`, `queue.push as schema`, `queue.publish`, `queue.publish as schema`, `on queue`, `on queue as schema`, `on topic`, `nack`, `nack requeue`)
- [x] `examples/functional_tests/schemas/core.marreta` — added `queue_order` schema for Section 28
- [x] `examples/functional_tests/docker-compose.yml` — added RabbitMQ 4 service with healthcheck; app gains `MARRETA_QUEUE_PROVIDER`/`MARRETA_QUEUE_URL`; `depends_on` extended
- [x] `examples/functional_tests/test.sh` — Section 28 queue tests (skipped gracefully when queue not configured); rabbitmq startup/cleanup in local mode
- [x] `examples/functional_tests/app.marreta` — Section 28 added to index; `routes/queue.marreta` listed
- [x] `docs/spec/SPEC.md` — section 7 marked implemented; ack/nack table corrected (runtime errors nack without requeue)

### 2026-04-04 — v0.8 Queue Module — Phases 1–5

- [x] **Phase 1 — AST & Parser**: `TokenKind::{On,Topic,Nack,Requeue}`; `Statement::{OnQueue,OnTopic,Nack}` and `Expression::{QueuePush,QueuePublish}`; parser handles full queue syntax; `on` keyword disambiguated from named arg `on:`; 14 new parser unit tests
- [x] **Phase 2 — Queue Driver**: `src/queue/driver.rs` — `QueueDriver` trait, `QueueDelivery`, `QueueDriverError`, `MockQueueDriver`; `src/queue/rabbitmq.rs` — `RabbitMqDriver` via `lapin 4.4.0`; `src/queue/mod.rs` — `QueueEngine::from_env()`; `Cargo.toml` — `lapin = "4"`, `tokio-stream = "0.1"`
- [x] **Phase 3 — Route Loader & Registry**: `ConsumerDefinition`, `ConsumerKind`, `RouteRegistry.consumers`; multi-file consumer merge; 18 new unit tests
- [x] **Phase 4 — Interpreter Integration**: `Interpreter::with_queue()`; `queue.push`/`queue.publish` with optional schema filtering; `start_consumers()` in `server.rs`; ack/nack lifecycle; `/_health` → `queue_driver` injection chain through `main.rs`; 14 new unit tests
- [x] **Phase 5 — OpenAPI & Health**: `x-marreta-consumers` in `/openapi.json`; built-in `GET /_health` with `ok`, `api`, `version`, `db`, `doc`, `queue` fields; 8 new unit tests

### 2026-04-04 — v0.8 Queue Module Design

- [x] `docs/spec/docs/spec/013_QUEUE.md` — full implementation plan: `on queue/topic` consumer syntax, `queue.push/publish` producer syntax, optional schema contracts (`as schema_name`), ack/nack semantics, `QueueDriver` trait, RabbitMQ via `lapin`, `RouteRegistry` extension with `ConsumerDefinition`, OpenAPI `x-marreta-consumers` extension, health endpoint impact, 6-phase plan, design watch points
- [x] `docs/spec/SPEC.md` — section 7 (Queue) rewritten with full syntax reference, ack/nack table, producer/consumer examples, schema contract rules; roadmap entry updated

### 2026-04-04 — OpenAPI Response Object Fallback + Schema Contracts Showcase

- [x] `src/openapi.rs` — responses without a named schema now emit `{ "type": "object" }` instead of omitting `content` entirely; `reply html/text` routes use `text/html`/`text/plain` media types; `Fail`/`Require`/`Reject` error codes moved to `collect_error_codes()` separate from `collect_responses()`; `Transaction` blocks recursed for nested `reply` discovery
- [x] `src/file_loader.rs` — fixed silent multi-file load failure when entrypoint is a bare filename (e.g. `app.marreta` with no path prefix): `parent()` returned `""`, causing `read_dir("")` to fail and server to start with 0 routes in Docker
- [x] `examples/functional_tests/routes/contracts.marreta` — Section 27: 5 routes demonstrating all schema contract patterns (request-only, response-only, full contract, composed/Reference, optional fields)
- [x] `examples/functional_tests/schemas/core.marreta` — added `address`, `contact_payload`, `contact_response`, `contact_summary` for section 27
- [x] `examples/functional_tests/test.sh` — section 27 tests added (**279/279 passing**)
- [x] `tests/integration_tests.rs` — updated example file paths to `examples/snippets/` after reorganization; removed 4 empty placeholder test files (`interpreter_tests.rs`, `lexer_tests.rs`, `parser_tests.rs`, `value_tests.rs`)

---

### 2026-04-01 — Examples Reorganization

- [x] `examples/functional_tests/` — converted from single-file app to multi-file project (`routes/`, `tasks/`, `schemas/`); absorbed `doc_ops` as section 25 and `parallel` as section 26; `docker-compose.yml` gains MongoDB service (port 27019); `test.sh` expanded with MongoDB startup/cleanup and sections 25–26 (**268/268 passing**)
- [x] `examples/functional_tests/tasks/core.marreta` — extracted tasks (`double`, `triple`, `greet`, `summarise`, `count_active`, `list_active`) with `export`
- [x] `examples/functional_tests/schemas/core.marreta` — extracted schemas (`item_payload`, `item_response`, `search_payload`) with `export`
- [x] `examples/functional_tests/routes/core.marreta` — sections 1–16, 22–24 (all non-DB, non-doc routes)
- [x] `examples/functional_tests/routes/db.marreta` — sections 17–21 (CRUD, pipeline, parallel DB, native query, transactions)
- [x] `examples/functional_tests/routes/doc.marreta` — section 25 (full doc.* test suite: layers 1–4)
- [x] `examples/functional_tests/routes/parallel.marreta` — section 26 (POST broadcast routes: text, list, chain)
- [x] `examples/ecommerce/` — absorbed `ecommerce_doc`; added `routes/products_doc.marreta` and `routes/orders_doc.marreta` under `/doc/*` prefix; `schemas/payloads.marreta` gains `order_created_doc` (order_id: string); `marreta.env` and `docker-compose.yml` gain `MARRETA_DOC_*` config and MongoDB service; tasks/pricing not duplicated; added `test.sh` (**36/36 passing**)
- [x] `examples/snippets/` — new directory; 7 loose `.marreta` files moved via `git mv` (history preserved)
- [x] `tests/load/docker-compose.yml` — added MongoDB service and `MARRETA_DOC_*` envs on marreta container
- [x] `tests/load/run_doc.sh` — `COMPOSE_FILE` updated to `examples/ecommerce/docker-compose.yml`; container name resolved dynamically
- [x] `tests/load/collect_stats_doc.sh` — container name updated to `marreta-ecommerce`
- [x] Removed: `examples/db_ops/` (superseded by functional_tests sections 17–21), `examples/doc_ops/`, `examples/parallel/`, `examples/ecommerce_doc/`
- [x] Fixed: doc.* route path conflict in ecommerce (prefixed `/doc/`); missing `export` on tasks and schemas in functional_tests; MongoDB port mapping in ecommerce docker-compose

---

### 2026-04-01 — v0.7.2 Layer 3 Aggregation

- [x] `src/doc/query.rs` — `Accumulator` enum (Sum/Avg/Min/Max/Count); `DocQueryState` extended with `group_by`, `accumulators`, `post_sort`, `post_limit`; `DocQueryMode::Aggregate`; `is_aggregate()` helper
- [x] `src/doc/mongodb.rs` — `query_aggregate` added to `DocDriver` trait and `MongoDbDriver`; builds 4-stage pipeline (`$match` → `$group` → `$sort` → `$limit`) from `DocQueryState`
- [x] `src/interpreter.rs` — `apply_doc_query_pipeline_stage` extended with `group_by`, `sum`, `avg`, `min`, `max`, `count` (accumulator); post-group `order`/`limit` routing; `pick`/write terminal guards in aggregate mode; `fetch_all`/`fetch_one` route to `query_aggregate` when `mode == Aggregate`; `MockDocDriver.query_aggregate` returns configurable row; 18 new unit tests (1017 total)
- [x] `src/lexer.rs` — `emit_double` removes preceding `Newline` token when `>>` or `*>>` appears at line start, enabling indented multi-line pipelines
- [x] `src/parser.rs` — `parse_argument_list` accepts `TokenKind::As` as named argument key (`sum("f", as: "alias")`)
- [x] `examples/doc_ops/app.marreta` — Section 3: 4 aggregation routes (`/agg/seed`, `/agg/by-category`, `/agg/totals`, `/agg/top-electronics`)
- [x] `examples/doc_ops/docker-compose.yml` — MongoDB port 27018 exposed for local test runner
- [x] `examples/doc_ops/test.sh` — full functional test runner; **29/29 passing** (6 CRUD + 9 pipeline + 14 aggregation)

### 2026-03-31 — v0.7.1 Env Var Separation + Doc Pool Config

- [x] `src/config.rs` — added `doc_provider`, `doc_url`, `doc_pool_max_connections`, `doc_pool_min_connections`, `doc_pool_connect_timeout_ms`, `doc_pool_server_selection_timeout_ms` fields; read from `MARRETA_DOC_*` env vars (independent of `MARRETA_DB_*`)
- [x] `src/doc/mongodb.rs` — `DocEngine::from_config` reads `config.doc_provider`/`config.doc_url`; added `DocPoolConfig` struct; `MongoDbDriver::connect(url, pool_cfg)` applies all four pool params to `ClientOptions`
- [x] `src/interpreter.rs` — removed mutual exclusion guard; `require_doc_engine` error now references `MARRETA_DOC_PROVIDER`/`MARRETA_DOC_URL`
- [x] `src/main.rs` — log messages reference `config.doc_provider`
- [x] `examples/doc_ops/docker-compose.yml` — env vars updated to `MARRETA_DOC_PROVIDER`/`MARRETA_DOC_URL`
- [x] `examples/ecommerce_doc/docker-compose.yml` — same
- [x] `examples/ecommerce_doc/marreta.env` — same
- [x] `docs/spec/docs/spec/010_DOC_MODULE.md` — section 6 rewritten to document dual-namespace config and all pool params
- [x] `src/db/mod.rs` — test struct literals updated with new `None` doc fields

### 2026-03-31 — v0.7.0 Doc Module (MongoDB) — Layers 1 + 2

#### Implementation (feature/doc-module-v070)

- [x] `src/doc/mod.rs` — `DocEngine::from_config` initializes MongoDB engine when `MARRETA_DB_PROVIDER=mongodb`; single-provider constraint enforced
- [x] `src/doc/mongodb.rs` — `MongoDbDriver`: `connect()` with ping; full `DocDriver` trait impl (save, find, find_all, update_by_id, delete_by_id, query_fetch, query_fetch_one, query_count, query_exists, query_update, query_upsert, query_delete); `translate_mongo_error_op(err, op)` with per-operation context; `build_query_filter` (AND-join, all DocFilter variants, `_id` smart-cast); `build_query_options` (sort, limit, skip, projection)
- [x] `src/doc/query.rs` — `DocQueryState`, `DocFilter` (Eq/Ne/Gt/Gte/Lt/Lte/In/Like), `SortDirection`, `DocQueryMode`
- [x] `src/doc/bson.rs` — `value_to_bson` / `bson_to_value` full round-trip; ObjectId → String; DateTime → ISO string; Decimal128 → Float (documented precision limitation)
- [x] `src/value.rs` — new variants: `DocNamespace`, `DocCollection(String)`, `DocQueryBuilder(Box<DocQueryState>)`; all trait impls updated (type_name, is_truthy, Display, PartialEq, value_to_json)
- [x] `src/interpreter.rs` — `dispatch_doc_direct` (save, find, find_all, update/update_by_id, delete/delete_by_id); `apply_doc_query_pipeline_stage` (where, pick, order, limit, offset, like, in, update terminal, upsert terminal); terminals (fetch_all/fetch alias, fetch_one, count, exists, delete); `parse_doc_where_args` + `extract_doc_filter_from_expr` (StringLiteral LHS required, clear error for Identifier/named-arg); mutual exclusion guard for `db.*` under mongodb provider
- [x] `src/main.rs` + `src/server.rs` — DocEngine initialization and injection into interpreter
- [x] `examples/doc_ops/app.marreta` — full functional test app covering all Layer 1 + Layer 2 routes
- [x] `examples/doc_ops/docker-compose.yml` — MongoDB service + marreta server

#### Review fixes (post peer-review)

- [x] `where()` with bare identifiers now returns descriptive error pointing to string syntax
- [x] `where()` with named arguments (`where(field: val)`) now returns error — removed undocumented shorthand that violated dot-notation rationale
- [x] Terminal `"fetch"` renamed to `"fetch_all"`; `"fetch"` kept as alias for language identity continuity
- [x] `"fetch_all"` added as alias in `db.*` pipeline for symmetry
- [x] `"order_by"` and `"sort"` aliases removed from `doc.*` — only `"order"` accepted; direction now required (no silent default)
- [x] `"skip"` alias removed from `doc.*` — only `"offset"` accepted
- [x] `update` and `delete` added as canonical method names for `dispatch_doc_direct` (alongside `update_by_id`/`delete_by_id`)
- [x] `.unwrap()` in `query_upsert` replaced with proper `ok_or_else` error
- [x] All `map_err(translate_mongo_error)` calls in `MongoDbDriver` replaced with `translate_mongo_error_op(e, &op)` — every error now carries `"doc.{collection}.{operation}"` context
- [x] `examples/doc_ops/app.marreta` comment fixed: `cargo run -- run` → `cargo run -- serve`

#### Coverage

- [x] `src/doc/bson.rs` — **100%** (44/44). Added tests for all BSON type conversions and round-trips
- [x] `src/interpreter.rs` — **81%** (1105/1365). Added `MockDocDriver` + 35 unit tests covering all pipeline steps, terminals, dispatch methods, and error paths
- [x] All non-infra files ≥ 80%. Total project coverage: **73%** (infra floor: postgres.rs 9%, mongodb.rs 5%, main.rs 0%)

#### Functional validation

- [x] 25/25 functional tests passing against real MongoDB
- [x] Section 1 (Direct CRUD): save, find, find_all, update, delete — all operations verified
- [x] Section 2 (Pipeline): fetch_all, where (eq/gt/like/in), complex chain, count, exists, upsert, bulk update, bulk delete — all verified

### 2026-03-31 — v0.6.6 Coverage Push

- [x] `src/ast.rs` — 14 new tests: all 13 `BinaryOperator::Display` variants; `SchemaType::Display` for `Reference` and `TypedList`; `Statement::Raise` (with/without condition); `Statement::Transaction`; `Statement::Export`; `PipelineStage::Rescue`; `RescueHandler::Block`; `MapStatement::Skip`; `ParamDef` with schema; `Expression::Rescue`. Coverage: 68% → **100%**.
- [x] `src/openapi.rs` — 14 new tests: query binding parameter; dynamic status code fallback to 200; `Statement::Require`/`Reject` in `collect_responses`; `Statement::Export` wrapper descent; all 17 `status_description` codes including fallback; `stem_to_tag` (single/multi-word/empty); `source_file` used as tag name. Coverage: 79% → **100%**.
- [x] `src/server.rs` — 20 new tests: `swagger_ui_html`; 304 NOT_MODIFIED no-body; invalid status code fallback to 500; `execute_route` with Headers/Form/Raw/Query/Payload bindings; numeric + string path param coercion; DB engine injection; `register_route` for Patch/Delete verbs; `serve()` startup via `LocalSet` + abort (docs+CORS wildcard, specific origin, no-docs/no-CORS). Coverage: 71% → **91%**.
- [x] `src/db/mod.rs` — 6 new tests: `DbProvider` debug/clone/eq; `DbEngine` direct construction and clone via `StubDriver`; `from_config` with real URL exercises PoolConfig construction lines. Coverage: 57% → **91%**.
- [x] Overall: 75.52% → **77.99%** (+2.47pp). 1029 total tests.

### 2026-03-31 — v0.6.5 DB Mock Coverage

- [x] `src/interpreter.rs` — `MockDriver` + `MockTx` in-memory implementations of `DbDriver`/`DbTx` traits (no Postgres, no network). `interp_with_mock(seed_row)` helper creates a wired interpreter. 50 new tests in `mod tests_db_mock`: `db.TABLE.save/find/find_all/update/delete` (success + missing arg + wrong type errors); pipeline terminals `fetch/fetch_one/count/exists/delete/update` (success + error paths); pipeline steps `where/like/in/order_by/limit/offset/join/left_join/select` (success + wrong arg errors); `db.native_query` (simple SQL, `#{}` params, no-args, non-string); transaction commit (body ok) and rollback (body raises); QueryBuilder with non-FunctionCall/Identifier expression error.
- [x] interpreter.rs coverage: 61% → **83.8%** (+22.82%)
- [x] Overall coverage: 69.04% → **75.52%** (+6.48%)
- [x] 889 total tests (793 lib + 31 http_integration + 65 integration)

### 2026-03-31 — v0.6.4 Coverage Improvement (Round 2)

- [x] `src/config.rs` — 12 new tests: `MarretaConfig::load()` via env vars (host, port, invalid port fallback, cors false/0, cors_origin, docs_enabled false, docs_path, db_provider+url, all 6 pool params, absent pool params → None). Serialized with `static ENV_LOCK` mutex. Coverage: 37% → **98.3%**.
- [x] `src/interpreter.rs` — 58 new tests in `mod tests_coverage`: arithmetic cross-type (float+int, int+float, float+float for add/sub/mul/div/mod), incompatible type errors, subscript (Map[String], List[Integer], negative index, out-of-bounds, invalid type error), unary (negate float, negate invalid, not), reply html/text content types, reply non-integer status → TypeError, fail Map/List body → JSON, builtins (type, type arity error, len string/list/map, len arity error, len unsupported type, print), pipeline map on non-list error, TaskCall in pipeline, rescue not catching HttpResponse (expression and pipeline), broadcast in non-tokio OS thread path, export task/assignment, Route statement no-op, match fallback/no-match, NotCallable, short-circuit and/or, conditional assignment true/false, with_schemas, from_environment/into_environment, env_set. Coverage: 55% → **61%**.
- [x] 839 total tests (743 lib + 31 http_integration + 65 integration)
- [x] Overall coverage: 66.36% → **69.04%** (+2.68%)

### 2026-03-30 — v0.6.3 Coverage Improvement

- [x] `src/value.rs` — 88 new tests: `type_name()` for Task/DbNamespace/DbTable/QueryBuilder; `is_truthy()` for db/task variants; `call_method()` on non-method types (Null, Boolean, Task, DbNamespace); string `starts_with`, `ends_with`, `index_of`, `to_string`, unknown method + wrong arg type; list `join`, `sort` (ints/strings/mixed/floats), `unique`, `flatten`, `slice` (bounds, wrong type, missing args), `push`/`includes` no-args error, unknown method; map `values`, `delete`, `size`, `merge` (no-args, non-map), unknown method; integer `min`/`max` (integer, float, wrong type, missing), `to_string`, unknown method; float `round` (no-args, with places, wrong type), `floor`, `ceil`, `min`/`max` (integer, float, wrong type, missing), `to_string`, unknown method; `Display` for DbNamespace/DbTable/QueryBuilder; `value_to_json` for Task/DbNamespace/DbTable/QueryBuilder; `PartialEq` for DbNamespace/DbTable
- [x] `src/error.rs` — 65 new tests: `RaiseError`/`DbError` display; `semantic_code()` for all variants; `error_code()` fallback (RuntimeError) for HttpError/parser errors/HttpResponse; `operation_name()` for all variants including fallback "interpreter"; `display_message()` for all remaining variants (NotCallable, HttpError, FileNotFound, IoError, RouteConflict, ExportConflict, CircularSchemaReference, all parser errors); `line()` Some/None for all located/unlocated variants; `column()` Some/None (including InvalidIndentation → None); `ErrorCode::as_str()` for all 10 variants
- [x] Coverage: 61.58% → 66.31% (+4.73%). value.rs: 50% → 94.3%. error.rs: 57% → 81.6%.
- [x] 756 total tests (656 lib + 31 http_integration + 65 integration)

### 2026-03-31 — v0.6.2 Unit Test Coverage

- [x] `src/config.rs` — 12 new tests: all 6 `MARRETA_DB_POOL_*` params (file parsing, defaults → None, invalid values → None, bool coercion, inline comments, quoted values)
- [x] `src/db/mod.rs` — 6 new async tests: `DbEngine::from_config()` error paths — None provider → Ok(None), unsupported provider → DbError, missing URL → DbError, case-insensitive provider, semantic_code validation
- [x] `src/db/postgres.rs` — 10 new tests: `PoolConfig` struct (defaults, explicit values, zero min); `translate_pg_error()` for all constructible sqlx::Error variants (PoolTimedOut, PoolClosed, RowNotFound, Io), operation format, semantic_code, `db_err()` helper
- [x] `src/server.rs` — 18 new tests: `error_to_response()` for every MarretaError variant → correct HTTP status + JSON body; 204 body suppression (RFC 9110); extra headers forwarded; `to_axum_path()` conversion
- [x] `src/db/query_builder.rs` — 14 new edge case tests: empty filter list, limit-only, offset-only, Ne/Lte operators, float/null params, empty IN list, two joins, explicit columns with filter, filters_from_equality_map edge cases
- [x] `src/interpreter.rs` — 17 new tests: `raise` (message, semantic_code, conditional true/false, stops execution); rescue pipeline (catches raise, error.message, error.code, no-error pass-through); rescue expression (undefined var, no-error, error.code in handler); map/keep/skip blocks; task mutual calls; WrongArity + UndefinedTask carry names
- [x] `src/parser.rs` — 16 new tests: error cases (bare operator, missing closing bracket/brace/paren, EOF in expression, double operator); valid edge cases (empty list/map, nested structures, chained methods, pipeline chain, negative number, boolean expression, multiline map, conditional assignment, subscript, raise with condition)
- [x] 734 total tests passing (638 lib + 31 http_integration + 65 integration) — zero DB dependency

### 2026-03-31 — v0.6.1 DB Connection Pool Configuration

- [x] `MarretaConfig` — 6 new optional fields: `db_pool_max_connections`, `db_pool_min_connections`, `db_pool_acquire_timeout_secs`, `db_pool_idle_timeout_secs`, `db_pool_max_lifetime_secs`, `db_pool_test_before_acquire`
- [x] `config.rs` `load()` — reads all 6 from `marreta.env` + env vars; boolean coercion consistent with `MARRETA_CORS`
- [x] `postgres.rs` — `PoolConfig` struct; `connect(url, cfg)` uses `PgPoolOptions::new()` builder; sqlx defaults preserved when `None`
- [x] `db/mod.rs` — `from_config(&MarretaConfig)` (was two `&str` args); constructs `PoolConfig` from config; unsupported provider error corrected to `DbError`
- [x] `main.rs` — both `from_config` call sites updated
- [x] `openapi.rs` + `parser.rs` — fixed pre-existing compile errors in test helpers (`status_code: Expression`, not `i64`)
- [x] `interpreter.rs` — fixed pre-existing test failure: `interpolate_string` coerces undefined vars to `Value::Null`
- [x] Load test run `20260331T003019Z` with `MARRETA_DB_POOL_MAX_CONNECTIONS=50`: products p95 **36.9 ms → 4.9 ms (−87%)**, orders p95 **21.1 ms → 3.6 ms (−83%)**, throughput **4,039 → 4,826 req/s (+19.5%)**
- [x] 632 unit tests passing (was 535 after fixing compile errors; net +97 newly runnable tests)
- [x] `docs/performance/LOAD_TEST_POOL_CONFIG_20260331_0030.md` — load test analysis
- [x] `docs/spec/docs/spec/009a_DB_POOL_CONFIG.md` — implementation plan
- [x] `docs/spec/SPEC.md` — pool env vars documented in env config section

### 2026-03-30 — v0.6.0 Error Handling

#### Phase 1 — `raise` keyword
- [x] `TokenKind::Raise` + `keyword_lookup("raise")`
- [x] `Statement::Raise { message, condition, line, column }` in AST
- [x] `MarretaError::RaiseError { message }` — Display: `"raise: {message}"`
- [x] `parse_raise()` — `raise MSG` and `raise MSG if CONDITION`
- [x] `parse_require()` extended — `require X else raise MSG` emits `Statement::Raise`
- [x] `execute_statement(Statement::Raise)` — evaluates message, returns `Err(RaiseError)`
- [x] `server.rs` — `RaiseError` → HTTP 500 `{"error": "<message>"}`

#### Phase 2 — `rescue` pipeline step
- [x] `TokenKind::Rescue` + `keyword_lookup("rescue")`
- [x] `PipelineStage::Rescue { handler: RescueHandler }` + `RescueHandler` enum (Inline/Block)
- [x] `parse_rescue_stage()` — inline and block forms
- [x] Railway-oriented pipeline evaluation — errors deferred to rescue stage
- [x] `build_error_map()` helper — `{ message, op, code }` map
- [x] `execute_rescue_handler()` — dispatches Inline/Block

#### Phase 3 — `rescue` expression modifier
- [x] `Expression::Rescue { expr, handler }` in AST
- [x] Parser: lowest-precedence infix `expr rescue handler`
- [x] `evaluate(Expression::Rescue)` — catches errors, injects `error` map, evaluates handler

#### Phase 4 — Marreta Error Identity
- [x] `MarretaError::DbError { message, operation }` variant
- [x] `translate_pg_error()` in `postgres.rs` — no raw `sqlx::Error` propagates beyond module
- [x] `MarretaError::semantic_code()`, `operation_name()`, `display_message()` methods
- [x] `__fail__` synthetic built-in for `fail CODE, MSG` in expression position
- [x] Panic hook registered in `main.rs` via `std::panic::set_hook`

#### Phase 5 — Examples + E2E validation
- [x] Section 24 added to `examples/functional_tests/app.marreta` (9 new routes)
- [x] Section 24 tests added to `test.sh`
- [x] 176/176 functional tests passing (167 existing + 9 new), zero regressions

#### Post-review fixes (2026-03-30)
- [x] `ErrorCode` enum + `as_str()` — all semantic codes derived from a single source of truth; `semantic_code()` delegates to it
- [x] `error_code()`, `line()`, `column()` accessors added to `MarretaError`
- [x] `display_message()` covers all variants — no `"an error occurred"` fallback
- [x] `translate_pg_error()` rewritten — uses `db_err.message()` directly, no static mapping
- [x] `server.rs` `error_to_response()` consolidated — `code` + optional `at: "line:N"` in all HTTP 500 bodies
- [x] Section 24 extended with 4 error identity routes + assertions (reference_error, raise_error, db_error)
- [x] `postgres.rs` `connect()`: `TypeError` → `DbError`, sqlx message leak removed
- [x] `postgres.rs` save/update `None` arms (pool + tx): `TypeError` → `DbError` with correct `operation` field
- [x] `interpreter.rs` `dispatch_native_query`: removed `map_err(TypeError)` — preserves `DbError` from driver
- [x] 180/180 functional tests passing, zero regressions

### 2026-03-30 — v0.6.0 Language Ergonomics

#### Phase 5 — String methods `starts_with` / `ends_with`
- [x] `str.starts_with(prefix)` → Boolean
- [x] `str.ends_with(suffix)` → Boolean

#### Phase 6 — Utility Methods
- [x] `str.index_of(s)` → Integer (-1 if not found)
- [x] `list.join(sep)` → String
- [x] `list.sort()` → List (ascending, cross-type: Integer < Float < String < Boolean < Null)
- [x] `list.unique()` → List (deduplicate preserving order)
- [x] `list.flatten()` → List (depth-1)
- [x] `list.slice(from, to)` → List (out-of-bounds clamped)
- [x] `map.delete(key)` → new Map without key
- [x] `map.size()` → Integer
- [x] `float.round(n?)`, `float.floor()`, `float.ceil()`
- [x] `integer.min(n)`, `integer.max(n)`, `float.min(n)`, `float.max(n)`

#### Phase 1 — `fail` as full expression
- [x] `fail CODE, map_literal` returns map as JSON body
- [x] `fail CODE, variable` serializes variable as JSON body
- [x] Map/List values no longer wrapped in `{"error": ...}`

#### Phase 2 — String interpolation with expressions
- [x] `#{}` now parses and evaluates full expressions (method calls, arithmetic, etc.)
- [x] Simple variable `#{name}` still works (no regression)

#### Phase 3 — Subscript access `expr[key]`
- [x] `Expression::Subscript { object, key }` added to AST
- [x] `map["string-key"]` and `list[integer-index]` evaluation
- [x] Out-of-bounds / missing key returns `null`
- [x] Negative list indices supported

#### Phase 4a — `reply` with dynamic status
- [x] `Reply.status_code` changed from `i64` to `Expression` in AST
- [x] Parser evaluates status code expression at runtime
- [x] `reply variable, body` works

#### Phase 4b — `keep expr if cond` and `skip if cond`
- [x] `MapStatement` enum added: `Statement`, `Keep { value, condition }`, `Skip { condition }`
- [x] `skip` keyword added to lexer/token
- [x] Multiple `keep if` — first matching arm wins
- [x] Block with no keep/skip firing drops element implicitly
- [x] Existing unconditional `keep expr` unchanged

#### Test results
- 167/167 functional tests passing (37 new tests added in Sections 22–23)

---

### 2026-03-25 — Session 1

#### Phase 0 — Design & Specification
- [x] Discussed language vision, syntax, and architecture
- [x] Chose name "Marreta" with domain marreta.dev
- [x] Created `SPEC.md` — full language specification
- [x] Created `IMPLEMENTATION_PLAN.md` — detailed core implementation plan (15 phases)
- [x] Translated both documents to English (keeping "Marreta" as Brazilian identity)
- [x] Initial git commit on `main` branch

#### Phase 1 — Scaffold
- [x] `cargo init` with project name `marreta`
- [x] `Cargo.toml` configured (MIT license, description, `pretty_assertions` dev dep)
- [x] `src/lib.rs` with all module declarations
- [x] Empty source files: `token.rs`, `lexer.rs`, `parser.rs`, `ast.rs`, `interpreter.rs`, `environment.rs`, `value.rs`, `error.rs`
- [x] `tests/` directory with empty test files
- [x] `examples/` directory with empty `.marreta` example files
- [x] `cargo check` passes

#### Phase 2 — token.rs
- [x] `TokenKind` enum with all 50+ variants (literals, operators, delimiters, keywords, control)
- [x] `Token` struct with kind, line, column, lexeme
- [x] `Token::new()` and `Token::eof()` constructors
- [x] `Display` impl for Token (debug formatting)
- [x] `keyword_lookup()` function mapping 28 reserved words
- [x] 10 unit tests — all passing

#### Phase 3 — error.rs
- [x] `MarretaError` enum with 16 variants (lexer, parser, interpreter, HTTP, I/O)
- [x] `Display` impl with human-friendly messages including line:column
- [x] `std::error::Error` impl
- [x] `MarretaResult<T>` type alias
- [x] 17 unit tests — all passing

#### Phase 4 — ast.rs
- [x] `Statement` enum: Assignment, ConditionalAssignment, Require, Reject, TaskDef, ExpressionStatement
- [x] `Expression` enum: all literals, Identifier, BinaryOp, UnaryOp, PropertyAccess, MethodCall, FunctionCall, TaskCall, Match, Pipeline, Broadcast
- [x] `TaskBody` (Inline / Block), `MatchArm`, `MatchPattern`, `PipelineStage`, `Argument`
- [x] `BinaryOperator` and `UnaryOperator` enums with Display
- [x] Line/column tracking on all Statement variants
- [x] 15 unit tests — all passing

#### Phase 5 — value.rs
- [x] `Value` enum: Integer, Float, String, Boolean, Null, List, Map (Rc<RefCell>), Task
- [x] `is_truthy()` with full falsy/truthy rules
- [x] `type_name()` for error messages
- [x] `Display` impl (JSON-like output, strings quoted inside collections)
- [x] `PartialEq` with cross-type Integer/Float comparison
- [x] `call_method()` dispatcher for all built-in methods
- [x] String methods: length, upper, lower, trim, contains, split, replace, to_string
- [x] List methods: length, first, last, empty?, push, includes, reverse
- [x] Map methods: keys, values, has, merge
- [x] Integer/Float methods: abs, to_string
- [x] Helper constructors: `empty_map()`, `map_from()`
- [x] 35 unit tests — all passing

#### Phase 6 — environment.rs
- [x] `Environment` struct with scope stack (`Vec<HashMap>`)
- [x] `push_scope()` / `pop_scope()` (global scope protected)
- [x] `set()` in current scope, `get()` with lexical scoping (innermost first)
- [x] `has()`, `update()` (finds nearest scope), `depth()`
- [x] `all_variables()` and `all_tasks()` for REPL commands
- [x] `Default` trait impl
- [x] 14 unit tests — all passing

#### Phase 7 — lexer.rs
- [x] Hand-rolled `Lexer` struct with character-by-character scanning
- [x] All literal types: Integer, Float, String (with escape sequences)
- [x] String interpolation `#{}` preserved for runtime resolution
- [x] All operators: arithmetic, comparison, assignment, pipeline (`>>`/`*>>`), arrows (`->`/`=>`)
- [x] All delimiters: parens, brackets, braces, comma, dot, colon
- [x] Keyword recognition via `keyword_lookup()` (28 reserved words)
- [x] Identifier support including `?` suffix (Ruby-style `empty?`)
- [x] Significant indentation: Indent/Dedent tokens via indent stack
- [x] Newline suppression after continuation operators (`>>`, `->`, `=>`, `,`)
- [x] No duplicate newlines, blank line handling
- [x] `#` comments (line and inline, context-aware inside strings)
- [x] Line/column tracking on all tokens
- [x] Error handling: UnexpectedCharacter, UnterminatedString, InvalidIndentation, InvalidNumber
- [x] Number-before-dot disambiguation (`5.abs()` → Integer + Dot + Identifier)
- [x] 40 unit tests — all passing

#### Phase 8 — parser.rs
- [x] Pratt parser (top-down operator precedence) with 9 precedence levels
- [x] Assignments: simple `x = expr` and conditional `x = expr if cond`
- [x] `require` / `reject` guard statements with `else fail CODE, MSG`
- [x] `task` definitions: inline (`=> expr`) and block (indented body)
- [x] `match` expressions with literal patterns and `_` fallback
- [x] All binary operators: arithmetic, comparison, logical (`and`/`or`)
- [x] Unary operators: `-` (negate) and `not`
- [x] Property access chains (`a.b.c`), method calls with args
- [x] Function calls with positional and named arguments (`name: value`)
- [x] List `[...]` and map `{...}` literals
- [x] Pipeline `>>` with multi-stage and `map`/`keep` blocks
- [x] Broadcast `*>>` with indented target list
- [x] Infrastructure keywords (`db`, `queue`, `cache`) parsed as identifiers
- [x] Grouped expressions `(expr)`
- [x] `or` as default value operator
- [x] Error recovery with descriptive messages (line:column)
- [x] Multi-line pipeline support (newline+indent before `>>`)
- [x] 32 unit tests — all passing

#### Phase 9 — interpreter.rs
- [x] Tree-walking interpreter with `Interpreter` struct wrapping `Environment`
- [x] Statement execution: Assignment, ConditionalAssignment, Require, Reject, TaskDef, ExpressionStatement
- [x] All literal evaluation: Integer, Float, String, Boolean, Null, List, MapLiteral
- [x] Identifier lookup with UndefinedVariable error
- [x] Binary operations: arithmetic (+, -, *, /, %) with Integer/Float promotion
- [x] String concatenation (`String + any`), List concatenation (`List + List`)
- [x] Comparison operators (>, <, >=, <=, ==, !=) with cross-type Int/Float
- [x] Logical operators (and/or) with short-circuit evaluation and value return
- [x] Unary operators: negate (-) and not
- [x] Property access on Maps (returns Null for missing keys)
- [x] Method calls dispatched to `Value::call_method()`
- [x] Function/task calls with scope push/pop and arity checking
- [x] Block tasks with body execution and implicit return
- [x] Built-in functions: `print()`, `type()`, `len()`
- [x] Match expressions with literal patterns and fallback
- [x] Pipeline (`>>`) with task application and multi-stage chaining
- [x] Pipeline map/keep blocks with per-item scoping
- [x] Broadcast (`*>>`) applying input to multiple targets
- [x] String interpolation `#{}` resolved at runtime
- [x] Division by zero detection (integer and float)
- [x] NotCallable, WrongArity, TypeError, HttpError propagation
- [x] 91 unit tests — all passing (254 total across all modules)

#### Phase 10 — main.rs (CLI + REPL) + Integration Tests
- [x] CLI with command dispatch: `run`, `repl`, `tokenize`, `parse`, `--version`, `--help`
- [x] File execution: read `.marreta` file, tokenize, parse, execute
- [x] REPL with persistent state between lines
- [x] REPL special commands: `.exit`, `.quit`, `.vars`, `.tasks`, `.clear`, `.help`
- [x] Multi-line input detection (continuation after `>>`, `->`, `=>`, `,`, `task`, `match`)
- [x] Debug commands: `tokenize` (print tokens), `parse` (print AST)
- [x] Error output to stderr with non-zero exit code
- [x] 5 example programs: hello, variables, tasks, conditionals, pipelines
- [x] 65 integration tests (CLI flags, example files, end-to-end source execution)
- [x] E2E coverage: arithmetic, strings, interpolation, tasks, match, pipelines, map/keep, broadcast, require/reject, methods, builtins, error cases
- [x] Comprehensive test coverage: task-returning-match, pipeline+map+task, broadcast strings, require/reject edge cases (empty list, zero, empty string), type errors, wrong arity, truthiness for all types, nested tasks, complex multi-step programs, float precision, empty programs, comments
- [x] Parser fix: `match` as standalone expression (enables `task x(n) => match n ...`)
- [x] 353 total tests — all passing (288 unit + 65 integration)

#### Phase 13 — Polish
- [x] `cargo clippy` — all 5 warnings fixed (map_entry, collapsible_if, needless_return, cloned_ref_to_slice_refs)
- [x] `cargo fmt` — all files formatted to rustfmt standard
- [x] REPL accepts `quit`/`exit` without dot prefix
- [x] 353 tests — all passing

### 2026-03-26 — Session 2

#### Acceptance Criteria Audit & Gap Fixes
- [x] Full audit against IMPLEMENTATION_PLAN.md — identified 6 gaps
- [x] **Gap #1**: Pipeline `>>` now implicitly iterates over Lists (task applied per-element)
- [x] **Gap #2**: Interpreter errors now propagate line/column from statements (no more `line: 0, column: 0`)
- [x] **Gap #3**: Removed `unwrap()` from production code in main.rs REPL (replaced with `unwrap_or`/`let _ =`)
- [x] **Gap #4**: Added doc comments on all public API items (lib.rs modules, token.rs, lexer.rs, parser.rs)
- [x] **Gap #5**: Updated SPEC.md with v0.1 implementation notes (broadcast returns List, pipeline iteration, match standalone, builtins, REPL shortcuts, interpolation limits)
- [x] **Gap #6**: Broadcast `*>>` returning List of results documented as deliberate improvement
- [x] `cargo clippy` — 0 warnings
- [x] 353 tests — all passing
- [x] `unwrap()` removed from lexer.rs indent stack (replaced with `unwrap_or`)
- [x] All 25 acceptance criteria in docs/spec/001_CORE.md marked complete
- [x] IMPLEMENTATION_PLAN.md renamed to docs/spec/001_CORE.md
- [x] docs/spec/002_HTTP.md created for v0.2

---

## v0.5.1 — DB Module closing items — CLOSED

### 2026-03-29 — Session (v0.5.1 — Phase 6: like/in/native_query + functional test suite)

#### Functional test suite (`examples/functional_tests/`)
- [x] `app.marreta` — 21-section tutorial-style app covering all language features (130 routes)
- [x] `test.sh` — curl/jq test runner; `./test.sh` (local binary) or `./test.sh --docker` (fully containerized)
- [x] `seed.sql` — `items` table, 4 rows; mounted as `docker-entrypoint-initdb.d`
- [x] `docker-compose.yml` — postgres:16-alpine + marreta app service with healthcheck
- [x] `Dockerfile` — mirrors `tests/load/Dockerfile` pattern; each example carries its own image definition
- [x] **130/130 tests passing** end-to-end against live Postgres

#### Phase 6 — Closing items
- [x] `native_query` rewritten to use `#{}` interpolation exclusively — extracts expressions from raw AST `StringLiteral` before language interpolation runs, evaluates each in current scope, binds as `$1`/`$2` prepared params; fixes bug where `evaluate_args` would expand `#{}` before extraction
- [x] `>> like("col", "pattern")` — new accumulating QueryBuilder step (`FilterOp::Like`)
- [x] `>> in("col", list)` — new accumulating QueryBuilder step (`FilterOp::In`, list expansion)
- [x] `extract_native_query_params` + `evaluate_source_expression` helper methods added to `Interpreter`
- [x] All Phase 6 ACs marked complete; `docs/spec/009_DB_RELATIONAL.md` status set to CLOSED

#### Language gap analysis (discovered during functional test authoring → v0.6.0 scope)
- `fail` body must be any expression (currently string-only)
- `#{}` in strings must evaluate full expressions (currently variable lookup only)
- Subscript access `expr[key]` for hyphenated map keys and list indexing
- `keep expr if cond` / `skip if cond` inside `map` blocks
- `reply` status must accept any integer expression (currently literal only)
- `starts_with` / `ends_with` string methods
→ All captured in `docs/spec/docs/spec/008_LANGUAGE_ERGONOMICS.md`

---

## v0.5.0 — DB Module (Relational) — Design locked

**Design locked:** 2026-03-27 (see `docs/spec/docs/spec/009_DB_RELATIONAL.md`)

### Design Decisions

- **Three namespaces** — `db.*` (relational/PostgreSQL), `doc.*` (document/MongoDB, v0.6.0), `cache.*` (key-value/Redis, v0.8.0). Intentional separation: each namespace carries distinct semantics and prevents impedance mismatch from being hidden behind a unified API.
- **Two pipeline styles** — direct (`db.users.find(id)`) for simple single-step operations; pipeline composition (`db.orders >> where(...) >> fetch`) for multi-clause queries. Both always available.
- **Two-context pipeline model** — before a terminal (`fetch`, `count`, etc.) the pipeline is in SQL context, accumulating clauses. The terminal closes the query context and returns a plain `Value` to the language pipeline. After the terminal, `map`, `keep`, tasks, and all language operations work normally.
- **Expression-based `where`** — `>> where(total > 1000, status: "active")` uses the language's existing operators directly. `like` and `in` added as context keywords (only inside `where(...)`, not global reserved words). Key-value shorthand (`key: value`) retained for equality.
- **`>> select(cols...)`** — SQL-level projection step, avoids fetching unnecessary columns. Computed aliases supported: `select(id, net: "total * 0.9")`.
- **`native_query` with interpolated params** — `#{}` variables extracted at evaluation time and bound as `$1/$2` prepared statement parameters. Never concatenated. Same syntax the language already uses.
- **`transaction` block** — atomic, sequential, indented body. `*>>` inside `transaction` is a runtime error (parallel execution and atomicity are mutually exclusive). Nesting is a startup error.
- **`*>>` for parallel DB queries** — outside `transaction`, parallel queries are explicitly supported: `payload *>> -> db.users.find(id) -> db.orders >> where(user_id: id) >> fetch`.
- **Phase 0 prerequisite** — async interpreter refactor + true parallel `*>>` via `tokio::try_join_all`. Required because `sqlx` is async-only. Both structural changes done together to avoid two separate refactors.
- **`QueryBuilder` without terminal** — runtime error in v0.5.0 (interpreter flag). Startup static analysis deferred — see Design Watch Points in SPEC.md.

### 2026-03-27 — Session (v0.5.0 Phase 5 — ecommerce example)

#### Phase 5 — Ecommerce Example Update

- [x] `routes/products.marreta` — `GET /products`, `GET /products/:id`, `POST /products`, `DELETE /products/:id` using `db.products.*`
- [x] `routes/orders.marreta` — `GET /orders`, `GET /orders/:id`, `POST /orders` (with `transaction` block), `DELETE /orders/:id` using `db.orders.*`
- [x] `schemas/payloads.marreta` — `order_created` schema updated with `order_id: integer`
- [x] `app.marreta` — bumped to `project_version = "2.0.0"`
- [x] `docker-compose.yml` — Postgres 16-alpine service with healthcheck
- [x] `seed.sql` — `products` and `orders` table DDL + 3 sample products
- [x] `marreta.env` — example env config for local development

### 2026-03-27 — Session (v0.5.0 Phase 4 — native_query + transaction)

#### Phase 4 — native_query + Transaction Blocks

- [x] `TokenKind::Transaction` — new reserved keyword `transaction` in lexer/token.rs
- [x] `Statement::Transaction { body, line, column }` — new AST node in ast.rs
- [x] `Parser::parse_transaction()` — parses indented body; rejects nested `transaction` at parse time
- [x] `inside_transaction: bool` on `Parser` struct — nesting guard set/cleared around body parse
- [x] `inside_transaction: bool` on `Interpreter` struct — `*>>` guard set/cleared during execution
- [x] `Statement::Transaction` added to position-tracking match in `execute_statement`
- [x] `Statement::Transaction` execution: `BEGIN` → body → `COMMIT`; `ROLLBACK` on error
- [x] `*>>` inside `transaction` block → runtime error ("broadcast is not allowed inside a transaction")
- [x] `db.native_query(sql, arg1, …)` — `#{}` → `$1`, `$2`, … placeholder replacement; dispatches to `driver.native_query`
- [x] 5 new unit tests (parser nesting, no-engine guards, broadcast-in-transaction); 536 lib tests passing

### 2026-03-27 — Session (v0.5.0 Phase 3 — Query pipeline composition)

#### Phase 3 — Query Pipeline Composition

- [x] `DbTable` → `QueryBuilder` automatic promotion on first `>>` in pipeline
- [x] `>> where(col: val)` — equality filter via named arg
- [x] `>> where(col > val)` — comparison filter via BinaryOp AST walk (`extract_filter_from_expr`)
- [x] `>> where(col >= val, status: "paid")` — multiple filters in single call
- [x] Chained `>> where(...) >> where(...)` — filters accumulated across calls
- [x] `>> join("table", on: "fk")` — INNER JOIN accumulation
- [x] `>> left_join("table", on: "fk")` — LEFT JOIN accumulation
- [x] `>> select("col1", "col2")` — column projection
- [x] `>> order_by("col desc")` — ORDER BY clause
- [x] `>> limit(n)` / `>> offset(n)` — pagination clauses
- [x] `>> fetch` → `Value::List[Map]` (terminal, executes query)
- [x] `>> fetch_one` → `Value::Map` or `Value::Null`
- [x] `>> count` → `Value::Integer`
- [x] `>> exists` → `Value::Boolean`
- [x] `>> delete` → `Value::Integer` (rows affected)
- [x] `>> update({…})` → `Value::Integer` (rows affected)
- [x] `map`/`keep` on QueryBuilder without terminal → descriptive error with "did you forget >> fetch?"
- [x] Unsupported operator in `where()` → descriptive error
- [x] Non-identifier LHS in `where()` expression → descriptive error
- [x] 20 new unit tests — accumulation (pure), terminal errors, error hints; 627 tests passing

### 2026-03-27 — Session (v0.5.0 Phase 2 — Direct CRUD interpreter binding)

#### Phase 2 — Direct CRUD Interpreter Binding

- [x] `Value::DbNamespace`, `Value::DbTable(String)`, `Value::QueryBuilder(Box<QueryState>)` added to `value.rs` — no new AST nodes or parser changes required
- [x] `Identifier("db")` → `Value::DbNamespace` (namespace entry point intercepted in evaluate)
- [x] `PropertyAccess(DbNamespace, table)` → `Value::DbTable(table)` (intercepted before Map access)
- [x] `MethodCall(DbTable, method, raw_args)` → `dispatch_db_direct()` dispatches to driver via `tokio::Handle::block_on`
- [x] `save(map)` → INSERT RETURNING *, returns full `Value::Map`
- [x] `find(id)` → SELECT WHERE id=$1, returns `Value::Map` or `Value::Null`
- [x] `find_all()` → SELECT all, returns `Value::List`
- [x] `find_all(key: val, ...)` → SELECT with equality filters; named args read from raw AST to preserve key names (`args_to_equality_filters`)
- [x] `update(id, partial_map)` → UPDATE SET ... WHERE id=$N RETURNING *, returns updated `Value::Map`
- [x] `delete(id)` → DELETE WHERE id=$1, returns `Value::Boolean`
- [x] Descriptive error when no DB configured (AC-1.4 fulfilled via Phase 2)
- [x] 10 new unit tests — DbNamespace/DbTable intermediate values, Display, all ops error correctly without engine
- [x] 501 → 608 tests passing

### 2026-03-27 — Session (v0.5.0 Phase 1 — DB infrastructure)

#### Phase 1 — DB Infrastructure

- [x] `sqlx 0.8` (postgres, runtime-tokio-rustls) + `async-trait` added to `Cargo.toml`
- [x] `src/db/driver.rs` — `DbDriver` trait (save, find, find_all, update_by_id, delete_by_id, query_fetch, query_fetch_one, query_count, query_exists, query_update, query_delete, native_query), `QueryState`, `FilterClause`, `FilterOp` (Eq/Gt/Gte/Lt/Lte/Ne/Like/In), `JoinClause`, `JoinKind`
- [x] `src/db/query_builder.rs` — `build_select` (handles all filter ops, joins, ORDER BY, LIMIT, OFFSET, IN list expansion with correct `$N` numbering), `build_update`, `build_delete`, `filters_from_equality_map`
- [x] `src/db/postgres.rs` — `PostgresDriver` wrapping `sqlx::PgPool`; PG type → `Value` mapping (BOOL, INT2/4, INT8, FLOAT4, FLOAT8/NUMERIC, fallback String); dynamic binding macro pattern (no compile-time checked queries)
- [x] `src/db/mod.rs` — `DbEngine` (Arc<dyn DbDriver>), `from_config()` reads `MARRETA_DB_PROVIDER` + `MARRETA_DB_URL`, startup error on unsupported provider or missing URL
- [x] `src/config.rs` — `db_provider` + `db_url` fields added to `MarretaConfig`
- [x] `src/interpreter.rs` — `db_engine: Option<DbEngine>` field, `with_db()` builder
- [x] `src/server.rs` + `ServerConfig` — `db_engine` field; threaded into each request via `execute_route`
- [x] `src/main.rs` — DB engine initialized at startup before serve; prints "DB connected (postgres)" when active
- [x] 22 unit tests for `query_builder` — pure SQL generation, no DB required; all passing
- [x] 479 → 501 tests passing

### 2026-03-29 — Session (v0.5.0 — transaction rollback + parallel *>> fix)

#### Transaction Rollback
- [x] `DbTx` trait added to `src/db/driver.rs` — all CRUD + pipeline methods with `&mut self` + `commit`/`rollback` consuming `self: Box<Self>`
- [x] `PgTransaction` struct in `src/db/postgres.rs` — wraps `sqlx::Transaction<'static, Postgres>`, all ops use `&mut *self.inner` as executor
- [x] `begin()` method added to `DbDriver` trait and `PostgresDriver` impl
- [x] `TxHolder` wrapper in interpreter — `Arc<Mutex<Option<Box<dyn DbTx+Send>>>>`, `Clone` returns empty holder (safe for `*>>` isolation)
- [x] `active_tx` field added to `Interpreter`; `has_active_tx()`, `take_tx()`, `restore_tx()` helpers
- [x] `run_async` free function — same as `block_db` but without `&self`, needed for `commit`/`rollback` take-and-restore pattern
- [x] All DB dispatch sites updated: take-and-restore pattern routes ops through active transaction when one is open
- [x] `Statement::Transaction` handler: `begin()` stores tx, body executes within tx context, `commit` on success / `rollback` on error
- [x] `inside_transaction` flag on interpreter — `*>>` inside `transaction` returns runtime error
- [x] Verified: rollback test (force-fail inside transaction) leaves row count unchanged

#### Parallel `*>>` with DB calls — Fixed
- [x] Root cause: `*>>` handler called `handle.block_on()` directly on a tokio worker thread (`c.runtime = Entered`), causing "Cannot start a runtime from within a runtime" panic
- [x] Fix: `block_in_place(|| handle.block_on(join_all(tasks)))` — exits async context before waiting, allowing a backup worker to run the spawned branch tasks
- [x] Each branch spawned via `handle.spawn(async { block_in_place(|| evaluate()) })` — runs on a worker thread with `allow_block_in_place: true`, so nested `block_db` calls work correctly
- [x] `collect_join_handles` async helper function added (avoids `futures` crate dependency)
- [x] `test_broadcast_inside_transaction_errors` added — verifies runtime guard

#### Integration Tests
- [x] `tests/http_integration_tests.rs` — 3 failing tests fixed: URL params now coerced to Integer; match arms updated to integer literals
- [x] `tests/load/k6/db_ops.js` — complete rewrite: 27 steps covering all DB operations (direct CRUD, pipeline, parallel, native query, transactions); step 26 uses fresh item for delete test
- [x] 536 lib unit tests passing; all 63 k6 checks passing

### 2026-03-27 — Session (v0.5.0 Phase 0 — parallel broadcast)

#### Phase 0 — Parallel `*>>` (prerequisite for async DB)

- [x] `Interpreter` derives `Clone` — enables fork-per-branch execution
- [x] `Expression::Broadcast` evaluation replaced: sequential `for` loop → `std::thread::spawn` per branch, results joined in declaration order
- [x] Each branch receives an independent interpreter fork — branches cannot observe each other's side effects
- [x] `apply_broadcast_value()` added — no implicit list iteration for `*>>` branches (distinct from `apply_pipeline_value()` which iterates for `>>`)
- [x] Semantic distinction documented and tested: `*>>` passes full value to each branch; `>>` iterates when input is a List
- [x] `test_broadcast_result_order_is_declaration_order` — results arrive in declaration order
- [x] `test_broadcast_parallel_executes_concurrently` — spawn overhead is negligible
- [x] `test_broadcast_result_piped_into_task` — `*>>` result flows into `>>` with correct implicit iteration
- [x] `test_list_piped_into_broadcast` — List input arrives whole at each branch
- [x] `test_map_then_broadcast` — `map` result flows into `*>>` correctly
- [x] 473 → 479 tests passing
- [x] `examples/parallel/` — 3 functional routes validating all combinations:
  - `POST /analyze/text` — scalar `*>>` parallel tasks
  - `POST /analyze/list` — List `*>>` parallel aggregations (no implicit iteration)
  - `POST /analyze/chain` — `*>>` result `>>` map pipeline chain

### 2026-03-27 — Session (v0.5.0 design)

- [x] `SPEC.md` — Section 4 rewritten: two-style API, two-context pipeline model, `>> select(...)`, terminal operations, expression-based `where`, inner/left joins with table-prefixed results, `native_query` with type mapping table, `transaction` block with pipeline support
- [x] `SPEC.md` — Section 5 (`doc.*`) expanded: same expression-based filter API, MongoDB operator mapping, ordering/pagination/aggregation, conventions
- [x] `SPEC.md` — Section 7 (`cache.*`) expanded: `incr`/`decr`, `exists`, conventions
- [x] `SPEC.md` — Section 8 (`marreta.env`) updated with `MARRETA_DOC_*` vars
- [x] `SPEC.md` — Section 9 reserved words updated: `doc`, `transaction` added (~28 total)
- [x] `SPEC.md` — Section 12 roadmap updated: v0.5 DB Relational, v0.6 Doc, v0.7 Queue, v0.8 Cache
- [x] `SPEC.md` — Design Watch Points section added: 5 points covering context keywords, QueryBuilder terminal, pipeline universality, `*>>` inside transaction, `*>>` result ordering
- [x] `docs/spec/009_DB_RELATIONAL.md` created — 5 phases, 30 implementation steps, 25 acceptance criteria, full design rationale section

---

## v0.4.0 — Advanced Schemas & Task Contracts (COMPLETE)

**Design locked:** 2026-03-26 (see `docs/spec/docs/spec/006_ADVANCED_SCHEMAS_040.md`)

### Design Decisions

- Schema composition: `billing: address` references another schema by name
- Typed lists: `items: list of order_item` — keyword-driven, no generics notation
- Task contracts: `task apply_taxes(order as order_payload)` — inline `as` per parameter, consistent with existing `take payload as schema` and `reply 201 as result` patterns
- Circular schema references detected at startup — server does not start on cycle
- `TypeError` from task contract violation yields HTTP 500 (programmer error, not client error)

### 2026-03-26 — Session 5 (v0.4.0)

#### Phase 1 — Lexer, AST & Parser
- [x] `token.rs` — `TokenKind::Of` added for `list of Type` syntax
- [x] `ast.rs` — `SchemaType::Reference(String)` and `SchemaType::TypedList(Box<SchemaType>)` variants added
- [x] `ast.rs` — `ParamDef { name, schema }` struct introduced; `Statement::TaskDef.params` migrated from `Vec<String>` to `Vec<ParamDef>`
- [x] `ast.rs` — `SchemaType::Display` updated for new variants
- [x] `value.rs` — `Value::Task.params` migrated from `Vec<String>` to `Vec<ParamDef>`
- [x] `parser.rs` — `parse_schema_type()` handles `Reference` (identifier fallback) and `TypedList` (`list of <type>`)
- [x] `parser.rs` — `parse_param_list()` reads optional `as schema_name` per parameter
- [x] `openapi.rs` — `schema_type_to_openapi` refactored to return `Json` (supports `$ref` and `array/items`)
- [x] All existing constructions updated across `environment.rs`, `route_loader.rs`, `interpreter.rs`, `value.rs`
- [x] 7 new parser tests, 3 new OpenAPI tests; 449 lib tests passing, 0 warnings

#### Phase 2 — Recursive Validator
- [x] `validate_payload` signature gains `schemas: &HashMap<String, SchemaDefinition>` for cross-schema lookups
- [x] `validate_recursive` — internal recursive function with `path_prefix` and `depth` guard (`MAX_DEPTH = 20`)
- [x] `validate_field_type` — handles `Reference` (recursive nested validation), `TypedList` (per-element validation with indexed path)
- [x] Error paths are accumulative: `"billing.city is required"`, `"items[1].quantity is required"`
- [x] Unknown schema reference returns 422 with descriptive message
- [x] `server.rs` updated to pass `&schemas` to `validate_payload`
- [x] 10 new validator tests; 459 lib tests passing, 0 warnings

#### Phase 3 — Task Contract Enforcement
- [x] `interpreter.rs` — before binding params to scope, iterates `ParamDef.schema` and calls `validate_payload` for each bound argument
- [x] Validation failure re-wrapped as `MarretaError::TypeError` with message naming task, parameter, and schema
- [x] Unknown schema reference also yields `TypeError`
- [x] `TypeError` propagates to `error_to_response` catch-all → HTTP 500
- [x] Unbound parameters (`schema: None`) pass through without validation
- [x] 6 new interpreter tests; 465 lib tests passing, 0 warnings

#### Phase 4 — Circular Reference Detection
- [x] `MarretaError::CircularSchemaReference { cycle }` variant added with descriptive Display
- [x] `detect_circular_references()` — DFS over schema reference graph at load time
- [x] `dfs_schema()` — grey/black node tracking; detects direct cycles, two-schema cycles, N-schema chains, and `TypedList(Reference)` cycles
- [x] `collect_schema_refs()` — recurses into `Reference` and `TypedList` field types
- [x] Server does not start if a cycle is detected
- [x] References to unknown schemas allowed at load time (caught at validation time with 422)
- [x] 8 new tests (7 route_loader + 1 error display); 473 lib tests passing, 0 warnings

#### Phase 5 — Ecommerce Example Updated
- [x] `schemas/payloads.marreta` — `address`, `order_item` schemas added; `order_payload` uses `billing: address` and `items: list of order_item`; `order_created` updated with all response fields
- [x] `tasks/pricing.marreta` — `get_coupon_rate(order as order_payload)` task with contract; `apply_discount` and `calculate_total` retained
- [x] `routes/orders.marreta` — uses `get_coupon_rate(payload)` (task contract), `payload.items.length()`, `payload.billing.city`; `reply 201 as order_created`
- [x] Functional: `POST /orders` valid → `{"city":...,"coupon":"SAVE10","discount_rate":0.1,"item_count":2,"order_created":true}` ✓
- [x] Functional: missing `billing.city` → `{"error":"field 'billing.city' is required"}` ✓
- [x] Functional: item missing `quantity` → `{"error":"field 'items[1].quantity' is required"}` ✓
- [x] OpenAPI: `order_payload` shows nested `$ref` and `array/items` structure ✓
- [x] OpenAPI: `POST /orders` responses include 201, 400, 422 ✓
- [x] 473 lib tests passing, 0 warnings

### v0.4.0 Acceptance Criteria
- [x] `billing: address` (schema reference) works end-to-end: payload validated recursively, error paths include parent field name
- [x] `items: list of order_item` (typed list) works end-to-end: each element validated, error paths include index (`items[1].quantity`)
- [x] `task get_coupon_rate(order as order_payload)` enforces contract: wrong shape → HTTP 500 TypeError
- [x] Circular schema references detected at startup — server does not start; descriptive cycle path in error message
- [x] `validate_payload` uses schema registry for cross-schema lookup (nested and list references)
- [x] OpenAPI `components/schemas` emits `$ref` for referenced schemas and `array/items` for typed lists
- [x] `MAX_DEPTH = 20` guard prevents stack overflow on deep/complex schemas
- [x] All existing tests continue to pass (no regressions)
- [x] `examples/ecommerce/` exercises all v0.4.0 features and verified functionally end-to-end

---

## v0.3.3 — Response Schema (COMPLETE)

**Design locked:** 2026-03-26 (see `docs/spec/docs/spec/005_RESPONSE_SCHEMA_033.md`)

### Design Decisions

- `reply 201 as order_result, result` — binds a response schema to a reply statement
- Schemas are schemas — no distinction in syntax between request and response schemas; same `schema` declaration, no naming convention imposed
- Schema serializes the response map: undeclared fields stripped, missing optional fields omitted, missing required fields → `null`
- OpenAPI `responses` populated with `content.$ref` to the schema component
- `examples/ecommerce/` updated in v0.3.3 to use response schemas (criterion 5 — must remain functional)

### 2026-03-26 — Session 4 (v0.3.3)

#### Phase 1 — Parser
- [x] `Statement::Reply` gains `response_schema: Option<String>`
- [x] `parse_reply()` reads optional `as schema_name` between status code and comma
- [x] All existing `Statement::Reply` constructions updated with `response_schema: None`
- [x] 3 new parser tests; 521 tests passing, 0 warnings

#### Phase 2 — Response Serializer
- [x] `src/response_serializer.rs` — `serialize(value, schema)`:
  - Declared fields present in value → included as-is
  - Required fields absent → `Value::Null`
  - Optional fields absent → omitted
  - Undeclared fields → stripped
  - Non-map values → pass-through unchanged
- [x] 8 unit tests; 529 tests passing, 0 warnings

#### Phase 3 — Interpreter + Server Integration
- [x] `Interpreter` gains `schemas: Option<Arc<HashMap<String, SchemaDefinition>>>` field
- [x] `with_schemas()` builder injects schema registry into interpreter
- [x] `from_environment()` correctly initializes `schemas: None`
- [x] `Statement::Reply` arm destructures `response_schema` and applies `response_serializer::serialize` when schema name is set and registry is available
- [x] `server.rs` passes `Arc::clone(&schemas)` to every per-request interpreter via `with_schemas()`
- [x] 433 lib tests passing, 0 warnings

#### Phase 4 — OpenAPI Response Codes + Response Schema $ref
- [x] `openapi.rs` bumped to v0.3.3
- [x] `collect_responses()` — shallow AST scan of route body for `Reply`, `Fail`, `Require`, `Reject` statements; deduplicates by status code
- [x] `status_description()` — human-readable descriptions for common HTTP status codes
- [x] `build_operation()` builds `responses` map from AST instead of hardcoding `"200": "Success"`
- [x] `reply … as schema_name` adds `content.$ref` to the response entry
- [x] Schema-bound routes with payload binding always include `422 Unprocessable Entity`
- [x] Fallback to `"200": "Success"` when route body has no reply/fail statements
- [x] 6 new OpenAPI tests; 439 lib tests passing, 0 warnings

#### Phase 5 — Ecommerce Example Updated
- [x] `schemas/payloads.marreta` — added `product_created` and `order_created` response schemas
- [x] `routes/products.marreta` — `reply 201 as product_created, ...`
- [x] `routes/orders.marreta` — `reply 201 as order_created, ...`
- [x] Functional verification: POST /orders and POST /products return correctly serialized responses
- [x] OpenAPI shows `$ref` to response schemas with 201, 400, 422 status codes
- [x] 439 lib tests passing, 0 warnings

### v0.3.3 Acceptance Criteria
- [x] `reply CODE as schema_name, expr` syntax works end-to-end
- [x] Response serializer strips undeclared fields, nulls required missing, omits optional missing
- [x] Schemas are schemas — same `schema` declaration for request and response, no naming convention imposed
- [x] OpenAPI shows correct status codes derived from AST (not hardcoded 200)
- [x] OpenAPI shows response schema `$ref` when `reply … as schema_name` is used
- [x] Swagger UI shows 422 for schema-validated routes
- [x] `examples/ecommerce/` updated to use response schemas and remains functional

---

## v0.3.2 — Multi-file Support (COMPLETE)

**Design locked:** 2026-03-26 (see `docs/spec/docs/spec/004_MULTIFILE_032.md`)

### Design Decisions

- `marreta serve` auto-scans all `.marreta` files in the directory tree from `./app.marreta` by convention — no imports, no includes
- All symbols are **file-private by default** — no cross-file collisions
- `export` keyword makes a symbol (variable, task, schema) globally available
- `app.marreta` entrypoint is implicitly global — no `export` needed there
- Two-pass loading: Pass 1 collects exports → global scope; Pass 2 executes routes
- Export name conflicts detected at startup with a descriptive error

### 2026-03-26 — Session 4 (v0.3.2)

#### Phase 1 — Lexer & AST
- [x] `TokenKind::Export` added to `token.rs` and `keyword_lookup()`
- [x] `Statement::Export(Box<Statement>)` added to `ast.rs`
- [x] `parse_export()` in `parser.rs` — wraps task/schema/assignment; `export route` → parse error
- [x] `interpreter.rs` — `Statement::Export` delegates execution to inner statement
- [x] `route_loader.rs` — exported schemas registered in `RouteRegistry.schemas`
- [x] 5 new parser tests; 0 clippy warnings; 505 tests passing

#### Phase 2 — Multi-file Loader
- [x] `MarretaError::ExportConflict` — startup error when two files export the same name
- [x] `src/file_loader.rs` — `load_project(entrypoint)`: file scanning + two-pass loading
  - Pass 1: non-entrypoint files — only `export` symbols collected; file-private discarded
  - Pass 2: entrypoint (`app.marreta`) — all symbols are implicitly global
  - Route conflict detection across files
  - `source_file` tag set per route from file stem
- [x] `route_loader::path_pattern` made public (used by file_loader conflict check)
- [x] `main.rs` updated to call `file_loader::load_project()` instead of `route_loader::load()`
- [x] 8 new file_loader tests (single-file, multi-file merge, export, conflict, privacy); 513 tests passing, 0 warnings

#### Phase 3 — Scope Isolation
- [x] `ExportConflict` display test added to `error.rs`
- [x] `start_project_server` helper in HTTP integration tests (uses `file_loader::load_project` + TempDir)
- [x] 4 new HTTP integration tests proving scope isolation criteria:
  - exported task callable from route in another file
  - routes from separate files both respond
  - entrypoint variables accessible at request time
  - exported schema validates route in another file (valid → 201, invalid → 422)
- [x] 518 tests passing, 0 warnings

#### Phase 4 — E-commerce Example (Criterion 8)
- [x] `examples/ecommerce/` — functional multi-file project demonstrating all v0.3.2 features:
  - `app.marreta` — entrypoint: project_name, project_version, GET /health
  - `schemas/payloads.marreta` — `export schema product_payload`, `export schema order_payload`
  - `tasks/pricing.marreta` — `export task apply_discount`, `export task calculate_total`
  - `routes/products.marreta` — GET /products, POST /products (schema validated)
  - `routes/orders.marreta` — POST /orders (schema + tasks from other files)
- [x] Functionally tested: health, list, create, schema validation (422), discount math

---

## v0.3.1 — Schema & AutoDoc

### Planned Features (see docs/spec/003_SCHEMA_031.md)

| # | Feature | Status |
|---|---|---|
| 1 | Lexer/AST: `schema`, `as`, type keywords | COMPLETE |
| 2 | Schema validator (HTTP 422) | COMPLETE |
| 3 | OpenAPI 3.0 generation + Swagger UI | COMPLETE |
| 4 | `project_name`/`project_version` from startup env | COMPLETE |
| 5 | `MARRETA_DOCS_ENABLED` / `MARRETA_DOCS_PATH` config | COMPLETE |

### 2026-03-26 — Session 3 (v0.3.1)

- [x] Phase 1: `schema` declaration, `as` binding, `SchemaType`, `SchemaField`, `RouteRegistry.schemas`
- [x] Phase 2: `validator.rs` — validates payload against schema, returns 422 on violation
- [x] Phase 3: `openapi.rs` — OpenAPI 3.0 JSON built at startup; `/openapi.json` + `/docs` (Swagger UI)
- [x] `project_name` / `project_version` variables from startup env used as OpenAPI title/version
- [x] Swagger UI tag derived from route file name stem (e.g. `schema_test.marreta` → "Schema Test"), removes "default" grouping
- [x] `MARRETA_DOCS_ENABLED` / `MARRETA_DOCS_PATH` in `config.rs`
- [x] `CONVENTIONS.md` created — snake_case naming guide
- [x] `RouteDefinition.source_file` — file stem stored per route, used as OpenAPI tag
- [x] Docs reorganized: spec and implementation plans moved to `docs/spec/`
- [x] 500 tests passing (408 unit + 27 HTTP + 65 CLI)

---

## v0.11 — Iteration & Accumulation

- [x] `range(n)` and `range(start, end)` built-ins with inclusive bounds and empty-list fallback when `start > end`
- [x] `>> reduce(INITIAL) acc, item` pipeline stage with `TaskBody`-style implicit return
- [x] `while CONDITION` statement with a hard safety limit of 10,000 iterations
- [x] Task recursion and mutual recursion supported with `MARRETA_MAX_RECURSION_DEPTH` guard (default 500)
- [x] New list methods: `sum()`, `mean()`, `median()`, `std_dev()`, `zip()`
- [x] New scalar conversions: `to_integer()`, `to_float()`, `to_boolean()`, `to_string()`
- [x] Added unit/integration coverage for all new runtime features
- [x] Added functional routes and `test.sh` coverage for iteration and accumulation scenarios

## v0.2.1 — HTTP Expansion

### Planned Features (see docs/spec/002a_HTTP_021.md)

| # | Feature | Status |
|---|---|---|
| 1 | 204 No Content body fix | COMPLETE |
| 2 | CORS middleware (tower-http) | COMPLETE |
| 3 | `env` object (dotenvy + OS vars) | COMPLETE |
| 4 | Multiple `take` bindings (comma-separated) | COMPLETE |
| 5 | `take form` / `take raw` request types | COMPLETE |
| 6 | Response modifiers (`reply html`, `reply text`, extra headers) | COMPLETE |

### 2026-03-26 — Session 2 (v0.2.1)

#### v0.2.1 — All 6 Features

- [x] **Feature 6 — 204 fix**: `reply 204, null` returns 0-byte body (RFC 9110 §15.3.5); 304 also excluded
- [x] **Feature 5 — CORS**: `tower-http` `CorsLayer` applied globally; controlled via `MARRETA_CORS` / `MARRETA_CORS_ORIGIN` in `marreta.env`; `ServerConfig` and `MarretaConfig` extended
- [x] **Feature 2 — env object**: `dotenvy` loads `marreta.env` into `std::env`; all OS env vars injected as `Value::Map` named `env` at server startup; accessible in every route as `env.KEY`
- [x] **Feature 1 — Multiple take**: `Route.take` changed from `Option<TakeBinding>` → `Vec<TakeBinding>`; comma-separated syntax: `take payload, headers`; all existing tests migrated
- [x] **Feature 3 — form/raw types**: `TakeBinding::Form` (serde_urlencoded) and `TakeBinding::Raw` (bytes as String); handler switched from `Option<Json<JsonValue>>` to `Bytes`
- [x] **Feature 4 — Response modifiers**: `ReplyContentType { Json, Html, Text }` enum; `reply html CODE, "..."` / `reply text CODE, "..."`; optional 3rd arg for extra headers (`reply 302, null, { Location: "..." }`) ; `MarretaError::HttpResponse` extended with `content_type` and `extra_headers`
- [x] New dependencies: `tower-http = "0.6"` (cors), `dotenvy = "0.15"`, `serde_urlencoded = "0.7"`
- [x] `examples/http_hello.marreta` expanded to 17 routes covering all features
- [x] 29 new unit tests (ast, parser) + 8 new HTTP integration tests
- [x] 464 tests — all passing (372 unit + 92 integration: 65 CLI + 27 HTTP)

---

## v0.2 — HTTP Runtime

### Planned Phases (see docs/spec/002_HTTP.md)

| # | Phase | Status |
|---|---|---|
| 1 | Dependencies (axum, tokio, serde_json) | COMPLETE |
| 2 | New AST nodes (Route, Reply, Fail) | COMPLETE |
| 3 | Lexer & Parser extensions | COMPLETE |
| 4 | HTTP value types (value_to_json, json_to_value) | COMPLETE |
| 5 | Route Registry | COMPLETE |
| 6 | HTTP Server (axum) | COMPLETE |
| 7 | Request binding (take) | COMPLETE |
| 8 | Response (reply / fail) | COMPLETE |
| 9 | `marreta serve` CLI command | COMPLETE |
| 10 | `marreta.env` configuration | COMPLETE |
| 11 | Tests | COMPLETE |

### 2026-03-26 — Session 2 (continued)

#### v0.2 Phases 1–8
- [x] `axum 0.8`, `tokio 1`, `serde_json 1`, `tower 0.5` added to Cargo.toml (latest stable)
- [x] `reqwest 0.13` added to dev-dependencies for integration tests
- [x] `ast.rs` — `Statement::Route`, `Statement::Reply`, `Statement::Fail`, `HttpVerb`, `TakeBinding`
- [x] `parser.rs` — `parse_route()`, `parse_reply()`, `parse_fail()`
- [x] `error.rs` — `MarretaError::HttpResponse` (control flow), `MarretaError::RouteConflict`
- [x] `value.rs` — `value_to_json()`, `json_to_value()` helpers
- [x] **`Rc<RefCell>` → `Arc<RwLock>`** — `Value::Map` is now thread-safe (required for axum handlers); `RwLock` chosen over `Mutex` for parallel read performance
- [x] `Environment` derives `Clone` (required for per-request isolation)
- [x] `interpreter.rs` — `Reply`/`Fail` statements handled; `from_environment()`, `into_environment()`, `env_set()` added
- [x] `route_loader.rs` — `RouteRegistry`, `load()`, `path_pattern()`, conflict detection
- [x] `server.rs` — axum server, route registration, `execute_route()`, request binding
- [x] 363 tests — all passing (298 unit + 65 integration)

#### Unit tests for v0.2 phases 1–8
- [x] `ast.rs` — 8 new tests: HttpVerb display/equality, TakeBinding variants, Route/Reply/Fail node construction
- [x] `error.rs` — 4 new tests: HttpResponse display (reply/fail), RouteConflict display, body_json preserved
- [x] `parser.rs` — 16 new tests: all HTTP verbs, take payload/query/headers, reply/fail with various bodies, route with multi-statement body, route with require, invalid verb error
- [x] `interpreter.rs` — 9 new tests: reply/fail emit HttpResponse, body serialized correctly, execution terminates on reply/fail
- [x] `value.rs` — 16 new tests: value_to_json for all types (int/float/string/bool/null/list/map/nested), json_to_value for all types, roundtrip test
- [x] 416 tests — all passing (351 unit + 65 integration)

#### v0.2 Phase 9 — `marreta serve` CLI command
- [x] `serve` command added to `main.rs` dispatch: `marreta serve <file> [--port PORT]`
- [x] `run_serve()`: tokenize → parse → `route_loader::load()` → execute startup stmts → `Arc<Environment>` → `server::serve()`
- [x] `parse_port_arg()`: reads `--port N` from CLI args
- [x] `tokio::runtime::Runtime::new().block_on(serve(...))` — runs async server from sync main
- [x] Default port: 8080; host: 0.0.0.0
- [x] Version bumped to v0.2.0 in all CLI output
- [x] `examples/http_hello.marreta` — 4-route example (GET /hello, GET /greet/:name, POST /echo, GET /health)
- [x] 416 tests — all passing (no regressions)
- [x] **Bugfix**: axum 0.8 requires `{param}` syntax — added `to_axum_path()` in `server.rs` to convert `:param` → `{param}` at registration; internal representation unchanged
- [x] Functional smoke test passed: GET /hello, GET /greet/:name, GET /health, POST /echo all returning correct JSON

#### v0.2 Phase 10 — `marreta.env` Configuration
- [x] `config.rs` — `MarretaConfig { host, port }` with `load()` and `with_port()`
- [x] `load()` priority: `marreta.env` file → env vars (`MARRETA_HOST`/`MARRETA_PORT`) → defaults (`0.0.0.0:8080`)
- [x] `read_env_file()`: parses `KEY=VALUE`, strips comments (`#`), blank lines, inline comments, surrounding quotes
- [x] `--port` CLI flag (highest priority) applied via `with_port()`
- [x] `marreta.env.example` — example config file
- [x] `tempfile` added to dev-dependencies for file-based tests
- [x] 8 unit tests for `config.rs`
- [x] 424 tests — all passing (359 unit + 65 integration)

#### v0.2 Phase 11 — HTTP Integration Tests
- [x] `tests/http_integration_tests.rs` — 19 HTTP scenarios with real axum server + reqwest
- [x] `start_test_server()` helper: parse → route_loader → startup → bind random port → spawn tokio thread
- [x] Scenarios: GET/POST, URL params, take payload/query/headers, require/reject/fail guards, task calls, match, pipeline, `or` default, 204 no-reply, null body, startup constants, RouteConflict at boot, literal+param coexistence
- [x] Note: header names with hyphens (e.g. `x-custom`) not accessible via property access (Marreta Lang identifiers don't support hyphens) — documented in test
- [x] 443 tests — all passing (359 unit + 84 integration: 65 CLI + 19 HTTP)

---

## Implementation Phases Overview

| # | Phase | Status |
|---|---|---|
| 1 | Scaffold | COMPLETE |
| 2 | token.rs | COMPLETE |
| 3 | error.rs | COMPLETE |
| 4 | ast.rs | COMPLETE |
| 5 | value.rs | COMPLETE |
| 6 | environment.rs | COMPLETE |
| 7 | lexer.rs | COMPLETE |
| 8 | parser.rs | COMPLETE |
| 9 | interpreter.rs | COMPLETE |
| 10 | main.rs (CLI + REPL) | COMPLETE |
| 11 | Unit tests | COMPLETE |
| 12 | Integration tests | COMPLETE |
| 13 | Polish (clippy, fmt, errors) | COMPLETE |
