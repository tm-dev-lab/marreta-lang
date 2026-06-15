# 077 - Query and Header Input Schemas

> Status: Delivered
> Type: Runtime (request binding + validation) + OpenAPI generator + lint, with docs and examples
> Scope: Let a `schema` declare query-string and header inputs (not only the JSON body), by binding
> a schema per `take` (`take query as Q`, `take headers as H`), with the same validation and string
> coercion already used for the body. Add a second, multi-line `take` layout. Fix the OpenAPI query
> bug (`deepObject` → `form`). This spec deliberately does NOT add field defaults to schemas: that,
> and its database/migrate consequences, are a named follow-up. No change to the persistent schema
> model, so doc/db/queue/cache and `migrate` are untouched.

---

## 1. Purpose

Today a route reads query parameters and headers only through an untyped raw bind: `take query`
hands the route a flat map of strings (`query.term`), `take headers` the same for headers. The
parameters are never declared. Two consequences:

1. **The route does not document its own inputs.** Unlike Spring/FastAPI/Nest (which declare query
   params in the handler signature, not in the URL), a Marreta route gives no machine-readable
   statement of which query params it reads.
2. **The generated OpenAPI is blind and, worse, wrong.** For `take query` the generator emits a
   single generic parameter with `style: deepObject`, which tells a client to serialize
   `?query[term]=hello`, while the runtime reads flat `?term=hello`. A client or "Try it out"
   generated from the spec sends a shape the endpoint does not read. There is also no validation and
   no coercion: every query value arrives as a string, so a numeric param is the caller's problem.

This is not Marreta being wrong to keep query out of the URL literal (no framework puts it there).
It is that Marreta ships first-class OpenAPI and first-class validation, and query/headers are the
one input source with neither. Express/Flask can skip declaration because they do not promise a
generated spec; Marreta does.

The fix reuses the concept that already exists, `schema`, for the two other input sources, instead
of inventing new syntax. It mirrors the body's two tiers (`take payload` raw, or `take payload as
Schema` validated).

## 2. The change

### 2.1 OpenAPI query bug fix (folded in, lands with this spec)

The generic query parameter emitted for a raw `take query` uses `style: deepObject`. That is a pure
bug: it describes `?query[term]=` while the runtime reads flat `?term=`. Change it to `style: form`
(the flat serialization), and flip the generator test that currently asserts `deepObject` (otherwise
the gate breaks). This is correct independently of everything else in the spec, but it ships here
because the same generator path is being reworked.

### 2.2 Per-binding `as Schema` for query and headers

A schema can be bound to any input source, not only the body:

```ruby
schema SearchQuery
    term: string
    limit?: int
    page?: int

route GET "/search" take query as SearchQuery
    # query.term is a validated string; query.limit/page are validated ints or null
    reply 200, doc.products >> like(name: query.term) >> fetch
```

The same applies to headers (`take headers as ApiHeaders`) and is unchanged for the body
(`take payload as NewProduct`). Each binding independently is raw (`take query`) or schema-bound
(`take query as Q`); mixing raw and schema-bound bindings in one route is allowed.

This moves `as Schema` from a route-level clause (today implicitly the payload's) to a **per-binding**
clause. Pre-launch, there is no compatibility burden (see `project_no_prelaunch_breaking`); the
existing corpus is swept to the new form.

### 2.3 Two `take` layouts, no hybrid

There are exactly two ways to write the input list, and they differ in **how many `take` keywords**
appear, not in what they can express.

**Inline (Form 1) — a single `take`.** One `take` keyword on the route line, followed by all bindings
comma-separated. This is the compact shape for a route that takes one input, or several together
(query + headers + payload on one line).

```ruby
# one input
route POST "/products" take payload as NewProduct

# all three at once, one take, comma-separated
route POST "/products/search" take query as SearchQuery, payload as SearchBody, headers as ApiHeaders

# mixed raw and schema-bound, still one take
route GET "/products" take query as SearchQuery, headers
```

There is never more than one `take` on the route line: `take query ... take payload ...` on the same
line is invalid; the bindings after the single `take` are separated by commas.

**Multi-line (Form 2) — N `take` lines.** One `take` keyword per indented line, as many as there are
inputs. The `take` lines are the leading statements of the route body, before any logic. This is the
readable shape when several inputs are declared.

```ruby
# three inputs, one take per line
route POST "/products/search"
    take query as SearchQuery
    take payload as SearchBody
    take headers as ApiHeaders

    reply 200, ...

# mixed raw and schema-bound across lines
route GET "/products"
    take query as SearchQuery
    take headers

    reply 200, ...
```

A single input may also be written multi-line (`route POST "/products"` then `take payload as
NewProduct` on the next line), but the inline one-liner is the idiomatic choice for one input.

**Rule: a route uses Form 1 OR Form 2, never both.** If any `take` is on the route line, there can be
no indented `take` lines, and vice versa. A hybrid (for example `take query` inline on the route line
plus `take payload` indented below) is a parse error with a clear message. Rationale: the route's
input contract is read in one place, not split across the route line and the body; nothing is lost,
since any hybrid is expressible fully inline or fully multi-line.

```ruby
# INVALID — hybrid: a take on the route line AND an indented take
route POST "/products/search" take query as SearchQuery
    take payload as SearchBody       # parse error: mixing inline and multi-line take
    reply 200, ...
```

### 2.4 Flat-only for query and headers, enforced at the binding site, plus a lint

Query and header values are flat on the wire (no clean nested objects). A schema bound to query or
headers must therefore be **flat**: only scalar fields and lists of scalars. Two things are
rejected, not one: a field that references another schema (a nested object), **and** a
`list of <Schema>` (a list of nested objects) — only `list of <scalar>` is allowed. The body keeps
full nesting.

The same schema may be valid for the body (nested) and invalid for query (if it nests). So the
flat check lives at the **binding site** (`take query as X` / `take headers as X`), not on the
schema definition: a nested schema is body-only, and binding it to query/headers is a load-time
error. A dev-time **lint** flags the same condition early, consistent with the lint-warns /
runtime-guards pattern already in the language.

### 2.5 Coercion rules for query and headers

Query/header values arrive as text. A schema-bound value is coerced to its declared type, exactly
as path params already are, and a value that cannot be coerced is a 422.

- **Scalars:** `limit: int` coerces `"20"` to `20`; `"abc"` is a 422.
- **Boolean:** only `true` / `false` are accepted; anything else is a 422 (no `1`/`0` truthiness).
- **Lists (repeated key):** a `list of <scalar>` field is fed by a repeated key:
  `?status=active&status=frozen` → `["active", "frozen"]`. This is flat (a repeated scalar, not a
  nested object), maps to OpenAPI `style: form, explode: true` with an array schema, and reuses the
  existing `list of X` type. Comma-split (`?status=a,b`) is explicitly not supported (non-standard,
  manual parse). A single occurrence of a list field is a one-element list. This repeated-key list is
  framed for **query**; for **headers**, a `list of <scalar>` is fed by a repeated header of the same
  name, but the header-list case is rare and the exotic header is better served by the raw
  `take headers` escape hatch.
- **Empty value is absent:** `?term=` (present but empty) is treated as not provided, uniform across
  all types (so `?limit=` does not try to coerce `""` to an int; it is simply absent). This composes
  with required/optional: a required field that is empty/absent is a 422; an optional field is null.

### 2.6 Header name mapping

Header names are case-insensitive and usually hyphenated (`X-Request-Id`); schema field names are
snake_case. A field maps to a header by convention: case-insensitive, with `_` and `-` treated as
equivalent. So `request_id` matches `X-Request-Id` / `request-id` / `Request-Id`. A header that does
not map cleanly (an exotic vendor header) uses the raw escape hatch: `take headers` and
`headers["X-Weird"]`. An explicit mapping syntax is deferred (YAGNI) until a real case needs it.

### 2.7 Raw `take` (no schema) and the OpenAPI

When a binding is schema-bound, the generator emits the per-field, named, typed parameters
(`in: query` / `in: header`, flat). When a binding is raw (`take query` with no schema), there are no
declared names to emit, so the generator emits **no** query/header parameters for that route (the
endpoint reads arbitrary, undocumented input). Declaring a schema is how a route opts into documented
params. This keeps the generator unambiguous between the schema-bound and raw cases. (The 2.1 fix is
about the same raw case rendered correctly while it exists; with this rule the raw case simply
contributes nothing to the parameter list.)

### 2.8 Validation error path

A schema-bound query/header that fails validation or coercion returns **422**, through the same clean
validation path the body already uses (the body's `take payload as Schema` 422). No new error code.

### 2.9 The `or` fallback is documented honestly, defaults are deferred

Without schema defaults (deliberately out of scope, 4), an optional query param's fallback value
lives in the route body, exactly as today: `limit = query.limit or 20`. The spec documents this and
states the honest caveat: **`or` is not a real default.** `or` triggers on any falsy value, not only
on absence: `?limit=0` with `limit = query.limit or 20` yields `20`, not `0`; likewise for a boolean
or an empty string. For `limit` (where `0` is rare) this is harmless; for an int where `0` is
meaningful, or a boolean, `or` is wrong. A schema default would fill only when the field is absent.
This caveat is the honest argument for the follow-up default spec ("the schema default exists because
`or` is not a real default"); it is documented, not hidden.

## 3. Implementation outline

The change is additive and provider-agnostic. It does not touch the persistent schema model, so
`build_persistent_tables`, `migrate`, doc, queue, and cache are unaffected by the data model (they
are exercised only to confirm no regression).

- **Parser / AST:** `TakeBinding` carries an optional per-binding schema name. Parse the inline
  comma-list (Form 1) and the leading indented `take` statements (Form 2), with the no-hybrid rule
  and the "takes precede logic" rule, each a clear parse error. The route-level `schema` clause (the
  old payload-only `as`) is folded into the payload binding's optional schema.
- **Payload-schema reader migration (checklist — the per-binding `as` edge).** Moving the payload
  `as` from a route-level clause to the payload binding means **every reader of the old route-level
  payload schema must read it from the payload binding instead**; a missed reader silently stops
  validating the payload or drops it from the OpenAPI (no error). The readers to migrate (line refs
  approximate, verify before implementing): payload validation in `server.rs` (~861), and the request
  body in the OpenAPI generator (`openapi.rs` ~204, ~542, ~692). **The other three `as` uses are
  separate and stay untouched:** the queue/topic consumer schema (`take message as`, `OnQueue`/
  `OnTopic`, read in `server.rs` ~1689 and `openapi.rs` ~116/123), the task parameter schema
  (`f(x as Schema)`, `ParamDef.schema`), and the response schema (`reply N as`, `response_schema`).
- **Request binding (server + interpreter):** when a `take query`/`take headers` carries a schema,
  build the flat string map, run it through the validator with string coercion, and bind the coerced
  value; a failure is a 422. Raw binds are unchanged.
- **Validator:** reuse the existing coerce-and-validate path. Add string coercion for the all-string
  query/header case (scalar, boolean true/false, list-of-scalar via repeated key, empty-as-absent),
  and the flat-only check used at the binding site.
- **Lint:** a rule flagging a nested/schema-referencing schema bound to query/headers.
- **OpenAPI generator:** emit named/typed parameters (`in: query` / `in: header`) from a bound
  schema (lists as arrays, `required` from non-optional); a raw bind contributes no parameters; the
  `deepObject` → `form` fix (2.1) and its flipped test.
- **Formatter (`src/formatter.rs`):** format both `take` layouts (the inline comma-list and the
  multi-line block) and the per-binding `as` consistently and idempotently. The formatter already
  handles `take`, so the new layouts must be taught, not bolted on.
- **Header mapping:** the case-insensitive, `_`↔`-` convention in the header bind.
- **Corpus sweep:** the existing `.marreta` examples and e2e/functional routes using `take query` /
  `take headers` migrated to the new form where it demonstrates the feature, the `or` caveat noted
  where a query fallback is shown.

### Implementation strategy (explicit, owner-directed)

This touches a load-bearing language surface, so the unit and functional suites are expected to break.
The strategy is deliberate:

1. Land the core change (parser/AST, binding, validator, OpenAPI).
2. **Run the full suite and let it break** — the breakage is the impact map; do not pre-guess it.
3. **Classify each break before repairing it** — do not blanket-update expected outputs. Two classes:
   (i) a **grammar-migration fixture** (the same behavior written in the new syntax) is simply
   updated; (ii) a **genuine behavior change** must be consciously confirmed correct *before* its
   expected output is touched. This is the specific trap of "let it break": repairing a real
   regression as if it were a fixture migration, by flipping the expected value to go green. Then add
   the new coverage (unit + functional) the feature requires. Never weaken a test to go green (see
   `feedback_never_rewrite_failing_tests`); each break is understood and fixed at the root.
4. Run the migration functional suite (`docs/examples/migrations_functional/test.sh`) too, to prove
   the untouched persistent/migrate path did not regress (this spec does not change it, so it must
   stay green without edits).

### Test requirements

- **Parser unit tests:** Form 1 (inline comma-list, raw/schema/mixed), Form 2 (multi-line, raw/
  schema/mixed), the no-hybrid parse error, the "takes precede logic" error, per-binding `as`.
- **Validator/coercion unit tests (isolated):** scalar coercion and its 422, boolean true/false only,
  list-of-scalar via repeated key, empty-as-absent across types, the flat-only rejection.
- **OpenAPI unit tests:** a bound query schema emits named/typed params (list → array, required from
  optional); a bound header schema emits `in: header`; a raw bind emits no params; the `form` style
  (the flipped assertion).
- **Functional coverage end to end:** a route with `take query as Q` validates and coerces against a
  live server (good request passes coerced, bad type → 422, repeated key → list, empty → absent),
  headers likewise, and the generated OpenAPI shows the named params.
- **migrations_functional:** runs green unchanged (no persistent-model change).

### Coverage analysis

- **VS Code extension:** low, polish only (not "none"). The generic grammar already covers the new
  surface: `take` is a keyword (so multi-line `take` highlights), `as Schema` is highlighted anywhere
  (so `take query as X` highlights), and `take <name>` has a rule. Optional polish: add
  `take query as` / `take headers as` snippets, and optionally refine highlighting of the inline
  comma-list (`take a, b, c`). Verify the two layouts render acceptably; no provider/completion change
  is required (the client stays thin).
- **e2e:** add routes exercising a typed query and a typed header (validation, coercion, 422, repeated
  key) plus a `run.sh` live HTTP assertion, since this is request-binding/resolution semantics over
  the served process. The e2e is the in-memory language guardian and must track this new surface.
- **Documentation (substantial):** `concepts/schemas.md` (a schema can declare query/header inputs;
  the flat-only rule; exact query match vs the header `_`/`-` convention), a new
  `how-to/read-request-inputs.md` (every take variation — raw/typed, inline/multi-line, the five
  inputs incl. why `take raw` exists — and how to *read* each: payload nesting, exact query match,
  header normalization, raw subscript) added to `SUMMARY.md`, `how-to/validate-a-payload.md` (extend
  to query/headers), `how-to/openapi-docs.md` (query/
  header params now appear, the raw-bind = undocumented rule), `reference/lint.md` (the new flat-
  schema-in-query lint rule), `reference/conventions.md` (the two `take` layouts and the no-hybrid
  rule, and the `or`-is-not-a-default caveat), and the tutorials/quickstart examples using `take
  query` updated to the typed form with the `or` caveat where a fallback is shown. All examples must
  be lifted from a tested project under `docs/examples`.

### Downstream surfaces, site sync, and no-regression

The change reaches several surfaces beyond the runtime; the spec is not done until each is verified
or updated.

- **Site sync (`docs/guide` → marreta.dev/docs).** Every guide page touched above is authored content
  the site serves; the site is synced on delivery, same as prior specs.
- **README.md.** The "Built-In API Concepts" table (Request validation = `take payload as Schema`)
  and "A Taste of the Language" reflect query/header schema binding; add a short typed-query example.
- **Formatter (`marreta fmt`).** Beyond the implementation item above, `marreta fmt` over the whole
  corpus must stay clean and idempotent on both `take` layouts; it is part of the gate.
- **Linter (`marreta lint`).** The new flat-schema-in-query/header rule (2.4) is on the lint
  reference page and runs clean across the corpus.
- **Served Swagger.** The `/docs` Swagger UI is the same generator output; verify the *served* spec
  shows the named query/header params and the corrected `form` style, not only the unit test.
- **Example apps no-regression (mandatory).** `smart_inventory`, `omni_hub`, `ecommerce` use no
  `take query`/`take headers` today, but the per-binding `as` grammar change must keep
  `take payload as Schema` parsing; each must load and its `test.sh` pass. Migrating any to the typed
  query form is optional showcase; no-regression is mandatory.
- **Benchmark `digital_bank` stays functional.** Verify the Marreta app still loads and serves at the
  end (no `take query` there today, so its raw paths are unaffected). Do NOT re-run the benchmark; if
  it remains functional, change nothing in it.
- **`init` scaffold + site initializer.** Verify the code `marreta init` scaffolds still parses under
  the new grammar; if `init` is updated to showcase a typed query/header, the site initializer that
  mirrors `init` is updated to match. If `init` is unchanged, the initializer is left alone.
- **TM Dev Lab launch post (post-delivery check).** At the end, review the launch post's examples
  against the new query handling and update any that show a raw query that should now be the typed
  form. This is a separate repo and a post-delivery follow-up, not part of the merge.

## 4. Out of scope

- **Field defaults in schemas** (`limit: int = 20`). Deliberately deferred: defaults are the only
  thing that would force touching the shared schema model and rippling into doc/db/queue/cache and
  `migrate`. They become a named follow-up spec covering (a) `default` on `SchemaField` with the
  `?`/`=` mutual exclusion, (b) runtime fill (uniform across inputs), then (c) the real DB column
  `DEFAULT` and `migrate` detecting/generating/applying default drift. That follow-up builds on the
  already-delivered 073 (Migrate Roundness) drift machine; its genuinely new behavior is the first
  `ALTER COLUMN SET DEFAULT` (migrate acting in-place on an existing column, where today in-place
  changes are report-only). Adding defaults later is purely additive to this spec (no rework). See
  `project_query_schema_defaults_specs`.
- **Defaults on `list of` and on schema-reference fields** (the open question kept for the follow-up:
  likely restrict defaults to scalars and list-of-scalar, the useful list default being `[]`; no
  default on relations/nested-schema fields).
- **Explicit header-name mapping syntax** (`@header(...)`), deferred until a real exotic-header case
  needs it.
- **`native_query` and the persistent/migrate path**: unchanged; exercised only for no-regression.

## 5. Acceptance criteria

1. A schema can be bound to query and to headers per binding (`take query as Q`, `take headers as
   H`), validated and coerced like the body; raw `take query` / `take headers` are unchanged.
2. Both `take` layouts work — inline comma-list (Form 1) and leading indented lines (Form 2), each
   with raw, schema-bound, and mixed bindings — and a hybrid (a `take` on the route line plus an
   indented `take`) is a clear parse error.
3. A schema bound to query/headers must be flat (scalars and lists of scalars, no schema reference);
   binding a nested schema is a load-time error, and a dev-time lint flags it.
4. Coercion holds: scalar from text (bad value → 422), boolean only `true`/`false`, list-of-scalar
   via repeated key (`?k=a&k=b` → list), empty value treated as absent uniformly.
5. Header fields map by the case-insensitive `_`↔`-` convention; an unmapped header uses the raw
   `take headers` escape hatch.
6. The OpenAPI shows named, typed parameters for a schema-bound query/header (lists as arrays,
   `required` from non-optional); a raw bind contributes no parameters; the query serialization is
   `style: form` (the `deepObject` bug fixed, its test flipped).
7. A schema-bound query/header validation failure returns 422 through the clean validation path.
8. The `or` fallback is documented with the falsy-trap caveat; schema defaults are not added.
9. The formatter formats both `take` layouts and per-binding `as` idempotently, and `marreta fmt`
   is clean across the whole corpus.
10. No-regression across the corpus: `smart_inventory`, `omni_hub`, and `ecommerce` load and their
    `test.sh` pass; the `digital_bank` Marreta app stays functional (verified, not re-run); `init`
    scaffolds parseable code (and the site initializer is synced only if `init` changed).
11. Downstream surfaces reflect the change: `docs/guide` (site-synced), `README.md`, the lint
    reference page, and the *served* `/docs` Swagger; the TM Dev Lab launch post is reviewed
    post-delivery and updated only if its examples need it.
12. Standard gates green: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, the full
    test suite (repaired and extended per the strategy), plus `functional_tests`,
    `migrations_functional` (green unchanged), and `e2e/run.sh`.

---

## Design decisions (resolved in review)

- **Cut defaults from this round (owner + reviewer).** Defaults were the only thing forcing a change
  to the shared schema model; removing them makes the spec purely additive and provider-agnostic, and
  removes the migrate risk entirely. Defaults become a clean future spec, not painting this one into a
  corner.
- **Reuse `schema` for the three inputs**, with per-binding `as` mirroring `take payload as`, rather
  than inventing query-specific syntax. One concept, reusable across routes.
- **Four product decisions:** (1) lists in query via repeated key (yes; "flat" means no nested object,
  a repeated scalar is flat; never comma-split); (2) empty query value = absent, uniform; (3) a raw
  bind contributes no OpenAPI parameters (declared schema = documented); (4) header mapping by
  convention plus the raw escape hatch.
- **No hybrid `take` layout:** a route is fully inline or fully multi-line, for a single-place input
  contract.
- **Factual correction carried in:** 073 (Migrate Roundness) is Delivered, not parked; its drift
  machine exists and reports type/nullability/removal. The deferred default spec extends it, it does
  not coordinate with parked work.
- **The `or` caveat is documented, not hidden** (`or` fires on falsy, a real default fires only on
  absence), and is the honest motivation for the future default spec.

---

## Delivery notes

Delivered. Code review approved with no findings (verified against the diff). All gates green.

What landed:

- **Parser/AST** (`ast.rs`, `parser.rs`): `TakeBinding` is a struct (`kind`/`name`/`schema`), `as` is
  per-binding, and a route uses one of two layouts — inline (single `take`, comma-separated) or
  multi-line (N leading indented `take` lines) — with the no-hybrid and takes-before-logic rules as
  parse errors. The route-level `schema` field was removed from `Route`/`RouteDefinition`, so every
  reader had to move to the payload binding (compile-enforced, not coverage-dependent).
- **Reader migration**: payload validation (`server.rs`) and the OpenAPI request body (`openapi.rs`)
  resolve the payload schema via `ast::payload_schema(&take)`. The other three `as` uses (queue/topic
  consumer, task param, `reply as`) are untouched.
- **Validator** (`validator.rs`): `coerce_scalar_input` (text→type; boolean `true`/`false` only;
  `list of <scalar>` via repeated key; empty=absent; header `_`/`-` case-insensitive convention while
  query matches exactly) and `first_non_flat_field` (rejects schema reference, `list of <Schema>`,
  map/nested).
- **Binding** (`server.rs`) + **scenario parity** (`scenario_tests.rs`): `RawQuery` threaded so a
  repeated-key list coerces identically in-memory and on the live server.
- **OpenAPI** (`openapi.rs`): named/typed parameters per field for a schema-bound query/header (list →
  array, `required` from optional); a raw bind emits no parameters; the misleading `deepObject` query
  parameter is gone.
- **Load guard** (`file_loader.rs`) + **lint** `non_flat_input_schema` (`lint.rs` + lint reference).
- **Coverage**: unit (coercion, flat-check, parser layouts, lint, OpenAPI); `functional_tests`
  section 68 (26 asserts incl. decimal/instant, enum, list, required header, case-insensitive, the
  three-typed combo) for a 601/0 total; e2e 68 scenarios + 41 live smoke (incl. the served
  `/openapi.json`). Gates: fmt, clippy `-D warnings`, full suite, `functional_tests` 601/0,
  `migrations_functional` PASS unchanged (persistent model untouched), e2e, `vsce package`.
  No-regression proven live: smart_inventory 30/0, omni_hub 20/0, ecommerce 40/0, plus digital_bank
  and the `init` scaffold load.
- **Docs**: new `how-to/read-request-inputs.md` (every take variation + how to read each input — the
  exact-match rule for query, the lowercased raw header keys, the schema `_`/`-` convention, why
  `take raw` exists), plus `concepts/schemas`, `validate-a-payload`, `openapi-docs`, `conventions`,
  `reference/lint`, and the README. Every example was run against a served project.

Decisions confirmed in review: remove `deepObject` entirely (raw = no params) rather than `→ form`;
query stays exact-match (no `_`/`-` for query — a principled asymmetry, header-only, no functional
risk; documented); defaults remain out of scope as a named follow-up.

Pending (post-delivery): the `docs/guide` site sync (the new how-to + touched pages) and the TM Dev
Lab launch post review (separate repo; update any raw `take query` example that should now be typed).

---

## P.S. Do not forget the docs of record

On delivery, update both `CHANGELOG.md` and `docs/spec/SPEC.md`. See SPEC.md section 1.3.
