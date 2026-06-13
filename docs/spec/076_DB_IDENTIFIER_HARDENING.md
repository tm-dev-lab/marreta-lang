# 076 - Db Identifier Hardening

> Status: Delivered
> Type: Runtime (db query builder) + security, with docs
> Scope: Close the SQL identifier injection vector that the Spec 071 lint only warns about. Filter
> values are parameterized, but identifiers (the `order_by` clause, `select` columns, and filter
> column names) are concatenated verbatim into the SQL, so a runtime-derived identifier (for example
> `order_by(query.sort)`) is injectable. Add a runtime guard in the query builder so a non-literal
> or unknown identifier cannot produce injectable SQL. Carries open design questions for the
> reviewer brainstorm (validate vs quote vs reject, where the guard lives, how `order_by`
> column-plus-direction is handled).

---

## 1. Purpose

Marreta parameterizes filter **values**: `where(price: x)` renders `price >= $1` and binds the value
as a prepared-statement parameter (`src/db/query_builder.rs:47-58`). There is no value-level
injection.

But **identifiers** are concatenated verbatim into the SQL string:

- `ORDER BY {}` from the `order_by` string (`src/db/query_builder.rs:67`),
- `SELECT {}` from `select_cols.join(", ")` (`src/db/query_builder.rs:14`),
- the filter column name in `{column} {op} $n` and `{column} IN (...)`
  (`src/db/query_builder.rs:51, 55, 58`).

These strings arrive from the pipeline, where `order_by` and `select` are evaluated to plain strings
(`src/interpreter/pipeline.rs:227, 242`) that may come from request input. The concatenation is safe
when the identifier is a literal from the `.marreta` source (the documented usage,
`order_by("price desc")`), and becomes a SQL injection vector the moment a developer passes runtime
input:

```ruby
route GET "/products"
    take query
    db.products >> order_by(query.sort) >> fetch   # query.sort is request input, concatenated raw
```

Spec 071 shipped the `non_literal_sql_identifier` lint that warns at dev time when a db identifier is
built from a runtime value. That is advisory only: it can be ignored, suppressed
(`# marreta: allow`), or never run, and it protects nothing in production. The lint is the warning;
this spec is the runtime guard that actually closes the vector. The intent was set when 071 landed:
the lint warns now, the runtime guard (this spec) is the real defense.

## 2. The change

The design is layered, not a single mechanism, because of one load-bearing fact verified in the
code: **the column allowlist does not always exist.** `db.<table>` works with or without a declared
`db:` schema, and `build_persistent_tables` only knows columns for schemas that declare `db:`. So
"validate against known columns" cannot be the universal mechanism. A guard built only on a schema
allowlist would leave every schema-less table unprotected.

### 2.1 Universal syntactic floor plus quoting (always, closes the vector)

In the query builder, every identifier is validated to a strict shape and emitted quoted. A valid
SQL identifier is `[A-Za-z_][A-Za-z0-9_]*`, optionally one dot for `table.column`. Anything else (a
space outside the order direction, a quote, `;`, `--`, a parenthesis, an operator) is rejected with
a clean Marreta error. What passes is emitted quoted (`"price"`). This is the floor because it works
with no schema and makes injection structurally impossible: a string that passes this shape cannot
contain `;`, `'`, or `--`. The floor alone closes the hole.

### 2.2 Optional schema layer (when a `db:` schema is declared)

When the table has a declared schema, validate the (already floor-passing) identifier against the
known columns, catching a typo too, with an `unknown column 'x'` error. This is a bonus layer, not
mandatory: a schema-less table is still safe via the floor, it just is not typo-checked.

### 2.3 `order_by` is structure, so the guard is a mini-parser

`order_by("price desc")` is not a bare identifier. The supported grammar is explicit and minimal:
`column [asc|desc]`, comma-separated for multiple columns. Each part validates the column (floor,
plus the schema layer when present), accepts only the `asc`/`desc` keyword (case-insensitive), and
rejects everything else. `NULLS LAST/FIRST` is out of this form (use `native_query`, already
parameterized, or a follow-up). The grammar is documented, and anything that does not match is
rejected.

### 2.4 Dynamic sort is first-class, and falls out for free

With the floor plus quoting, `order_by(query.sort)` where `query.sort` is `"price desc"` simply works
safely: the column is validated and quoted, the direction parsed. Injection closes but the
legitimate dynamic-sort use case (a UI letting the user sort by a column) survives, which is the
whole gain over a "reject all runtime identifiers" design. Deferred (follow-up if demanded): an
explicit allowlist API, for example `order_by(query.sort, allow: [...])`, for the schema-less case
that wants typo protection too. The floor already makes the schema-less case safe.

### 2.5 `select` takes bare column identifiers only (correction to the brainstorm)

The brainstorm raised a "computed alias" sharp corner (`select(net: "total * 0.9")`) and proposed a
literal-only rule. Verification corrected the premise: **computed select aliases are deferred and
unimplemented.** Spec 009 (`AC-3.5d`) records "computed alias with named args deferred to Phase 5",
the runtime drops the named-argument name and the builder emits no `AS` (so the current behavior is
an accidental half-feature, not the documented one), and the user-facing `db` docs do not expose it.

So this spec does not add computed aliases or the literal-versus-runtime origin tracking they would
need. `select` accepts only bare column identifiers under the same floor (plus the schema layer when
present), and rejects non-identifier content. This is simpler and closes the hole with no
origin-tracking path. If computed aliases are ever implemented (a future spec), they ship with the
literal-only rule baked in from day one. This removes the only place the "just validate the
identifier" story broke.

**Mandatory doc-of-record sync (review finding).** The language spec of record still documents the
unimplemented computed alias: `docs/spec/SPEC.md` section 4.3, line 1028 ("accepts column names and
computed aliases") and the example on line 1033 (`select(net: "total * 0.9")` rendering
`(total * 0.9) AS net`). That documents exactly what this hardening will reject, so 076 must correct
section 4.3 in the same change: drop "computed aliases" from the sentence and remove the example,
and state that `select` accepts column names (computed alias deferred, Spec 009 Phase 5). `db.md`
already does not expose it, so only SPEC.md section 4.3 needs the edit. Without this, the hardening
would ship contradicting the language spec (someone following section 4.3 would get
`invalid_identifier`).

### 2.6 The guard lives in the query builder, allowlist threaded as data

The floor is pure `string -> string`, so it belongs where the SQL is built (and protects even paths
that bypass the pipeline). The schema layer needs the registry, but instead of splitting the guard
across two locations, the allowlist is threaded into the query state as
`known_columns: Option<HashSet<String>>`, populated from the schema when available. The builder does
both: the floor always, the column check when `Some`. One location, testable in isolation, which for
security code is exactly the pure-classifier pattern of 067 and 071.

### 2.7 Error shape: 400 through the clean machinery

Two sub-cases, both 400 (the request shaped the query from input), both routed through the clean
error path (the `status_for_error` / stable-body machinery) so SQL never leaks:

- **Illegal form** (an injection attempt or garbage): `400 invalid_identifier`.
- **Unknown column** (valid form, not a real column, only when the schema is known):
  `400 unknown_column`, distinct message.

The hardcoded-typo case (`order_by("totl")` in the source) is already caught by the 071 lint at dev
time, so it needs no special runtime treatment.

`native_query` is out of all of this: its `#{}` interpolations are already bound as
prepared-statement parameters, not concatenated.

## 3. Implementation outline

- `src/db/query_builder.rs`: the pure syntactic floor plus quoting at the three concatenation points
  (the `ORDER BY`, the `SELECT` column list, and the filter column name), the `order_by` mini-parser
  (2.3), and the optional schema-column check when the threaded allowlist is `Some`.
- `src/db/driver.rs`: `DbQuery` (the query state) gains `known_columns: Option<HashSet<String>>`.
- `src/interpreter/pipeline.rs`: populate `known_columns` from the persistent schema when the table
  has a declared `db:` schema (the registry is in scope here), leaving it `None` otherwise.
- `src/error.rs` and the HTTP mapping: `invalid_identifier` and `unknown_column`, both mapped to 400
  through the clean-error path so SQL never leaks.
- Docs: the `db` namespace page (the identifier rules and the supported `order_by` grammar) and
  `reference/errors.md` (the two new errors), aligned with the 071 lint page so the lint and the
  runtime guard tell one story.
- `docs/spec/SPEC.md` section 4.3: remove the computed-alias sentence and example (the mandatory
  doc-of-record sync, 2.5), recording that `select` accepts column names with computed alias deferred.

### Test requirements

- **Pure classifier unit tests, isolated** (the security pattern of 067 and 071): the identifier
  floor (accepts `price` and `orders.id`, rejects `price; drop`, `price'`, `a--b`, `count(*)`,
  embedded spaces) and the `order_by` mini-parser (accepts `price`, `price desc`, `a, b asc`, rejects
  a bad direction or an illegal column), each tested without a database.
- **Query builder unit tests**: a literal identifier still renders (now quoted), an injection attempt
  in `order_by` / `select` / a filter column is rejected, `order_by("price desc")` and a multi-column
  order still work, and the schema layer rejects an unknown column when `known_columns` is `Some`.
- **Functional coverage end to end against real Postgres** for the four vectors the 071 lint names
  (`order_by`, `like`, `in`, and `select`): a hostile runtime value in each does not execute injected
  SQL (it is rejected), while a legitimate dynamic `order_by(query.sort)` with `"price desc"` sorts
  correctly and a literal `select`/filter still works.

### Coverage analysis

- **VS Code extension**: none (no language surface change; the 071 lint already exists).
- **e2e**: add a route exercising a runtime `order_by` (dynamic sort) so the guard's behavior is
  asserted over the served process, since this is runtime/resolution semantics.
- **Documentation**: the `db` namespace identifier rules and supported `order_by` grammar, plus the
  two new errors in `reference/errors.md`, consistent with the 071 lint page.

## 4. Out of scope

- The 500 error-body leak (the sibling security item: do not send raw provider messages to the
  client). Separate change, separate spec.
- `native_query` (its interpolations are already parameterized).
- Removing or weakening the 071 lint (it stays as the dev-time signal that points at the same vector).

## 5. Acceptance criteria

1. The universal floor (2.1) holds with no schema: any identifier in `order_by`, `select`, or a
   filter column that is not a valid identifier shape is rejected, and what passes is emitted quoted,
   so injection is structurally impossible even for a schema-less table.
2. When a `db:` schema is declared, a floor-passing identifier that is not a known column is rejected
   with `unknown_column` (2.2).
3. `order_by` parses `column [asc|desc]` comma-separated (2.3): a legitimate dynamic
   `order_by(query.sort)` with `"price desc"` works safely, a bad direction or illegal column is
   rejected, and multi-column order works.
4. `select` accepts only bare column identifiers (2.5): a non-identifier (a computed expression) is
   rejected, literal `select("id", "name")` is unchanged, and no literal-versus-runtime origin
   tracking is added (computed aliases stay deferred).
5. The four vectors the 071 lint names (`order_by`, `like`, `in`, `select`) are closed end to end:
   a hostile runtime value in each does not execute injected SQL.
6. Rejections route through the clean error path as `400 invalid_identifier` (illegal form) or
   `400 unknown_column` (valid form, unknown column), never leaking SQL (2.7).
7. The guard is a pure classifier tested in isolation (the floor and the `order_by` parser), plus
   query-builder and functional coverage against real Postgres.
8. Docs updated: the `db` identifier rules and supported `order_by` grammar, and the two new errors,
   consistent with the 071 lint page. **The language spec of record is synced: `SPEC.md` section 4.3
   no longer documents the computed alias** (the example and the "computed aliases" wording removed,
   deferred-to-Phase-5 recorded), so the docs do not contradict the runtime.
9. Standard gates: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, the full test
   suite, plus `functional_tests` and `migrations_functional` (runtime change touching the db path).

---

## Design decisions (resolved in review)

The brainstorm resolved the five questions and surfaced one correction:

- **Q1**: not validate-or-quote-or-reject but a **universal syntactic floor plus quoting** (2.1,
  works without a schema) and an **optional schema layer** (2.2). The load-bearing fact: `db.<table>`
  works without a declared schema, so a schema allowlist cannot be the universal mechanism. Rejecting
  all runtime identifiers is out (it would kill dynamic sort).
- **Q2**: `order_by` is a **mini-parser** of a minimal grammar `column [asc|desc]`, comma-separated
  (2.3). `NULLS LAST/FIRST` is out of this form.
- **Q3**: the guard lives **in the query builder**, with the column allowlist **threaded** into the
  query state as `known_columns: Option<HashSet<String>>` (2.6), one location, pure, isolated-testable.
- **Q4**: **dynamic sort is first-class in this spec** and falls out of the floor plus quoting (2.4);
  an explicit allowlist API for the schema-less typo case is a deferred follow-up.
- **Q5**: **400** through the clean machinery, two codes: `invalid_identifier` and `unknown_column`
  (2.7).
- **Correction (the sharp corner):** the brainstorm's `select` computed-alias rule assumed a feature
  that does not exist. Computed aliases are deferred (Spec 009 `AC-3.5d`), the runtime drops the alias
  name with no `AS`, and the user docs do not expose them. So `select` takes bare identifiers only
  (2.5), no origin tracking, simpler and hole-free. Computed aliases, if ever built, ship with the
  literal-only rule from the start.
- **Doc-of-record sync (review counter-finding):** `SPEC.md` section 4.3 still documents the
  computed alias (line 1028 wording and the line 1033 example), so 076 must correct it in the same
  change (2.5), or the hardening would contradict the language spec. The four live-proof vectors are
  therefore `order_by`, `like`, `in`, and `select`-bare-rejection (rejecting `select("total * 0.9")`),
  not a select-computed-literal case.

### Implementation decisions (recorded from the code review)

- **The count query is a trusted aggregate, not a select column.** The count terminal used to stuff
  `select_cols = ["COUNT(*) AS count"]`, a runtime-injected non-identifier the floor would reject.
  `QueryState` gained `count: bool`, and the builder renders `COUNT(*) AS count` as trusted SQL and
  ignores `select_cols` when it is set, so the only writer of `select_cols` is user input.
- **A rejected identifier is a client 400 and is NOT logged as an uncaught runtime error (security
  policy).** `invalid_identifier` / `unknown_column` return cleanly without emitting a `[marreta]`
  runtime-error line or a structured `runtime_error` event. This is consistent with the sibling
  surface: input validation that returns 422 also returns before touching the DB without logging,
  while the 409 unique violation logs because it is a real database event. Errors barred before the
  database do not log; real DB events do. **Consequence, recorded deliberately:** SQL-injection
  *attempts* are therefore not observable in the event log. Detecting attack probing (a security
  observability surface, for example a dedicated security event or a metric) is a separate concern,
  out of scope here, a possible follow-up. Recorded so it is not "improved" the wrong way later
  (either by silently starting to log client 4xx as runtime faults, or by assuming attempts are
  already observable).

---

## Delivery notes

Delivered. All gates green (`cargo fmt --check`, `clippy -D warnings`, the full suite plus the pure
classifiers, and the runtime tier: `functional_tests` 575/0, `migrations_functional`, e2e),
validated live against real Postgres.

What landed:

- **The guard in `src/db/query_builder.rs`** (pure, isolated-testable): `quote_identifier` (the
  floor: `name`/`table.column`, emitted double-quoted, else `invalid_identifier`) and
  `build_order_by` (the `column [asc|desc]` comma-separated mini-parser), with `validate_column`
  adding the optional schema layer. `build_select`/`build_update`/`build_delete` now return `Result`
  and validate the select columns, filter columns, and order_by.
- **The schema layer**: `QueryState` gained `known_columns: Option<HashSet<String>>`, populated in
  the pipeline from the persistent schema when the table declares `db:` (`None` for a schema-less
  table, which the floor still guards). A floor-passing unknown column is `unknown_column`.
- **The count fix**: `QueryState.count: bool` renders `COUNT(*) AS count` as trusted SQL, so the
  only writer of `select_cols` is user input (recorded in Design decisions).
- **Errors**: `invalid_identifier` and `unknown_column` map to 400 through the clean path, never
  leaking SQL, and are not logged as uncaught runtime errors (the security-policy decision recorded
  in Design decisions; attack-attempt observability is out of scope).
- **Docs**: the `db` namespace identifier rules and `order_by` grammar, the two new errors in
  `reference/errors.md`, and `SPEC.md` section 4.3 synced (computed alias removed, deferred recorded).
- **Functional coverage**: `functional_tests` section 18D exercises the four vectors against real
  Postgres (injection in `order_by`/`like`/`in`/`select` rejected, dynamic sort survives,
  `unknown_column` on a schema-backed table), with the intentional runtime identifiers dogfooding the
  071 suppression valve.

Defense in layers with the 071 lint: the lint warns at dev time, this guard bars at runtime. Closes
the SQL identifier injection vector (CONCERNS.md item 6), the last named security follow-up from 071.

Site: `db.md` and `errors.md` are under `docs/guide`, so the site is synced on delivery.

---

## P.S. Do not forget the docs of record

On delivery, update both `CHANGELOG.md` and `docs/spec/SPEC.md`. See SPEC.md section 1.3.
