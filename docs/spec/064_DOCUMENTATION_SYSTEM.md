# 064 - Documentation System (`docs/guide`, authored Diataxis tree)

> Status: Delivered
> Type: Documentation
> Scope: Establish a single Markdown documentation tree (`docs/guide/`) that serves both
> GitHub and (later) the site, organized by the Diataxis model and authored by hand. A
> bootstrap generator was used once to produce the first complete reference and was then
> removed from the repo. Coverage is kept current by process (PR checklist + the
> marreta-spec delivery step), not by an automated gate. The site (Spec 065) renders this
> same tree and is out of scope here.

---

## 1. Purpose

The project needs real documentation, for GitHub and to feed the future site, and writing
it twice is wasteful. The hard requirement is coverage: everything must be documented,
meaning the language, the runtime, every `MARRETA_*` variable (purpose and default),
install, quickstart, the editor, the CLI, and errors.

To guarantee the first version captured everything, a one-off generator derived the
reference from the authoritative in-repo inventories (the catalog, `SchemaType`,
`ErrorCode`, the CLI dispatch, the classified env vars). That generator was bootstrap
scaffolding only. It is **not** shipped: there is no `src/docs.rs`, no
`marreta tooling docs` command, and no docs CI gate in the repo. Keeping a generator in
the tree would confuse the community and the generated tables read as robotic.

From now on the docs are **authored by hand** and kept complete by process. See the
decision history in §6.

## 2. Coverage inventory (what must always be documented)

These are the authoritative in-repo sources an author consults to confirm coverage. They
were the generator's inputs and remain the manual checklist:

| Surface | Source of truth |
|---|---|
| Namespaces, methods, functions, keywords | the catalog (`marreta tooling catalog --format json`) |
| Schema types | `SchemaType` (ast.rs) |
| Config / env | `MARRETA_*` across the runtime (greppable with `rg MARRETA_`), classified per §2.1 |
| CLI commands | `main.rs` dispatch (serve, test, doctor, init, migrate + subcommands, fmt, lint, tooling) |
| Error codes | `ErrorCode` (error.rs) |
| Language features | `docs/spec/` (design record, the source for user docs, not user docs themselves) |

### 2.1 Env-var classification

A flat `grep MARRETA_` is not a usable inventory. The hits span three kinds:

- **Fixed runtime vars** are concrete keys the runtime reads (`MARRETA_PORT`,
  `MARRETA_DB_PROVIDER`, the pool/connection keys, `MARRETA_LOG_LEVEL`, and so on). These
  are the documented inventory, each with purpose, default, required-when, and provider.
- **Dynamic pattern vars** are runtime-owned but with a user-chosen suffix, such as
  `MARRETA_FEATURE_<NAME>`. The reference documents the pattern, never each instance.
- **User-defined envs** are arbitrary `env.*` values read by project source (for example
  auth). They are not runtime-owned and are documented once as a capability under
  how-to/auth, not enumerated.

## 3. Structure, Diataxis under `docs/guide/`

Four modes, each a folder, one page per `.md`:

- **`tutorials/`** are learning-oriented and step-by-step (install, quickstart, first API,
  persistence).
- **`how-to/`** are task-oriented recipes (validate a payload, persist with local services
  and migrations, use cache, publish to a topic, authenticate, deploy with Docker).
- **`reference/`** is information-oriented and exhaustive: the `marreta.env` table, one
  page per namespace, schema types, one page per CLI command, error codes, and the
  versioning policy (Spec 063).
- **`concepts/`** is understanding-oriented (the provider abstraction today; persistence by
  convention, file-namespaces, the "no imports" tenet, and the runtime model are candidates
  as the section grows). A separate, broader `explanation/` mode is deferred.

Each page carries front-matter (`title`, `category`, `slug`, `summary`) and the tree is
indexed by a single `docs/guide/SUMMARY.md` (mdBook-style, GitHub-friendly). The site
(Spec 065) consumes the same front-matter and `SUMMARY.md`.

## 4. Authoring rules

`docs/STYLE.md` is the informal style guide. The load-bearing rules:

- **Voice** is concrete and warm, never marketing. Lead with a runnable example, then
  explain.
- **Punctuation** uses no em-dashes, no semicolons, and no emojis.
- **Examples must be real.** Every code example is lifted from, or verified against, a
  tested project under `docs/examples`. A documented example that does not run is a
  defect. A `db.` example must include the `marreta migrate generate` and
  `marreta migrate apply` steps, or the table will not exist.
- **Reference enrichment** is ongoing: the bootstrap-generated reference pages are factual
  but dry, and each needs a textual pass (intro, "When to use", gotchas, real examples).

## 5. Keeping coverage current (process, not a gate)

There is no automated completeness gate. Coverage is enforced by process:

1. The marreta-spec delivery checklist adds a **Documentation** axis to the per-spec
   coverage analysis (alongside VS Code extension and e2e): any added or changed surface
   updates `docs/guide`.
2. `.github/PULL_REQUEST_TEMPLATE.md` carries the docs checklist, so a reviewer cannot
   approve a behavior change that did not update the docs.
3. `SPEC.md section 1.3` records the docs DoD axis.

Adding a new namespace, variable, command, or error therefore forces a docs update through
review discipline rather than a CI check.

## 6. Decision history

- The first proposal (approved) shipped a generator plus a CI completeness/freshness gate
  (the "documentation guardian"). It was implemented and used to produce the first complete
  reference tree.
- The direction then changed: the user decided not to keep any automated docs mechanism in
  the repo. The generator and gate were removed (`src/docs.rs`, `tests/docs_gate.rs`, the
  `marreta tooling docs` command, the `pub mod docs`), and the generated pages were stripped
  of their generation markers and `TODO(prose)` stubs to become plain authored content. The
  guarantee moved from a gate to the PR checklist and the marreta-spec delivery step.

## 7. Out of scope

- **The site** (rendering, identity, search, navigation UI) is a separate spec (065) that
  consumes this tree.
- **Versioned docs** (per-release snapshots) revisit post-1.0.
- **Any automated docs generator or completeness gate.** Explicitly not shipped.
- **A dedicated `explanation/` mode** (understanding-oriented deep-dives beyond the current
  `concepts/` page) is deferred; `concepts/` covers the provider model for now.
- Translations and i18n.

## 8. Acceptance criteria

1. `docs/guide/` exists with its Diataxis folders (`tutorials/`, `how-to/`, `reference/`,
   `concepts/`) and a `SUMMARY.md` index, and pages carry the front-matter schema.
2. The repo contains no docs generator and no docs gate: no `src/docs.rs`, no
   `marreta tooling docs`, no `tests/docs_gate.rs`. The reference pages are plain authored
   Markdown with no generation markers.
3. The reference covers every catalog namespace (one page each) with its methods, the
   schema types, every public CLI command, every error code, and the classified env vars
   (fixed vars with purpose/default/required-when/provider, each pattern documented once,
   user-defined `env.*` excluded).
4. Tutorials, how-to, and concepts exist for the core surfaces (quickstart, persistence
   with and without migrations, validation, response shaping, caching, external calls,
   messaging, security, error handling, and the provider model), curated and reviewed, with
   every example verified against a tested project.
5. Authoring rules live in `docs/STYLE.md`, and coverage enforcement lives in the PR
   template and the marreta-spec delivery checklist.
6. The tree is renderable by a Starlight-style site from front-matter and `SUMMARY.md` (no
   GitHub-only constructs), verified structurally, not by building the site.
7. Standard gates green; `SPEC.md section 1.3` records the docs DoD axis.

---

## 9. Delivery notes

Delivered on the `feature/documentation-system-064` branch:

- `64b2b4b` — bootstrap generator and completeness gate (mechanical first pass, later removed).
- `e642650` — authored the Diataxis guide and removed the bootstrap generator and gate.
- `3fba332` — event-driven tutorial, ack/nack and BDD how-to content, and the
  `topic.publish` scenario-mock parity fix (`src/scenario_tests.rs`) with a regression
  scenario (`functional_tests/tests/queue/publish_test.marreta`).
- `7a22260` — enriched the reference and restructured `reference/methods/` into
  `reference/types/` (each page covers a type and its methods), with full per-variable
  configuration tables.

The `explanation/` Diataxis mode was not shipped; `concepts/` (the provider model) covers
the understanding-oriented need for now. Gates green: fmt, clippy (`-D warnings`), suite
1515 + 3 + 38 + 37; `functional_tests` 566/566; `migrations_functional` PASS; `e2e`
scenarios + 18 live smoke.

---

## P.S. Do not forget the docs of record

On delivery, update both `CHANGELOG.md` and `docs/spec/SPEC.md`. See SPEC.md section 1.3.
