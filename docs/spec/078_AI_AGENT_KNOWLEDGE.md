# 078 - AI Agent Knowledge (generated primer and llms.txt)

> Status: Delivered
> Type: Tooling (CLI generation + doctor) + docs/site, no language runtime change
> Scope: Teach AI coding assistants what Marreta is, so a developer in the AI era (Copilot, Claude,
> Cursor, Antigravity, etc.) gets correct Marreta from their model instead of hallucinated or
> Python/Ruby-shaped code. Two generated artifacts, both rendered from a single source and never
> hand-maintained: a project-level `AGENTS.md` primer scaffolded by `marreta init`, and
> `llms.txt` / `llms-full.txt` served at the site root. Plus a freshness check in `doctor` and a
> command to regenerate the primer. The Marreta MCP server is a separate fast-follow (Spec B), out
> of scope here.

---

## 1. Purpose

Marreta is designed for excellent human developer experience, but in the current era a developer
rarely writes the code by hand. They ask the model in their IDE. Those models do not know Marreta (a
new, niche language), so they hallucinate syntax or fall back to a Python/Ruby that "looks close".
For a new language this is an adoption barrier of the first order, arguably larger than any item of
human DX: a great language the AI cannot write is a bad AI-era experience.

This spec closes that gap by putting authoritative, correct Marreta knowledge where models already
look. It does NOT change the language. Everything it ships is generated from sources that are already
verified (the catalog, the authored guide, the tested examples), so the knowledge cannot drift from
the language as it evolves.

Two complementary channels, by where the agent works:

- **In the developer's project** (the agent editing their repo): a scaffolded `AGENTS.md`.
- **On the web** (an agent fetching docs at query time): `llms.txt` / `llms-full.txt` at the site
  root.

## 2. The change

### 2.1 `AGENTS.md` scaffolded by `marreta init`

`marreta init` writes an `AGENTS.md` at the project root. `AGENTS.md` is the cross-tool convention
that agentic assistants load as in-context instructions; the agent reads it as context, it does not
reliably fetch URLs on its own. So the file is **offline-first for the stable core, with a pointer to
the live full reference**, not a pure pointer (a pure pointer fails silently when the tool cannot
fetch, which is the worst case) and not a full dump (token budget plus staleness).

Content principle (the highest-leverage decision): **the primer is a diff against the model's priors,
not a tutorial.** The model already knows how to program; it needs the Marreta-specific corrections.
The first screen is "the things models get wrong in Marreta" (the delta from "do not write it like
Python/Ruby"), because that is where injection pays off most. Lead with the delta, then a compact
core-syntax cheat, then a pointer.

Shape (the approved first-screen content; at build, every code block is region-extracted from a
tested example, and `vX.Y.Z` is injected at emission, so only the narrative lines are hand-kept):

```
# Marreta Lang — agent guide   (Generated for Marreta vX.Y.Z)
> Full, live reference: https://marreta.dev/llms-full.txt

Marreta is a DSL for REST APIs. It is not Python, Ruby, or JavaScript. Write Marreta, not a framework.

## Get this right (do not write it like Python/Ruby/JS)

1. No imports, no framework, no app object. A route is a top-level declaration, its body is indented.
       route GET "/accounts/:id"
           reply 200, account

2. The request is exposed as params, query, payload, headers, and auth, accessed directly. There is
   no req/request/ctx object and no request.json().
       account = doc.accounts.find(params.id)
       owner = auth.subject

3. Validate input with a schema and take payload as. An invalid body returns 422 automatically.
   Do not parse or validate by hand, and do not import a validator.
       schema NewAccount
           owner: string
       route POST "/accounts" take payload as NewAccount
           account = doc.accounts.save({ owner: payload.owner })
           reply 201, account

4. Infrastructure is built-in namespaces (db. doc. cache. queue. topic. http_client.), configured in
   marreta.env. Never construct a client or a connection in code.
       cache.set("rate:usd", 5)

5. Respond with reply STATUS, body. Guard with require X else fail STATUS, "msg".
   There is no return, no raise, no res.json.
       require account else fail 404, "account not found"
       reply 200, account

6. Queries are pipelines with >>, not chained methods.
       product = db.products >> where(sku: params.sku) >> fetch_one

7. Types are keywords. A collection is list of X (not [X] or List[X]), an optional field is name?: type.
       schema Search
           term: string
           tags: list of string
           limit?: integer

8. Tests live in the language: scenario / given / when / then, run in memory. No pytest, no jest.
       scenario "returns an account"
           given doc.accounts.find("a1") returns { owner: "Ana" }
           when GET "/accounts/a1"
           then status 200

## Core syntax (cheat)
route VERB "path" [take payload as Schema] · reply STATUS, body · fail STATUS, "msg" · require COND else ...
schema Name / field: type / field?: type / list of type · match X / if / else · and or not
db. doc. cache. queue. topic. http_client. · pipelines with >> (where, order, fetch, fetch_one)

## When you need more
Anything not here, or version-specific (full namespace/method list, OpenAPI, auth, migrations):
fetch https://marreta.dev/llms-full.txt
```

- **Emitted set (canonical plus thin pointers).** `marreta init` writes exactly: the canonical
  `AGENTS.md`, plus `.github/copilot-instructions.md` and `.cursor/rules/marreta.mdc` as **thin
  pointers** to it where the tool honors a pointer/include (a full render only where a tool
  demonstrably does not follow a pointer). Because every variant is rendered from one source, a
  "full" variant is a render, not a hand-kept copy, so there is no drift. This exact set is what
  `--no-agents` skips and what the init-fixture parity (2.7) covers, not the `AGENTS.md` alone.
- **Version stamp, injected at emission (not baked into the asset).** The committed and baked primer
  is **version-neutral** (a placeholder line, no version). `init` and the regenerate command write
  the stamp at emission time from the binary's own version (`env!("CARGO_PKG_VERSION")`), producing
  the `Generated for Marreta vX.Y.Z` header the developer sees, the basis for the freshness check
  (2.4) and the agent's cue to fetch the live reference when the runtime is newer. Keeping the version
  out of the committed asset is what keeps the codegen git-diff gate stable across version bumps and
  dev builds (2.8) and lets the web initializer inject the same stamp with the fixture normalizing the
  stamp line (2.7).
- **Opt-out.** `marreta init --no-agents` skips the whole emitted set. The default is on
  (zero-ceremony), but AI scaffolding is not forced on a developer who does not want the files in
  their repo, and the opt-out is documented.

### 2.2 `llms.txt` and `llms-full.txt` at the site root

Generated in the language repo and synced to the site web root (so `marreta.dev/llms.txt` resolves,
per the convention), the same single-source-then-mirror pattern as `docs/guide` (see 2.5).

- **`llms.txt`** is the curated index: an H1, a one-paragraph project summary, then the guide's
  sections and links with a one-line description each. This is almost a direct transform of
  `docs/guide/SUMMARY.md`, with each link's description taken from that page's frontmatter `summary:`
  field, so nothing is hand-written.
- **`llms-full.txt`** is the whole guide concatenated into one plain-markdown file, in `SUMMARY.md`
  order, so an agent ingests the entire reference in one fetch without crawling.

### 2.3 Single-source generation (the anti-drift and anti-obsolescence core)

Every artifact above is a render of sources that are already verified. Three layers, by reliability:

1. **`catalog_json()`** — the language surface (namespaces, operations), mechanically verified. Used
   to drive and to CHECK completeness (2.6).
2. **`docs/guide` via `SUMMARY.md` + page frontmatter** — structure, prose, and the per-link
   descriptions. The backbone of `llms.txt`/`llms-full.txt` and the reference part of `AGENTS.md`.
3. **`e2e` / `docs/examples`** — the tested snippets. **Every code block in `AGENTS.md` is extracted
   from a tested example by a named region marker (an anchor comment in the example file), never typed
   into the primer.** Provenance is by construction, not an after-the-fact text search: the generator
   pulls each snippet from a marked region, so a snippet cannot drift (change the syntax, the example
   breaks in CI, the primer re-renders), and the gate (2.6) asserts every `AGENTS.md` block resolves
   to an existing region. Each first-screen bullet (2.1) points to a real example region.

The only hand-written content is the short "diff against priors" narrative (2.1). It is the one layer
the completeness gate cannot verify on its own (semantic drift, for example "use `take payload` for a
query param" is syntactically valid and semantically wrong). It is therefore kept minimal and each
claim anchored to an executable example. The spec calls this out as the surface that rots silently.

Generation runs in the language repo (where the sources live), so the anti-drift gate in CI covers
it. Because everything is single-source, re-targeting a new convention later (the `AGENTS.md`/llms
standards are still emerging) is a change of render, not a rewrite: single-source generation is both
anti-drift and anti-obsolescence.

**Generation is a build-time step, not a runtime one.** The primer does not depend on the project (it
is a language primer, identical for every project of a given version), so there is nothing to
generate per scaffold. The release build runs the generator once, runs the anti-drift gate, **bakes
the resulting version-neutral `AGENTS.md` into the binary as a static asset** (the version stamp is
injected at emission, not baked in, see 2.1), and emits `llms.txt`/`llms-full.txt` for the site. The runtime never runs the generator: `marreta init`,
the regenerate command, and the site initializer all just **emit the pre-built artifact**, exactly as
the initializer already bundles a ready file. This keeps the binary light (no docs, examples, or
generator shipped inside it, only the baked primer) and guarantees the three consumers emit identical
bytes.

### 2.4 `doctor` freshness check, read-only

`doctor` compares the project's `AGENTS.md` version stamp against the installed runtime and, when the
file is behind, warns and points to the regenerate command. It never rewrites the file (the developer
may have edited it), the same report-only discipline as Spec 067: report drift, never silently
overwrite a user's file.

### 2.5 A command to regenerate the primer

A CLI command rewrites the project's `AGENTS.md` (and any tool-specific pointers) from the primer
baked into the installed binary (it copies the pre-built artifact, it does not run the generator).
Idempotent, explicit (the developer ran it, so overwriting the generated content is expected). This is the developer-side lifecycle, distinct from
the maintainer-side `llms.txt` generation (which runs in the language repo CI and syncs to the site,
not something a developer runs). The command is `marreta agents` (see Design decisions).

### 2.6 Anti-drift gate

A CI test in the language repo, modeled on the existing catalog-to-docs lint test, asserting the
completeness chain **catalog ⊆ docs ⊆ llms-full** plus the codegen-freshness check:

- Every namespace/operation in `catalog_json()` appears in the generated `llms-full.txt`, so the full
  reference stays current as the language grows. (Completeness is asserted against `llms-full.txt`,
  **not** against the primer: `AGENTS.md` is a deliberately curated subset, the cheat, so catalog ⊆
  primer is not a property to assert. The primer's content selection is human-curated and reviewed at
  the gate; only its snippet provenance and stamp are machine-gated, see below.)
- Every `.md` under `docs/guide` is referenced by `SUMMARY.md` (no orphan page) **and has a non-empty
  `summary:` frontmatter**, so `llms.txt` is complete and no link degrades to an empty description.
- Every code block in `AGENTS.md` resolves to an existing region marker in a tested example (snippet
  provenance by construction, 2.3), so no hand-typed snippet can drift.
- Regenerating the committed artifacts leaves no diff (`git diff --exit-code`), the codegen-freshness
  check. Because the asset is version-neutral (2.1), this gate is stable across version bumps and dev
  builds.

### 2.7 Parity with the site initializer

The site has a web scaffold generator (`marreta.dev/initializer`, the site repo's own Spec 008) that
is a client-side reimplementation of `marreta init` (which is `038_PROJECT_INIT` in this repo). The
web generator is `src/lib/scaffold.mjs` in the **site repo**, with parity already enforced by
fixtures: `scripts/sync-init-fixtures.sh` (also in the site repo) captures the real `marreta init`
output as the ground-truth fixtures the client-side generator is tested against. So a project
scaffolded on the web must include the **same** emitted set (2.1) as the CLI, or web-scaffolded
projects ship without the primer.

This adds a **second sync to the site, distinct from `llms.txt`**:

- **`llms.txt` / `llms-full.txt`** are public, served at the site web root (2.2).
- **The generated `AGENTS.md` content** is synced to the site as a **private build asset** (not a
  public docs page), bundled by the initializer into the scaffold it generates. It is in the site
  host only so the initializer can emit it, exactly as the developer's intent: present for the
  generator, not exposed as documentation.

Parity is enforced by the mechanism that already exists: the init-fixture sync now captures
`AGENTS.md` too, and the `scaffold.mjs` test asserts the web generator emits the matching file. No
new parity machinery, the AGENTS.md just rides the fixture path that keeps web and CLI scaffolds
identical.

### 2.8 Generation, baking, and CI (no new workflow)

The artifacts are committed-codegen, not generated on the fly, so they are visible, reviewable, and
gated. The mechanics:

- **Generator (an `xtask`, not a runtime subcommand).** A workspace `xtask` that calls
  `catalog_json()`, reads `docs/guide` (`SUMMARY.md` + frontmatter) and the tested examples (by region
  marker), and writes the artifacts as **committed files** (e.g. `assets/agents/AGENTS.md`,
  `assets/llms/llms.txt`, `assets/llms/llms-full.txt`). Not a `build.rs`: `catalog_json()` lives in
  the crate, and `build.rs` runs before the crate compiles (chicken-and-egg), so a workspace tool
  that depends on the crate is the clean path. An internal `marreta` subcommand was rejected: it would
  embed the generator and the docs/example readers into the runtime binary, contradicting this
  section's lean-binary goal (only the baked primer ships, not the generator).
- **Baking.** The binary `include_str!`s the committed `AGENTS.md`, so it is baked at compile time.
- **Anti-drift gate (no new workflow).** A CI test regenerates and asserts the committed artifacts
  match (regenerate, then `git diff --exit-code`), the same spirit as `cargo fmt --check`. A change
  to the catalog, docs, or examples without regenerating fails CI. This rides the existing `build.yml`
  PR/push gate, so it needs no new workflow.
- **Site distribution.** Extend the existing sync scripts (no new workflow): `llms.txt`/
  `llms-full.txt` copied to the site web root (public), and the `AGENTS.md` asset to the site as a
  private bundle for the initializer, while `sync-init-fixtures.sh` captures `AGENTS.md` so the
  `scaffold.mjs` parity test covers it. This runs as the same manual site-sync step already used at
  delivery, then commit/push triggers the site's existing `deploy.yml`. A dedicated automation
  workflow is optional and only worth it if the site sync stops being manual (today it is manual by
  design).

## 3. Implementation outline

Tasks are tagged by repo, because the work spans both.

**Language repo (`marreta-lang`):**
- A build-time generator, an `xtask` (see Design decisions), that reads `catalog_json()` + `docs/guide`
  (`SUMMARY.md` + frontmatter) + the tested examples (by region marker) and emits the committed
  artifacts: the version-neutral `AGENTS.md` (baked into the binary via `include_str!`), `llms.txt`,
  and `llms-full.txt`. Gated by 2.6.
- Region markers added to the example files the primer pulls from (snippet provenance, 2.3).
- `marreta init`: emit the baked `AGENTS.md` plus the thin pointers (the 2.1 set), injecting the
  version stamp from `env!("CARGO_PKG_VERSION")`; `--no-agents` skips the whole set.
- The regenerate command (2.5), `doctor` read-only stamp check (2.4), and the anti-drift gate test
  (2.6), all riding `build.yml`.

**Site repo (`marreta-lang-site`):**
- A sync step (extending the existing sync scripts) that copies `llms.txt`/`llms-full.txt` to the web
  root (public) and the version-neutral `AGENTS.md` to a private asset for the initializer.
- `scripts/sync-init-fixtures.sh` extended to capture the `AGENTS.md` (and pointer) fixtures.
- `src/lib/scaffold.mjs` updated to emit the same set, injecting the stamp itself, with the fixture
  test normalizing the stamp line so CLI↔web parity holds without the version value diverging.

### Test requirements

- **Generator unit tests:** `llms.txt` index matches `SUMMARY.md` structure and pulls frontmatter
  descriptions; `llms-full.txt` concatenates in `SUMMARY` order; `AGENTS.md` carries the version
  stamp and the live pointer.
- **Anti-drift gate (2.6):** catalog ⊆ docs ⊆ llms-full; no orphan `SUMMARY` page; every `AGENTS.md`
  snippet is lifted from a tested example. (A snippet or namespace added without coverage fails CI.)
- **`init`:** produces `AGENTS.md` by default and skips it with `--no-agents`.
- **`doctor`:** warns (read-only, exit unchanged) when the stamp is behind the runtime, silent when
  current; never edits the file.
- **Regenerate command:** rewrites the primer from the installed runtime, idempotent.
- **Lightweight effectiveness eval (should, not launch-blocking):** a small prompt set ("write a
  Marreta route that ...") run on two models with and without the primer, scored by the real
  `parse`/`lint` (the same toolchain that grounds the agent also scores the eval). This is a DX
  claim, not a performance benchmark, so the benchmark-neutrality rule does not apply: it may be
  stated affirmatively.

### Coverage analysis

- **VS Code extension:** none. `AGENTS.md` is a project-level artifact, not an editor surface; the
  extension stays a thin CLI client.
- **e2e:** the in-memory language guardian is unaffected (no language surface change). The generation
  and gate are covered by unit/functional tests in the language repo, not e2e scenarios.
- **Documentation:** a how-to ("Use Marreta with AI assistants") explaining `AGENTS.md`, the
  `--no-agents` opt-out, the regenerate command, and `llms.txt`; the CLI reference entry for the new
  command; and the site serving `llms.txt`/`llms-full.txt`. All examples lifted from a tested
  project, and these guide pages flow into the same generation (the docs about the feature are part
  of the corpus the feature renders from).
- **Site initializer:** must stay at parity with `marreta init` (2.7). The web scaffold generator
  bundles the same `AGENTS.md`, synced as a private asset, enforced by the existing init-fixture
  parity path. This is the spec-extension surface to update alongside the CLI change.

## 4. Out of scope

- **The Marreta MCP server (Spec B, the flagship fast-follow).** The validate/grounding loop, the
  server, hosting/distribution, and the "Ask Professor Martim" site chat as a second consumer all
  live there. Spec B is announced at launch and delivered shortly after, but not shipped here.
  Reason: the primer is a generated text artifact (near-zero surface, resolves ~80% by attacking
  hallucination at the source), while the MCP server is a running service with an execution surface
  and an irregular client-compatibility matrix; foundation first, a half-built flagship at launch is
  worse than none.
- **Tier 3 (passive):** crawlable docs feeding future training cuts already happen; a copy-paste
  prompt pack is a byproduct of the primer. Neither is specced.
- **No language runtime, parser, or `src` semantic change.** Generation and tooling only.

## 5. Acceptance criteria

1. `marreta init` writes a project `AGENTS.md` (offline stable core + live pointer + version stamp,
   leading with the "diff against priors" first screen), and `--no-agents` skips it. The site
   initializer emits the same `AGENTS.md` (parity enforced by the init fixtures), bundled from a
   private site asset, not a public docs page.
2. `llms.txt` (curated index from `SUMMARY` + frontmatter) and `llms-full.txt` (full concatenation in
   `SUMMARY` order) are generated in the language repo and served at the site root.
3. All artifacts are rendered from the single source (catalog + docs/guide + tested examples); no
   artifact is hand-maintained except the minimal "diff against priors" narrative, whose every claim
   is anchored to a tested example.
4. The anti-drift gate holds: catalog ⊆ `llms-full` (asserted against the full reference, not the
   curated primer), every `SUMMARY`-referenced page present with a non-empty `summary:`, every
   `AGENTS.md` snippet resolving to a region marker in a tested example, and regenerating the
   committed artifacts leaving no git diff (the version-neutral asset keeps this stable). Adding a
   namespace/operation or changing syntax without regenerating fails CI.
5. `doctor` warns read-only when the project's `AGENTS.md` stamp is behind the installed runtime, and
   never rewrites the file.
6. The regenerate command rebuilds the primer from the installed runtime, idempotent.
7. Docs: the AI-assistants how-to, the CLI reference for the new command, and the site serving
   `llms.txt`/`llms-full.txt`, site-synced.
8. Standard gates: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, the full test
   suite (including the new generator + gate tests), plus `functional_tests` and `e2e` green (no
   regression; no runtime change).

---

## Design decisions

The cut was approved in the brainstorm (Tier 1 here as Spec A; MCP announced and delivered as Spec B
fast-follow; everything generated from a single source, never by hand). The design-gate review then
resolved the following:

- **Version stamp is injected at emission, the asset is version-neutral (F1).** Resolves the collision
  with the codegen git-diff gate and with web parity: the committed/baked primer has no version, `init`
  and regenerate write the stamp from the binary's own version, `doctor` compares the written stamp,
  and the web fixture normalizes the stamp line. (2.1, 2.4, 2.5, 2.7, 2.8.)
- **Snippet provenance by construction (F2):** snippets are extracted from example files by region
  markers, never hand-typed; the gate asserts every `AGENTS.md` block resolves to a marker. (2.3, 2.6.)
- **The gate asserts `summary:` present (F3)** on every `SUMMARY` page, so `llms.txt` never degrades to
  an empty description. (2.6.)
- **Completeness is `catalog ⊆ llms-full`, not the primer (F4).** The primer is a curated subset; its
  selection is human-reviewed at the gate, and only its snippet provenance and stamp are machine-gated.
  (2.6.)
- **File set, repo tags, and citations pinned (F5):** the emitted set is `AGENTS.md` +
  `.github/copilot-instructions.md` + `.cursor/rules/marreta.mdc` (2.1); the outline tags each task
  language-repo vs site-repo (3); the initializer is the site repo's Spec 008, `marreta init` is
  `038_PROJECT_INIT` here (2.7).
- **Generator is an `xtask`** (a runtime subcommand was rejected, it would bloat the binary). **CLI
  regenerate command: `marreta agents`** (on-identity, justified against the Spec 057 trim).
  **Ownership v1: wholly generated** (regenerate overwrites, `doctor` only warns), marked-section
  preservation a later refinement.

### First-screen content (reviewed and approved 2026-06-15)

- **The first-screen content is settled** (the §2.1 "Get this right" list, eight corrections plus the
  cheat). Drafted after the design Approved and reviewed in its own round, distinct from the design
  aval. The review added the request-vars correction (params/query/payload/headers/auth, no
  req/request/ctx) as the highest-leverage prior, and `fetch` to the cheat terminals. The hand-written
  narrative is the one layer the gate cannot verify mechanically, so each code line is anchored to a
  region marker in a tested example, and the cheat vocabulary is kept corpus-anchored by review.

### Risks

- **Convention churn.** `AGENTS.md`/`llms.txt` are emerging, not ratified. Mitigated by single-source
  generation (re-target is a render change), which makes the generation choice anti-obsolescence.
- **Scaffold staleness.** A frozen `AGENTS.md` in the developer's repo versus an evolving runtime.
  Mitigated by the version stamp, the live `llms-full.txt` pointer, the bias to the stable surface in
  the offline core, the regenerate command, and the read-only `doctor` warning (the 067 pattern, no
  silent overwrite).
- **Not every developer wants an AI file in their repo.** Mitigated by `--no-agents` and
  documentation.
- **A wrong primer is worse than none** (it reinforces hallucination). Mitigated by snippet
  provenance (tested examples only) and the completeness gate; the residual is the hand-written
  prose, kept minimal and human-reviewed.

---

## Delivery notes

Delivered 2026-06-15. What landed:

- **Generator (`xtask` crate, new workspace).** `cargo run -p xtask -- gen` emits, from a single
  source, `docs/agents/llms.txt` (curated index from `SUMMARY` + `summary:`), `llms-full.txt` (full
  reference), and the version-neutral `AGENTS.md` (template with every code block substituted from a
  `# region:` marker in a tested example). `default-members = ["."]` keeps the runtime build/test
  scoped, so the generator never bloats the binary.
- **Snippet provenance.** Seven first-screen snippets come from a new provider-free
  `docs/examples/agents_primer` (scenario-tested in memory); the `db` pipeline snippet comes from
  `smart_inventory`'s functionally tested line (a pipeline is not scenario-mockable).
- **Runtime emission.** `marreta init` writes `AGENTS.md` + `.github/copilot-instructions.md`
  (`--no-agents` opts out); `marreta agents` regenerates; `doctor` flags a stale stamp read-only.
  The primer is baked via `include_str!` and the version is injected at emission. The Cursor pointer
  was dropped (Cursor reads `AGENTS.md` natively); the Copilot pointer stays.
- **Anti-drift gate.** `tests/agents_gate.rs` asserts catalog ⊆ `llms-full` over all four catalog
  kinds (operations matched by bare name), every `SUMMARY` page summarized, and region provenance;
  `build.yml` runs codegen freshness (`git diff`) plus the `agents_primer` scenarios.
- **Site + docs + editor.** `sync-docs.sh` serves `llms.txt`/`llms-full.txt` at the site root and
  bundles the primer for the initializer (byte-parity via the regenerated init fixtures); a "Use AI
  assistants" how-to and CLI reference entry; a `marreta agents` VS Code palette command (0.2.21).

Two review gates passed: design (with the separate first-screen content round) and code review
(Approved after the catalog-gate completeness fix that extended coverage to operations). Gates green:
`fmt`, `clippy -D warnings`, full suite + `agents_gate`, `functional_tests`, `migrations_functional`,
`e2e`, `vsce package`; site fidelity + build.

The Marreta MCP server (Spec B) remains the named fast-follow, out of scope here.

---

## P.S. Do not forget the docs of record

On delivery, update both `CHANGELOG.md` and `docs/spec/SPEC.md`. See SPEC.md section 1.3.
