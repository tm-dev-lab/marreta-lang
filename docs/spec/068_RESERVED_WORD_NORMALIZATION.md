# 068 - Reserved Word Normalization

> Status: Delivered
> Type: Language syntax (lexer/parser) + tooling
> Scope: Reserve the infrastructure namespaces `doc` and `feature` and the `env` accessor at the
> lexer level, the same way `db`/`cache`/`queue` already are, so a documented namespace can no longer
> be shadowed by a variable. Done with a normalize-back parser so nothing downstream of the
> declaration position changes. Pre-launch: our own corpus is swept, no compatibility burden. The
> only new runtime-visible behavior is blocking these words in a binder position.

---

## 1. Purpose

The reserved-word surface is inconsistent. SPEC.md §13 documents `doc` as a global reserved word (the
document-database namespace, peer of `db`/`cache`/`queue`), but `src/token.rs`'s `keyword_lookup`
reserves only `db`/`queue`/`cache`: `doc` is lexed as an ordinary identifier and recognized later by
name (e.g. `src/doc/index_inference.rs` matches `Identifier("doc")`). So a variable named `doc`
shadows the namespace and the document store silently disappears from that scope - the exact
inconsistency that undermines the "namespaces are reserved" promise the developer experience rests
on. The same gap applies to `feature` (the feature-flag namespace, `src/feature_flags.rs`) and `env`
(the environment accessor). This spec closes the gap and makes the reserved set drift-proof, without
changing anything else about how those namespaces are used.

## 2. The change

### 2.1 The two-layer rule (the teachable thesis)

Reserved words fall into two layers, and the keywords page documents them as such:

- **Layer 1 - reserved.** Words the lexer tokenizes that cannot be an identifier in any binder
  position: the infrastructure **namespaces** (`db`, `queue`, `cache`, `fs`, `json`, `base64`,
  `uuid`, `log`, `time`, `math`, `http_client`, `topic`, and - added here - `doc` and `feature`),
  the **`env`** accessor, the structural keywords, and the type-tokens (`string`/`integer`/`float`/
  `boolean`/`instant`/`date`/`duration`/`interval`).
- **Layer 2 - contextual.** Words meaningful only in one position and free as an identifier
  everywhere else: the `db:` schema directive, the type-names `list`/`decimal`/`enum`, the pipeline
  vocabulary (`where`/`fetch`/`limit`/`count`/`order`...), the scenario DSL (`scenario`/`given`/
  `when`/`then`, which has a test guaranteeing its non-reservation), and the injected bindings
  (`params`, `auth`, `payload`, ...).

Rule, in one sentence: **namespaces are reserved; directives and vocabularies are contextual.**

### 2.2 Reserve `doc`/`feature`/`env` with normalize-back

The lexer emits the new tokens (`Doc`, `Feature`, `Env`). The parser **normalizes them back** to
today's AST (`Identifier("doc")`, etc.) in every position **except a binder (declaration) position**.
So the interpreter, the scenario mocks, the lint, and the existing `Identifier("env")` special-cases
(for example the auth config) are unchanged. The token does real work in exactly one place: blocking
the word in a binder position (§2.5). The cost downstream of declaration is zero.

### 2.3 The contextual layer does not change (one sentence)

`list`/`decimal`/`enum` stay contextual type-names, free as identifiers outside a type position; this
is an existing fact, documented for completeness, not a change. (The pre-rewind plan to normalize the
declarative `index`/`unique` keywords and the `doc:` marker is gone: Spec 067 became inference, so
none of those tokens exist.)

### 2.4 Audited tolerance in name positions

Reserving a word must not break its legitimate, non-declaration uses. The **name positions** are:
after `.` (`payload.doc`), a map key (`{ doc: 1 }`), a schema field name (`doc: string`), a named-arg
name (`where(doc: ...)`), and a column in `select(...)`. The work is threefold:

- **(a) Add** `Doc`/`Feature`/`Env` to the existing manual name-lists (`parse_member_name` for after
  `.`, `expect_identifier_or_keyword_as_key` for a map key, and the schema-field / named-arg /
  `select` paths).
- **(b) Audit** those same lists for the **already-reserved** tokens, because trivial uses like
  `payload.time`, `{ date: 1 }`, `where(time: ...)`, and a `time` column in `select(...)` may be
  broken today; this spec is the place to close those pre-existing holes.
- **(c) Freeze** with a table test: every reserved token × every name position (positive, parses as
  the name) and every binder position (negative, the dedicated error).

A **schema field named `doc`/`feature`/`env` is allowed** via this tolerance (they are not
directives). This differs from `db`, which stays unusable as a schema field because the pre-existing
`db:` directive from Spec 025 already claims that position - a contrast worth one sentence in the
docs.

### 2.5 Dedicated error in a binder position

Binding one of the reserved namespaces as a name fails with a dedicated message, e.g.:
`'doc' is a reserved word (the document database namespace); rename the variable.`

The **binder positions** that must block - each a negative table-test case - are: an assignment
target, a task name, a **task parameter** (`task f(doc)`), a **map/reduce block variable**, a
**schema name**, an **auth provider name**, a **consumer `take` binding** (the dev-chosen name in
`on queue "orders" take <name>`, and likewise `on topic`), and a **route path parameter**
(`route GET "/x/:doc"`). Enumerating them guards the one risk of normalize-back: accidentally
tolerating a binder because it resembled a name position.

The **route path parameter is special**: the name lives inside the route string literal, so the
lexer never emits a `Doc`/`Feature`/`Env` token there. The block therefore happens at **load /
route-registration** time (`route GET "/x/:doc"` is rejected with the same dedicated message), and in
the table test this one case is a **load test, not a parse test**.

### 2.6 Catalog→token invariant

A test asserts that **every `CatalogKind::Namespace` has a token in the lexer** (extending the Spec
061 catalog guardrail), so a future namespace is reserved by construction and the `doc` drift cannot
recur. Note: **`env` is not a catalog namespace** (it is the environment accessor, not a provider),
so the invariant does not cover it; `env`'s token is added and tested directly.

## 3. Implementation outline

- **Lexer** (`src/token.rs`): `keyword_lookup` maps `doc`/`feature`/`env` to new `TokenKind`s, peers
  of `db`/`queue`/`cache`.
- **Parser**: normalize-back at every name position (the §2.4 lists); block with the dedicated error
  at the binder positions (§2.5).
- **Catalog / tooling**: the catalog→token invariant test (§2.6); `doctor` and `lint` are unaffected
  because normalize-back keeps the AST identical downstream.
- **Corpus sweep**: migrate any `doc`/`feature`/`env` identifier in a binder position across our own
  `.marreta` (examples, e2e, benchmarks, `docs/guide` snippets) **and** the `marreta init` templates
  (the operational strings in `src/init.rs`).
- **VS Code extension**: grammar tokens + completions for the new reserved words (the extension stays
  a thin CLI client).
- **Docs of record**: SPEC.md §13 + CHANGELOG. The keywords-page two-layer writeup is deferred to
  Spec 069.

## 4. Out of scope

- Reserving `index`/`unique`/`list`: the first two no longer exist (067 is inference), and
  `list`/`decimal`/`enum` stay contextual type-names.
- The **`shadows-injected-binding` lint** - flagging a local that shadows an injected binding
  (`params`, `auth`, ...). That is a lint concern, not reserved-word reservation; tracked as a sister
  follow-up in SPEC.md §1.4.
- The keywords-page guide writeup (the two-layer rule), deferred to Spec 069 with the other 067/068
  guide docs.
- The cross-repo site follow-up.

## 5. Acceptance criteria

1. `doc`, `feature`, and `env` are lexer keyword tokens; binding any of them as a name fails with the
   dedicated reserved-word message. The enumerated binder positions (assignment, task name, task
   parameter, map/reduce block variable, schema name, auth provider name, consumer `take` binding,
   and route path parameter) are negative table-test cases - the route path parameter blocked at
   load / route-registration time (the name is inside the route string literal), the rest at parse.
2. Normalize-back holds: every name position (`.` member, map key, schema field name, named-arg name,
   `select(...)` column) parses the word as that name, and a schema field named `doc`/`feature`/`env`
   is expressible (with the documented contrast against `db`, claimed by the `db:` directive).
3. The pre-existing name-position holes for already-reserved tokens (`payload.time`, `{ date: 1 }`,
   `where(time: ...)`, a `time`/`date` column in `select`) are audited and closed; the table test
   (every reserved token × every name position positive, every binder position negative) passes.
4. The catalog→token invariant test asserts every `CatalogKind::Namespace` has a lexer token; `env`
   is added and tested directly, with a one-line note that it is not a catalog namespace.
5. Our own corpus and the `marreta init` templates contain no `doc`/`feature`/`env` identifier in a
   binder position, and the example / e2e / benchmark suites stay green.
6. The VS Code extension tokenizes, colors, and completes the new reserved words.
7. Standard gates: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, the full test
   suite, `functional_tests`, `migrations_functional`, and `e2e`; for the extension, `node --check`
   plus a VSIX package.

---

## P.S. Do not forget the docs of record

On delivery, update both `CHANGELOG.md` and `docs/spec/SPEC.md`. See SPEC.md section 1.3.

---

## Delivery notes

Foundation (lexer tokens + normalize-back at the primary/member positions + the shared
`reserved_word_as_name` round-trip) landed at `8dd8dda`. This delivery completed it:

- **Dedicated error** (`src/error.rs`): `MarretaError::ReservedWord` with a per-namespace role
  message, wired into `expect_identifier` so every strict binder fails uniformly; the statement
  dispatcher routes `<reserved> = ...` (and `export <reserved> = ...`) into the assignment path so
  the assignment target blocks too. The consumer `take` binding got an explicit reserved-word arm.
- **Name positions** (`src/parser.rs`): unified on one tolerant `expect_name` (identifier or any
  reserved word as a name), replacing the after-`.`, map-key, schema-field, `db:`-table, and
  named-arg hand-rolled lists; the type tokens normalize back in expression position (`select(date)`).
  The former `expect_identifier_or_keyword_as_key` was removed as redundant.
- **Route path parameter** (`src/route_loader.rs`): `validate_path_params` rejects a reserved
  `:param` at load with the same dedicated error (load test, not parse).
- **Tests**: catalog→token invariant + `env`-direct (`src/tooling/catalog.rs`); the reserved-word
  table test over every name position (positive) and binder position (negative) plus the load-time
  path-param test (`src/parser.rs`, `src/route_loader.rs`). Unit suite 1542 green.
- **Corpus + extension**: swept `docs/examples`, `docs/benchmarks`, `e2e`, and `src/init.rs`
  templates — no binder uses of `doc`/`feature`/`env`. The VS Code grammar already lists them as
  namespaces and completions are catalog-driven, so no extension change was needed.
- **Gates**: `fmt`, `clippy -D warnings`, unit suite, `functional_tests` 567/567,
  `migrations_functional` PASS, `e2e` PASS (+18 live smoke), extension `node --check` + VSIX.
