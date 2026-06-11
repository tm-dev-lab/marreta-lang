# 069 - Docs For Inferred Indexes And Reserved Words

> Status: Delivered
> Type: Documentation (authored guide under `docs/guide`)
> Scope: Write the hand-authored `docs/guide` pages for two features that shipped with only
> their docs of record (SPEC.md + CHANGELOG) and deferred the guide: Spec 067 (inferred document
> indexes) and Spec 068 (reserved word normalization, which explicitly deferred its keywords-page
> two-layer writeup here). Docs-only, no runtime change. Every code example is lifted from or
> verified against a tested project under `docs/examples`.

---

## 1. Purpose

Two delivered features have no guide coverage yet, so a reader cannot learn them in context:

- **Spec 067 (inferred document indexes).** The document provider infers each collection's index
  from the query surface (the `where`/`order` shape of every `doc.<collection>` pipeline) and
  ensures it in MongoDB at `serve` startup, with no declaration, no marker, and no migration. The
  guide page `reference/namespaces/doc.md` does not mention indexes at all, so the performance
  story (and the fact that the developer does nothing) is invisible. Note: the pre-067 declarative
  approach (an `index` directive, a `doc:` marker, migration indexes) was reverted before any
  release, so this is purely additive. There is no stale declarative text to remove (verified).
- **Spec 068 (reserved word normalization).** Spec 068 deferred its keywords-page writeup here. The
  current `reference/keywords.md` groups constructs by purpose but never states the reserved-word
  model, so a reader cannot predict whether a given word can be a variable. The teachable thesis
  (namespaces are reserved, directives and vocabularies are contextual) has no home.
- **Error-codes reference gaps.** `reference/errors.md` omits the live `unique_violation` (HTTP 409)
  contract code that 067 kept, and predates the automatic-422 validation behavior, with an intro
  that overstates that the developer always chooses the HTTP status. These are part of the 067/068
  docs debt this spec settles.

## 2. The change

### 2.1 Document the inferred index (Spec 067)

Add an **Indexes** section to `reference/namespaces/doc.md`. It must document both the mechanism and
its boundaries, so the section promises a contract rather than magic (the first reader who hits an
exclusion in production must have been warned).

The mechanism, in plain prose:

- The runtime reads the `where` equality/range fields and the `order` sort of every query against a
  collection and ensures a matching index in the document provider, in the background, at
  `marreta serve` startup. The developer declares nothing.
- A new query shape is picked up and ensured on the next deploy (idempotent), so indexes follow the
  code rather than a separate migration.
- `marreta doctor` reports the inferred indexes (present, absent, orphan) so the developer can see
  the plan.
- Keep the existing "no migration" promise consistent: a collection, its fields, and now its indexes
  all exist without a declaration step.

The boundaries (where inference does not reach, and what it never does):

- **Exclusions.** Inference covers the `>> where(...) >> order(...)` query surface with literal field
  names. It does not infer from `like(...)`, from a raw `doc.pipeline(...)`, or from a field that is
  not a literal string (a field chosen through a variable). Moving a query from `>> where` to a raw
  pipeline drops the inference for that shape.
- **Operational.** The ensure runs in the background, so `serve` binds immediately and a new filter
  on a large collection serves unindexed until the build finishes. Inference never drops an index: a
  shape that no longer exists leaves an orphan, which `doctor` reports for a human to remove.
- **Coexistence.** An index created by hand is never touched (ownership is by name), and a hand-made
  unique index that a write violates surfaces as the `unique_violation` error (HTTP 409), linking to
  [the error codes reference](../errors.md) (ties to §2.4).

The mechanism example uses the query forms already on the page (the `where(...) >> order(...)`
pipeline), so it is covered by the existing `docs/examples` doc coverage. No new query syntax is
introduced.

### 2.2 The two-layer reserved-word model as the keywords-page frame (Spec 068)

`reference/keywords.md` today groups constructs by purpose but never states the reserved-word model.
Rather than append a section (which would leave the page with two competing organizing principles),
the two-layer model becomes the page's **organizing intro**: the one-sentence rule plus the two
layers up top, and the existing purpose-groups annotated with the layer each belongs to.

The intro states:

- The one-sentence rule: **namespaces are reserved, directives and vocabularies are contextual.**
- **Layer 1, reserved.** Words the lexer reserves that cannot be a variable (binder) in any
  position: the infrastructure **namespaces** (`db`, `doc`, `feature`, `cache`, `queue`, `topic`,
  `fs`, `json`, `base64`, `uuid`, `log`, `time`, `math`, `http_client`), the **`env`** accessor, the
  structural keywords, and the **type tokens** (`string`, `integer`, `float`, `boolean`, `instant`,
  `date`, `duration`, `interval`).
- **Layer 2, contextual.** Words meaningful only in one position and free as an identifier
  everywhere else: the `db:` schema directive, the type-names `list` / `decimal` / `enum`, the
  pipeline vocabulary (`where`, `fetch`, `limit`, `order`, ...), the scenario DSL (`scenario`,
  `given`, `when`, `then`), and the injected bindings (`params`, `auth`, `payload`, ...).
- A reserved word is still free in a **name position** (after `.`, a map key, a schema field name, a
  named-arg name, a `select` column), where it reads as that name. It is blocked only as a binder,
  with the dedicated error `'doc' is a reserved word (...); rename the variable.`
- The documented contrast: a schema field named `doc`/`feature`/`env` is allowed, but `db` is not,
  because the `db:` directive already claims that line.

A single compact example block makes the name-position-vs-binder rule concrete in both directions
(free as a name, blocked as a binder), for example `payload.date`, `{ env: "prod" }`, and
`select(date)` as names against `doc = 1` raising the dedicated reserved-word error. One block, not
one per word, lifted from or verified against a tested `docs/examples` project.

### 2.3 A pointer from the concepts page

Add one sentence to `concepts/namespaces.md` stating that a namespace name is reserved and cannot be
shadowed by a variable (so a documented provider never silently disappears from a scope), linking to
the keywords Reserved words section. This is the "why" behind 2.2 in the concepts layer.

### 2.4 Fill the error-codes reference gaps (Spec 067 contract + pre-existing accuracy)

`reference/errors.md` is missing live, developer-facing contract behavior. Three changes, on one
page:

- **`unique_violation` (the Spec 067 contract code).** The `unique_violation` code (surfaced as HTTP
  409) survived the rewind by cherry-pick, is live in the runtime, and fires for any unique index
  violation, relational or document, including a hand-made one. It is a contract error code with no
  entry in the reference. Add a row for it (meaning: a write violated a unique index or constraint,
  returned as 409). This is what §2.1's coexistence note links to.
- **The automatic 422 from schema validation.** Payload validation (`take payload as Schema`) returns
  HTTP 422 automatically on a violation. The page does not mention it. Document this behavior so a
  reader knows a 422 can come from the runtime, not only from their own `fail`.
- **Fix the overstating intro.** The intro says the developer's own `fail`/`raise` choose the HTTP
  status "while these codes do not". The automatic 422 (and the 409) contradict that absolute. Adjust
  the sentence so it no longer overstates that status is always developer-chosen.

The last two are pre-existing accuracy gaps independent of the rewind (they were in the original
069 docs plan that the re-scope dropped). They are kept here deliberately rather than dropped; the
decision is recorded so nothing vanishes silently.

## 3. Implementation outline

- `docs/guide/reference/namespaces/doc.md`: new Indexes section, mechanism + boundaries (§2.1).
- `docs/guide/reference/keywords.md`: the two-layer model reframed as the page intro, groups
  annotated by layer, plus one example block (§2.2).
- `docs/guide/concepts/namespaces.md`: one cross-linking sentence (§2.3).
- `docs/guide/reference/errors.md`: a `unique_violation` row, the automatic-422 note, and the intro
  fix (§2.4).
- Follow `docs/STYLE.md`: Marreta code blocks in ```ruby, no em-dashes / semicolons / emojis in
  prose, refer to "the document provider" rather than MongoDB except where a page already names the
  provider. Each example must be lifted from or verified against a tested project under
  `docs/examples` (the doc-namespace examples already exist).
- No `src/` change, so the runtime/extension tiers do not apply; only the core doc review.

## 4. Out of scope

- The `shadows-injected-binding` lint (a separate small spec, tracked in SPEC.md §1.4).
- Any runtime, grammar, or extension change. This is the authored-guide pass only.
- The cross-repo site sync and the marreta-lang-stealth "Spec 067" reference update (the latter is
  the last rewind-cycle follow-up, actioned at 069 close).

## 5. Acceptance criteria

1. `reference/namespaces/doc.md` has an Indexes section that explains the mechanism (inference from
   the query surface, ensure-at-startup, no declaration / marker / migration, `marreta doctor`
   report, consistent with the page's "no migration" promise) **and the boundaries** (the `like` /
   `doc.pipeline` / non-literal-field exclusions, the background-ensure and never-drop/orphan
   operational notes, and the hand-made-index coexistence with the `unique_violation` 409 link).
2. `reference/keywords.md` is organized by the two-layer model: the one-sentence rule and the two
   layers as the intro, the existing groups annotated by layer, the name-position freedom with the
   dedicated binder error, the `db` schema-field contrast, and one compact example block showing a
   reserved word free as a name and blocked as a binder.
3. `concepts/namespaces.md` states that a namespace is reserved and cannot be shadowed, linking to
   the keywords section.
4. `reference/errors.md` has a `unique_violation` row (HTTP 409), documents the automatic 422 from
   schema validation, and no longer overstates that the HTTP status is always developer-chosen.
5. Every code example is present in or verified against a tested `docs/examples` project; prose
   follows `docs/STYLE.md`.
6. Standard gates for a docs-only change: `cargo fmt --check`, `cargo clippy --all-targets -- -D
   warnings`, and the full test suite stay green (no runtime change, so `functional_tests` /
   `migrations_functional` / e2e are unaffected and not required by this spec).

---

## P.S. Do not forget the docs of record

On delivery, update both `CHANGELOG.md` and `docs/spec/SPEC.md`. See SPEC.md section 1.3.

---

## Delivery notes

Four authored guide pages, docs-only, no runtime change. Paid the guide debt that 067 and 068
deferred:

- `reference/namespaces/doc.md`: an **Indexes** section, mechanism (inference from the `where`/`order`
  query surface, background ensure at `marreta serve` start, no declaration / marker / migration,
  `marreta doctor` report) and boundaries (the `like` / `doc.pipeline` / non-literal-field / builder-
  indirection exclusions, the background-ensure tradeoff, never-drop/orphan, hand-made-index
  coexistence with the `unique_violation` 409).
- `reference/keywords.md`: the two-layer reserved/contextual model reframed as the page's organizing
  intro (the one-sentence rule, both layers, the existing groups noted as Layer 1), with a runtime-
  verified name-vs-binder example block.
- `concepts/namespaces.md`: a sentence on why a native namespace is reserved (cannot be shadowed),
  linking the model.
- `reference/errors.md`: a `unique_violation` (409) row, a Validation 422 note, and a non-exhaustive
  intro that no longer overstates that the HTTP status is always developer-chosen.

Examples lifted from / verified against `docs/examples` (the `doc_index` demo) and the runtime
(reserved-word and builder-termination snippets checked with the binary). Two review rounds: the
design round (F1 errors.md gaps + F2 index boundaries incorporated, S1/S2 adopted) and the diff round
(builder terminated with `>> fetch`, non-exhaustive errors intro, the builder-indirection exclusion,
the exact `marreta serve` trigger). Core gates green (`fmt`, `clippy -D warnings`, full test suite);
no `functional_tests` / `migrations_functional` / e2e needed since nothing in the runtime changed.
