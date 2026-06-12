# Marreta Lang — Language Specification

> **Domain:** marreta.dev
> **Document version:** 0.4.2
> **Status:** Living specification

---

## 1. Overview

Marreta Lang is a **Domain-Specific Language (DSL)** designed exclusively for building REST APIs. Its goal is to completely eliminate the boilerplate found in traditional frameworks (Spring Boot, Express, Django REST, etc.) by embedding HTTP, database, messaging, and cache as **first-class citizens** of the language.

The language is **interpreted**, with the execution engine (runtime) written in **Rust**, ensuring memory safety, high performance, and native concurrency without the developer needing to deal with any of these complexities.

### 1.1 Philosophy

- **Natural reading:** code should read like a business specification, not machine instructions.
- **Zero ceremony:** no semicolons, no braces, no explicit type declarations, no infrastructure library imports.
- **Full infrastructure abstraction:** domain code never references concrete providers (Postgres, Redis, RabbitMQ). Binding is done through external configuration.
- **Convention over configuration:** inspired by the Rails philosophy, the engine assumes sensible defaults and the developer only configures what differs.
- **Minimal reserved words:** the language has the smallest possible set of reserved words, prioritizing operators and visual structure.

### 1.2 Runtime Stack (Rust)

| Layer | Rust Crate | Purpose |
|---|---|---|
| HTTP Server | `axum` or `hyper` | High-performance async web server |
| Relational DB | `sqlx` | Async connection to PostgreSQL (initial driver) |
| NoSQL DB | `mongodb` | Official async driver for MongoDB |
| Messaging | `lapin` | Async AMQP client for RabbitMQ (initial driver) |
| Cache | `redis-rs` | Async Redis client |
| Async Runtime | `tokio` | Event loop and task scheduler |
| Memory Management | `Rc<RefCell<T>>` | Reference counting without complex GC |

### 1.3 Spec Maintenance Conventions

Feature specs must use a single phase taxonomy from plan to validation. Prefer
`Phase 1`, `Phase 2`, and so on; do not mix numeric phases with lettered
tracks in the same document. `Test Plan` sections should reference the same
phase names used by the `Implementation Plan`, and delivered specs should add a
short delivery-notes block mapping phases to commits when useful.

Keep the docs of record in sync with every spec. A spec is not done until both
`CHANGELOG.md` (the Current Status note, plus a delivery entry when warranted) and
this file (the Active Follow-Ups section, plus any user-facing syntax or behavior
that changed) reflect it. Update them as part of the delivery, not afterward.

User-facing documentation is part of the definition of done (Spec 064). Any spec that
adds or changes a namespace, method, keyword, `MARRETA_*` variable, CLI command, or error
code updates the authored guide under `docs/guide/` in the same change. This Documentation
axis sits alongside the VS Code extension and e2e axes in the per-spec coverage analysis,
and `.github/PULL_REQUEST_TEMPLATE.md` carries the checklist so it is not skipped at review.

Rewind versus supersede. A spec that has **never reached a release** may be **rewound** rather
than superseded forward: revert the branch to before its delivery (force-push), reuse the spec
number for the corrected design, and preserve the reverted objects under a tag so nothing is lost.
A spec that has **shipped in a release** gets a **forward superseding spec** instead (the Spec 052
supersedes Spec 047 precedent), because something out there may have consumed the old state. Before
a rewind, verify no release tag descends from the commits being removed; if one does, the rewind is
vetoed. The reverted spec records a short History note (what was reverted and why) so the U-turn
reads as learning, not churn. Spec 067 was the first rewind under this rule (declarative document
indexes reverted in favor of inference; reverted objects at tag `pre-067-revert`).

### 1.4 Active Follow-Ups

- No active implementation follow-up is currently required for specs 026-059.
  The 026-047 block (block `if/else`, `time`, `math`, `fs`, `json`, `base64`,
  `log`, runtime request logging, `uuid`, W3C Trace Context, async trace
  propagation, the runtime event log contract, project initialization, feature
  flags, schema constructors, API contract types, formatter, lint, editor
  tooling, and OpenAPI docs refinement) all have delivered specs and working
  implementations.
- Specs 048-053 are also delivered: runtime/CLI versioning (single source in
  `Cargo.toml`), hot-path profiling, the single-engine AST hot-path optimization
  (which retired the route execution template fast path explored in 050), the
  repository re-consolidation into the engineering monorepo (Spec 052, which
  supersedes the Spec 047 split), pre-release code quality and hardening
  (Spec 053), a consolidated, static test-coverage summary in `marreta doctor`
  (Spec 054), and the `docs/` layout grouping plus the `e2e/` feature suite
  (Spec 055, which amends the Spec 052 layout).
- Spec 056 is delivered: a one-line `curl ... | sh` installer (`install.sh`)
  plus a manual `install.yml` workflow that validates it across the full OS matrix
  against a published release tag. Its acceptance item for the matrix run is
  exercised when a release is cut and the workflow is dispatched, the same model
  as the Release Smoke Test and Release E2E.
- Spec 057 is delivered: trimmed the CLI to the API workflow by removing the `run`
  script-runner and the `repl` interactive evaluator (both off-identity scripting
  surfaces), hiding the `tokenize` and `parse` debug commands from the public help
  while keeping them callable, and making bare `marreta` print help. The 72
  `run`-based integration tests were removed with no runtime change, their coverage
  confirmed redundant with the interpreter unit tests plus `e2e` and
  `functional_tests`.
- Spec 058 is delivered: the one-shot, human-facing commands (`init`, `fmt`,
  `lint`, `doctor`, `test`, `migrate`) now print a consistent frame — a header rule,
  the unchanged body, and a footer with a one-line status summary and elapsed time
  (horizontal rules only). The frame goes to `stderr` (cargo model) so `stdout` data
  stays clean; `serve`, machine/JSON modes, and the debug commands stay unframed.
- Spec 059 is delivered: fixed and enriched the VS Code extension. CLI contract
  (interpolation unused-variable fix, lint diagnostic spans, `tooling definition`
  go-to-definition, `auth` in `tooling symbols`, project-less `--stdin`); extension
  core (definition provider, standalone-file support, visible CLI-missing
  notification, full-token spans, indentation fix); enrichment (purple mallet icon +
  opt-in file icon theme, richer snippets, palette commands, CodeLens, "Remove
  unused variable" quick-fix, status bar, marketplace polish). The extension stays a
  thin client of the CLI — all language analysis is in `marreta tooling …`.
- Spec 060 is delivered: the messaging producer side is split — topics publish via a
  dedicated `topic.publish` namespace and queues keep `queue.push`, symmetric with
  `on topic` / `on queue`. `queue.publish` was removed (clean pre-release cutover),
  the operational surface (errors, `operation`, scenario `given`) reads
  `topic.publish`, and the catalog is now correct against the parser.
- Spec 061 is delivered: each `.marreta` file is an inferred namespace from its file
  name, so exported tasks are called `file.task()` (consistent with `db.find`), making
  origin visible with no imports. Pre-release breaking change: cross-file exported
  tasks are reached only via `file.task()` (calls, pipeline `>> file.task`, and
  broadcast `-> file.task`); bare resolves only in the declaring file and from
  `app.marreta`. Collisions (same stem, a built-in namespace name, the reserved `app`,
  or a non-identifier stem) are load errors; only exported tasks are externally
  reachable, and the same task name may repeat across files. A private task reached as
  `file.task` reports the qualified task as undefined. `doctor` gains an informational
  Modules section (file-namespaces → exports), lint gains `unused_exported_task` (and
  now counts bare pipeline/broadcast targets as uses), and the VS Code extension colors
  file-namespaces and completes/jumps to their exported tasks (thin CLI client).
- Spec 062 is delivered: the payload validator is relation-aware — a reference to a
  persistent (`db:`) schema is a foreign-key relation (Spec 025) and is let through, not
  recursed, so a persistent schema works as an API contract even when it participates in
  a relation cycle (`take payload as User` with `User <-> Order` terminates: own fields
  validated, relations let pass). With persistent references let through, the only
  validator loop left is an all-value-schema cycle; the loader re-gains that check on the
  live `serve`/`test`/`doctor` path (`CircularSchemaReference`), shared with `marreta
  lint` via one relation-aware helper (`schema_cycle`), and the dead
  `route_loader::detect_circular_references` was removed. A cycle through any persistent
  schema is allowed; only an all-value cycle fails at load. functional_tests section 37
  covers the runtime "allowed" scenarios over HTTP.
- Spec 063 is delivered: a project declares the minimum Marreta Lang runtime it needs via
  a new `requires_marreta` field in the committed manifest (`app.marreta`), and the
  runtime enforces it at load with a clear config error (`IncompatibleRuntime`) — the
  language/runtime compatibility contract, equivalent to `package.json engines` /
  `go.mod` / `Cargo.toml rust-version`. It is orthogonal to `project_version` (the
  product's own version) and lives in the manifest, never in the gitignored, per-deploy
  `marreta.env`. Pairs with a language-versioning (semver) policy that makes the number a
  reliable signal. Scope is the "app needs runtime >= X" axis; the reverse (old app on a
  newer runtime) stays for a deprecation policy / future editions.
- Spec 064 is delivered: a single Markdown documentation tree under `docs/guide/`
  (Diátaxis: tutorials / how-to / reference / concepts) authored by hand, serving GitHub
  and the future site (Spec 065). A one-off bootstrap generator produced the first complete
  reference and was then removed — there is no `src/docs.rs`, no `marreta tooling docs`, and
  no docs CI gate. Coverage is kept current by process: the marreta-spec delivery checklist
  gains a Documentation axis (alongside the VS Code extension and e2e axes),
  `.github/PULL_REQUEST_TEMPLATE.md` carries the docs checklist, and §1.3 records the docs
  DoD axis. Every code example is lifted from, or verified against, a tested project under
  `docs/examples`. The delivery also fixed a scenario-mock parity bug so
  `given topic.publish … returns …` works (it registered under the `topic` namespace while
  the queue driver resolves publish under `queue`), with a regression test. Site rendering
  (Spec 065) and an `explanation/` Diátaxis mode remain for later.
- Spec 065 is delivered: four developer-experience improvements — a token-aware `marreta fmt`
  that also normalizes intra-line spacing per the house style (comment/string-safe, guarded by a
  layout-aware token-stream invariant plus idempotency), a clear `invalid indentation` parser
  error for the unexpected-indent case (was a generic "expected expression"), provider
  connection progress logs at `marreta serve` startup (configured providers only), and an
  authenticated MongoDB healthcheck in the `marreta init` docker-compose template (closing the
  `docker compose up --wait` race). Cross-repo follow-up: re-sync the site initializer fixtures
  (Spec 008) since `marreta init`'s compose changed.
- Spec 067 is delivered: inferred document indexes. The runtime infers each collection's index
  from its query surface (the `where`/`order` shape of every `doc.<collection>` / `doc.query("col")`
  pipeline, by the ESR rule with prefix dedup) and ensures it in MongoDB in the background at
  `serve` startup, with no declaration, no `doc:` marker, and no migration. It superseded a reverted
  pre-release declarative approach under the §1.3 rewind rule (reverted objects at tag
  `pre-067-revert`); the `unique_violation` 409 mapping and the doc-driver ensure machinery were
  preserved by cherry-pick. Cross-repo follow-up: update the "Spec 067" references in
  marreta-lang-stealth (security section, backlog) from the declarative meaning to the inference one.
- Spec 066 is delivered: the rigorous, publishable launch study in `docs/benchmarks/digital_bank`
  validating the three about-page hypotheses (low/predictable resources, strong performance despite
  high abstraction, developer experience). Four feature-identical contenders (Marreta, FastAPI,
  NestJS, Spring Boot), a 120s JIT warmup + 300s steady-state window, three interleaved repetitions
  with median + CV and a consistency gate over the full grid, fixed load levels + a 250-step
  saturation ladder, expanded resource metrics (incl. startup at 20ms and test-suite time), and an
  objective DX measurement (total SLOC, dependencies/footprint, capability matrix). Re-run post-067
  on a dedicated VM; the business/wiring split was dropped and the whole `results/` tree is
  regenerable (`RESULTS.md`/`METHODOLOGY.md`/`DX.md` are the record). No runtime change.
- Spec 068 is delivered: reserved-word normalization. The namespaces `doc` and `feature` and the
  `env` accessor are reserved at the lexer level (peers of `db`/`cache`/`queue`), so a documented
  namespace can no longer be shadowed by a variable, via a normalize-back parser - the new token
  only blocks a binder position (with a dedicated `'doc' is a reserved word ...; rename the variable.`
  error at every binder: assignment, task name, task parameter, map/reduce variable, schema name,
  auth provider name, consumer `take` binding, and the route path parameter, the last blocked at
  load since the name lives inside the route string literal). Every name position (after `.`, map
  key, schema field name, named-arg name, `select` column) is normalized back to today's AST through
  one shared `expect_name` helper, which also closed the pre-existing holes where the already-reserved
  tokens (`db`, `date`, ...) were missing from each position's hand-rolled list. A schema field named
  `doc`/`feature`/`env` is allowed (they are not directives); `db` stays unusable there because the
  `db:` directive claims that line. A catalog→token invariant test asserts every `CatalogKind::Namespace`
  has a lexer token (`env` excepted as a non-catalog accessor, tested directly), and a table test
  freezes every reserved token × every name position (positive) and every binder position (negative).
  Our own corpus and the `marreta init` templates were swept (no binder uses found); the VS Code
  extension already tokenizes/colors the words (grammar) and completes them (catalog-driven). Post-rewind
  trim of the pre-067 spec: the declarative `index`/`unique` keywords and the `doc:` marker are gone,
  so that half is dropped.
- Spec 069 is delivered: the hand-authored `docs/guide` pages for 067 and 068, which shipped with
  only their docs of record. An Indexes section on `reference/namespaces/doc.md` (the runtime infers
  each collection's index from its query surface and ensures it at `serve` startup, no declaration /
  marker / migration, reported by `marreta doctor`, with the exclusion / background-ensure /
  never-drop / hand-made-index boundaries), the deferred two-layer reserved/contextual model framing
  `reference/keywords.md` (namespaces are reserved, directives and vocabularies are contextual;
  reserved words free in a name position, blocked as a binder), a cross-linking sentence on
  `concepts/namespaces.md`, and the `reference/errors.md` gaps (the live `unique_violation` 409 code,
  the automatic-422 validation note, and the overstating intro fix). Docs-only, no runtime change.
- Spec 070 is delivered: a release path for the VS Code extension. A manual
  (`workflow_dispatch`) `release-vscode.yml` that versions from `docs/editors/vscode/package.json`
  (tag-equals-version guard), builds the VSIX on a single runner, and publishes a GitHub Release under
  a `vscode-v*` tag namespace, with `make_latest:false` so it never steals the installer's
  `releases/latest` (no existing workflow changes). The same workflow publishes to Open VSX (Cursor
  and forks) and the MS Marketplace (stock VS Code), each gated on its secret with a loud per-channel
  skip, and ships the binary-first, palette-first install how-to under `docs/guide`. Proven by a green
  dispatched dry run (`vscode-v0.2.18`). Shipped alongside: curated release bodies for both the
  runtime and the extension (the runtime drops the auto changelog, first release has no changelog
  section by decision, the second introduces hand-curated highlights).
- Spec 071 is delivered: a lint DX pass. Grew `marreta lint` from its minimalist eight rules into a
  focused launch surface: `shadows-injected-binding` (subsumes the 068 sister follow-up),
  `route-without-response` (a route that silently 204s), `match-without-fallback` (silent `Null`),
  `non-literal-sql-identifier` (a SQL identifier built from a runtime value, the order_by injection
  vector from the security CONCERNS), and `unused-schema` / `unused-auth-provider`, on top of inline
  suppression (`# marreta: allow <code>`), a single rule catalog (the drift-proof source for docs +
  editor links + an invariant test), the `reference/lint` docs page, editor `codeDescription` links,
  and a suppress quick-fix (ordered after any real fix).
- Spec 072 is delivered: a fmt consistency pass. One correctness fix (the formatter walked only
  `routes`/`schemas`/`tasks`/`tests`, silently skipping files in custom dirs like `auth/` that the
  loader loads; now shares the loader's recursive discovery, pinned by an invariant test) plus four
  invariant-safe normalizations (collapse blank-line runs to one, exactly one final newline, strip
  file-edge blanks, `#comment` to `# comment`), and a documented stance for the non-goals (no
  wrapping, alignment, sorting, reflow). The final-newline rule required refining the token-stream
  snapshot (`significant_tokens`) to drop the file-terminal `Newline` even when it sits behind
  synthesized `Dedent`s, a foundation touch surfaced in review and guarded by unit tests; the parked
  spec's "safe by construction" claim was corrected to the verified fact. Lint discovery was already
  recursive, no fix there.
- Spec 073 is delivered: a migrate roundness pass. Two surgical fixes to `marreta migrate`. (1) The
  replay that derives schema state for `diff`/`generate` rejected any SQL it did not generate,
  trapping a developer who hand-writes a legitimate migration (an index, a backfill) with no
  non-destructive exit; now it tolerates provably schema-neutral statement classes, honors a
  `-- marreta: skip-replay` marker, gives an actionable rejection error, and uses a statement
  splitter that is string-, dollar-quote-, and comment-safe. (2) `diff`/`generate` were silent about
  changes the additive-only planner does not support (a column type change reported "up to date");
  now they report the drift (type, nullability, removed field, removed table) without acting on it.
  Plus a same-second `generate` collision guard. The code review caught a third gap the original live
  probes missed (an apostrophe or `;` inside a comment in hand-written SQL), fixed in the splitter.
  Verified live against real Postgres.
- Follow-up (security): `db identifier hardening` - the `non-literal-sql-identifier` lint (Spec 071)
  warns at dev-time when a `db` identifier (`order_by` / `select` alias / `like`/`in` field) is built
  from a runtime value, the `order_by` injection vector; the runtime guard (quote, validate against
  known columns, or reject in the query builder) is a separate security change with its own design
  and runtime gate tier. Defense in layers with the lint, not a replacement for it.
- The remaining public-v1 gaps should now be tracked as new explicit specs,
  not as open follow-ups from the delivered block.

### 1.5 Language Versioning Policy

The runtime version (`Cargo.toml`, exposed as `version::MARRETA_VERSION`) is the
language/runtime compatibility signal that projects pin via `requires_marreta`
(Spec 063). To keep the number meaningful:

- A **breaking** language/runtime change bumps a defined component — the **minor** while
  `0.x` (pre-1.0), the **major** once `1.0` is reached.
- The **same** breaking-change release advances the **compatibility floor**
  (`version::COMPAT_FLOOR`) to that version, in the same PR; **additive** (non-breaking)
  releases bump the version but leave the floor unchanged. The floor is always
  `<= MARRETA_VERSION`.
- `marreta init` stamps `requires_marreta = ">=<COMPAT_FLOOR>"`, so a scaffold runs on
  any runtime from the last breaking version up.

Today both `MARRETA_VERSION` and `COMPAT_FLOOR` are `0.2.0`.

---

## 2. Core Syntax

### 2.1 Syntactic Principles

1. **End of statement** is defined by line break (`\n`), never by `;`.
2. **Scope** is defined by indentation (Python-style), never by `{}`.
3. **Everything is an expression:** conditionals, `match` blocks, and tasks always return a value. The last evaluated expression is the implicit return.
4. **Strings** use double quotes `"` with interpolation via `#{}`. Any valid expression can appear inside `#{}`.
5. **`fail` is a Never expression:** `fail` can appear as a statement or on the right-hand side of an expression (e.g. `match` arm). It always short-circuits the route — it never produces a value.

### 2.2 Variables

Silent type inference. The Rust engine determines the type at assignment time in the AST.

```marreta
name = "Payments API"
version = 1.0
active = true
items = [1, 2, 3]

# String interpolation — any expression inside #{}
message  = "Starting #{name} v#{version}"
summary  = "#{items.length()} items in cart"
greeting = "Hello #{user.name or 'guest'}, score: #{score * 1.0}"
```

**Types supported by the runtime:**

| Type | Example | Rust Representation |
|---|---|---|
| Integer | `42` | `i64` |
| Float | `3.14` | `f64` |
| String | `"hello"` | `String` |
| Boolean | `true`, `false` | `bool` |
| List | `[1, 2, 3]` | `Vec<Value>` |
| Map | `{ name: "Ana", age: 30 }` | `HashMap<String, Value>` |
| Instant | `time.now()` | `DateTime<Utc>` |
| Date | `time.date("2026-04-27")` | `NaiveDate` |
| Time | `time.at("09:30:00")` | `NaiveTime` |
| Duration | `time.minutes(90)` | duration |
| Interval | `time.interval(a, b)` | interval pair |
| Null | `null` | `Option::None` |

### 2.3 Conditionals

Marreta Lang supports conditionals in four complementary forms:

#### 2.3.1 Block `if/else`

Used for general multi-line branching. `if/else` is an expression: it returns
the last evaluated expression from the chosen branch.

```marreta
status = if balance > 0
    "positive"
else
    "empty"
```

```marreta
if cached
    reply 200, cached
else
    fresh = db.orders.find(params.id)
    reply 200, fresh
```

**Semantics:**
- `if/else` is an expression.
- `else` is optional.
- If the condition is false and `else` is omitted, the expression returns `null`.
- `else if` is syntactic sugar for nested `if`.
- Variables assigned inside a branch are block-scoped to that branch.
- `reply` and `fail` inside a branch preserve their normal early-return behavior.

#### 2.3.2 Guards (`require` / `reject`)

Used for validations that interrupt the flow on failure. They are the idiomatic way to validate input in REST routes.

```marreta
# String message
require payload.user_id else fail 400, "User is required"
require payload.items   else fail 400, "Cart is empty"

# Structured error body — any expression
require payload.name else fail 400, { error: "name required", field: "name" }
require row          else fail 404, { error: "not found", id: params.id }

# Rejects if the condition is true
reject client.delinquent else fail 402, "Payment pending"
```

**Semantics:**
- `require EXPR else fail CODE, BODY` → if `EXPR` is falsy (null, false, empty), returns HTTP `CODE` with `BODY`.
- `reject EXPR else fail CODE, BODY` → if `EXPR` is truthy, returns HTTP `CODE` with `BODY`.
- `BODY` is any expression: string literal, map literal, variable, or task call.

#### 2.3.3 Conditional suffix

For simple one-line conditional assignments:

```marreta
status = "approved" if balance > 100
fee = 0.0 if client.type == "VIP"
```

#### 2.3.4 Pattern Matching (`match`)

For logic branching with multiple paths:

```marreta
fee = match client.type
    "VIP"     -> 0.0
    "PREMIUM" -> 5.0
    fallback  -> 15.0
```

**`fallback`** is the reserved word for the default case (equivalent to `default` or `_`).

`match` is always an expression — it returns the value of the matching arm. Because `fail` is a Never expression, it can appear in any arm without breaking this property: the route short-circuits before the match result is used.

```marreta
# fail in fallback arm — route exits if no arm matches
discount = match client.tier
    "gold"   -> 0.20
    "silver" -> 0.10
    fallback -> fail 400, { error: "unknown tier", tier: client.tier }

# fail in any arm
result = match status
    "ok"    -> process(payload)
    "retry" -> fail 429, "too many requests"
    fallback -> fail 400, { error: "unknown status" }
```

### 2.4 Pipelines (`>>`)

The `>>` operator is the heart of Marreta Lang. It pushes data from one stage to the next, left to right.

#### 2.4.1 Simple pipeline (single operation)

```marreta
# Push all items to be saved in the database
payload.items >> db.inventory.save

# Push to the messaging queue
payload.items >> queue.push("processing")

# Push to cache
result >> cache.set("last_query")
```

When the input is a list, the engine applies the operation to each element automatically (implicit iteration).

#### 2.4.2 Multi-stage pipeline

```marreta
payload.items >> db.inventory.save >> queue.push("invoices")
```

#### 2.4.3 Pipeline with transformation (`map` / `keep`)

When the transformation requires multiple lines, we use the `map` block with `keep` to return the transformed data back to the pipeline:

```marreta
orders = payload.items
    >> map item
        item.discount = calculate_discount(item, client.category)
        item.total = item.price - item.discount
        item.status = "processed"
        keep item
    >> db.orders.save
```

**Semantics:**
- `map VARIABLE` opens a transformation block. Each element of the input list is bound to `VARIABLE`.
- `keep EXPR` — keeps the element with the given value; block ends.
- `keep EXPR if COND` — if `COND` is true, keeps with `EXPR` and block ends; if false, falls through to the next statement.
- `skip if COND` — if `COND` is true, drops the element from the result and block ends; if false, falls through.
- If the block ends with no `keep` or `skip` firing, the element is **dropped implicitly**.
- Indentation defines the `map` block scope.

```marreta
# Guard + transform (skip if acts as a filter at the top)
processed = orders >> map order
    skip if not order.active
    skip if order.total <= 0
    keep { id: order.id, net: order.total * 0.9 }

# Cascading alternatives — first matching keep wins
classified = items >> map item
    keep "premium"  if item.score > 100
    keep "standard" if item.score > 50
    keep "basic"    if item.score > 0
    # score <= 0: element is dropped implicitly

# Mixed: filter and transform in one pass
result = records >> map r
    skip if r.deleted
    keep r.value * 2 if r.type == "double"
    keep r.value
```

#### 2.4.4 Broadcast (`*>>`)

Sends the same data to multiple destinations in parallel:

```marreta
saved_orders *>>
    -> queue.push("payment_processing")
    -> queue.push("invoice_emission")
    -> cache.set("latest_orders")
```

**Semantics:** The Rust engine executes each destination in a separate `tokio` task, concurrently.

### 2.5 Tasks (Functions)

Reusable logic blocks. We don't use `function`, `def`, or `fn`.

#### 2.5.1 Inline task (single expression)

```marreta
task apply_discount(value) => value * 0.90
```

#### 2.5.2 Task with body

```marreta
task calculate_tax(item)
    base = item.price * 1.15
    discount = base * 0.05 if item.promotion
    base - discount
```

The last evaluated expression is the implicit return. There is no `return`.

#### 2.5.3 Usage in pipelines

```marreta
payload.items >> task(calculate_tax) >> db.orders.save
```

### 2.6 Operators

| Operator | Description |
|---|---|
| `+`, `-`, `*`, `/` | Arithmetic |
| `%` | Modulo |
| `==`, `!=` | Equality |
| `>`, `<`, `>=`, `<=` | Comparison |
| `and`, `or`, `not` | Logical (words, not symbols) |
| `>>` | Pipeline (pushes data to next stage) |
| `*>>` | Broadcast (parallel pipeline to multiple destinations) |
| `->` | Mapping arrow (used in `match` and broadcast) |
| `=>` | Definition arrow (used in inline task) |
| `.` | Property / method access |
| `[expr]` | Subscript access — string key on maps, integer index on lists |

### 2.7 Subscript Access (`expr[key]`)

Any map or list can be accessed with the `[key]` suffix. `key` is any expression.

**Map subscript** — enables access to keys that are not valid identifiers (e.g. hyphenated header names) and dynamic key access:

```marreta
# Hyphenated keys — not accessible via dot notation
token   = headers["x-api-key"]
id      = headers["x-request-id"]
ct      = headers["content-type"]

# Dynamic key
field   = "x-correlation-id"
corr_id = headers[field]
```

**List subscript** — positional access by integer index (zero-based). Out-of-bounds returns `null`:

```marreta
first  = items[0]
second = items[1]
last_n = items[items.length() - 1]
nth    = items[n]           # n is a variable
```

Dot notation (`obj.key`) and subscript notation (`obj["key"]`) are interchangeable for identifier-safe keys.

### 2.8 String Methods

| Method | Signature | Returns |
|--------|-----------|---------|
| `length` | `str.length()` | `Integer` |
| `upper` | `str.upper()` | `String` |
| `lower` | `str.lower()` | `String` |
| `trim` | `str.trim()` | `String` |
| `contains` | `str.contains(sub)` | `Boolean` |
| `starts_with` | `str.starts_with(prefix)` | `Boolean` |
| `ends_with` | `str.ends_with(suffix)` | `Boolean` |
| `replace` | `str.replace(old, new)` | `String` |
| `split` | `str.split(sep)` | `List[String]` |

```marreta
path.starts_with("/api")          # true
"webhook-prod".ends_with("prod")  # true
```

### 2.9 Comments

```marreta
# Single-line comment (shell/Python style)
```

No block comments in the MVP.

---

## 3. HTTP Module

Routes are declared at the root level of the file. The Rust engine automatically registers each route on the web server when loading the script.

### 3.1 Route Declaration

```marreta
route VERB "PATH" [take BINDING [, BINDING ...]]
    # route body
    reply CODE, DATA
```

**Supported verbs:** `GET`, `POST`, `PUT`, `PATCH`, `DELETE`

### 3.2 URL Parameters

```marreta
route GET "/users/:id"
    user = db.users.find(id)
    require user else fail 404, "User not found"
    reply 200, user
```

The engine automatically extracts `:id` from the URL and injects it as a variable in the route scope.

### 3.3 Request Bindings (`take`)

`take` binds one or more request sources to variables in the route scope. Multiple bindings are separated by commas:

```marreta
route POST "/checkout" take payload, headers
    require payload.cart else fail 400, "Carrinho vazio"
    reply 201, { ok: true }
```

**Binding types:**

| Binding | Variable type | Description |
|---|---|---|
| `take payload` | `Map` | JSON request body (`application/json`) |
| `take query` | `Map` | Query string parameters (`?key=value`) |
| `take headers` | `Map` | Request headers (identifier-safe names only) |
| `take form` | `Map` | Form-encoded body (`application/x-www-form-urlencoded`) |
| `take raw` | `String` | Raw body bytes as string (webhooks, HMAC verification) |

All bindings can be combined:

```marreta
route POST "/webhook" take raw, headers
    require headers.xsignature else fail 401, "Missing signature"
    reply 200, { received: true }
```

### 3.4 Query Parameters

```marreta
route GET "/products" take query
    limit = query.limit or 10
    page = query.page or 1
    products = db.products.find_all(limit: limit, offset: page)
    reply 200, products
```

### 3.5 Request Body (JSON)

```marreta
route POST "/users" take payload
    require payload.name else fail 400, "Name is required"
    require payload.email else fail 400, "Email is required"
    user = db.users.save(payload)
    reply 201, user
```

### 3.6 Form Data

```marreta
route POST "/contact" take form
    require form.email else fail 422, "Email obrigatório"
    reply 200, { received: true }
```

### 3.7 Raw Body (Webhooks)

```marreta
route POST "/webhook/stripe" take raw
    require raw else fail 400, "Empty body"
    reply 200, { ok: true }
```

### 3.8 Headers

```marreta
route GET "/debug/headers" take headers
    reply 200, {
        accept: headers.accept,
        api_key_present: headers["x-api-key"] != null
    }
```

> **Note:** Header names with hyphens (e.g. `x-api-key`) are not accessible via dot notation because `-` is the subtraction operator. Use subscript notation instead: `headers["x-api-key"]`. Identifier-safe names work with both dot and subscript: `headers.accept` and `headers["accept"]` are equivalent.

### 3.9 Authentication and Authorization

Authentication is declarative. Application routes should not manually parse
JWTs, validate API keys, or duplicate authorization boilerplate in route bodies.

Auth providers are project-wide declarations discovered by the loader:

```marreta
auth jwt customer_auth {
    issuer: env.MARRETA_AUTH_CUSTOMER_ISSUER
    audience: env.MARRETA_AUTH_CUSTOMER_AUDIENCE
}

auth api_key internal_auth {
    header: "x-api-key"
    secret_hash: env.MARRETA_AUTH_INTERNAL_API_KEY_HASH
    principal: "internal-service"
}
```

Routes opt in with `require auth <provider>` and authorize with one or more
`allow` predicates:

```marreta
route GET "/orders/:user_id"
    require auth customer_auth
    allow auth.user.id == params.user_id or "admin" in auth.user.roles

    reply 200, db.orders.where(user_id == params.user_id).fetch()
```

Protected routes receive `auth` automatically. Public routes cannot access
`auth`. Failed authentication returns `401 { error: "unauthorized" }`; failed
authorization returns `403 { error: "forbidden" }`.

Supported auth providers in v0.13:

| Provider | Purpose |
|---|---|
| `auth jwt` | Bearer JWT validation with OIDC discovery, explicit JWKS, fixed public key PEM/file, or HMAC secret |
| `auth api_key` | Header API key validation with `secret_hash` or direct `secret` |

For the full contract, see `docs/spec/024_AUTHENTICATION_AUTHORIZATION.md`.

### 3.10 Environment Variables (`env`)

The `env` object is automatically injected at server startup. It contains all OS environment variables (and any variables from the project `marreta.env` file):

```marreta
route POST "/charge"
    key = env.STRIPE_KEY
    require key else fail 500, "Stripe key not configured"
    reply 200, { charged: true }
```

`env` is a read-only `Map` available in every route scope. Values are always `String`.

### 3.10 Responses

```marreta
# JSON response (default) — literal status
reply 200, data
reply 201, new_record

# Dynamic status — any expression that resolves to Integer
status = match role
    "admin" -> 200
    fallback -> 403
reply status, { data: result }

# HTML response
reply html 200, "<h1>Error</h1>"

# Plain text response
reply text 200, "Webhook received"

# Redirect
reply 302, null, { Location: "https://example.com/login" }

# Error response — body is any expression
fail 400, "bad request"
fail 404, { error: "not found", id: params.id }
fail 422, validation_errors
fail 500, build_error("unexpected failure")

# fail as a Never expression in match arms
label = match params.role
    "admin"  -> "Administrator"
    "user"   -> "Regular user"
    fallback -> fail 403, { error: "forbidden" }
```

`reply` and `fail` immediately terminate the route execution. `fail` can appear as a statement or as an expression (Never type) in any position where a value is expected — the route short-circuits at that point regardless.

**Content-Type by modifier:**

| Syntax | Content-Type |
|---|---|
| `reply CODE, data` | `application/json` (default) |
| `reply html CODE, "..."` | `text/html` |
| `reply text CODE, "..."` | `text/plain` |

The optional third argument `{ Key: "value" }` adds extra response headers (e.g. `Location` for redirects).

### 3.11 CORS

CORS is controlled via `marreta.env`:

```env
MARRETA_CORS=true         # Enable CORS middleware (default: true)
MARRETA_CORS_ORIGIN=*     # Allowed origins (default: *)
```

When enabled, the engine applies `tower-http`'s `CorsLayer` to all routes, handling preflight `OPTIONS` requests automatically.

---

## 3.12 Schema Declarations

`schema` is a top-level declaration that defines the expected shape and types of a JSON payload or response. Fields use lowercase type keywords. Optional fields are suffixed with `?`.

```marreta
schema user_payload
    name: string
    age: integer
    email?: string     # Optional — `?` on the field name (Ruby-style)
    is_active: boolean
```

**Primitive field types:** `string`, `integer`, `float`, `boolean`, `list`, `map`

### Schema Composition — Nested Schemas (v0.4.0)

A field can reference another declared schema by name. The validator recursively validates the nested object and reports errors with the full dotted path:

```marreta
schema address
    street: string
    city: string
    zipcode: string

schema order_payload
    billing: address       # nested schema reference
    coupon?: string
```

A payload missing `billing.city` returns:
```json
{ "error": "field 'billing.city' is required" }
```

### Schema Composition — Typed Lists (v0.4.0)

Use `list of <Type>` to validate arrays of objects or primitives. The syntax follows the keyword-driven style of the language — no angle brackets or generics notation.

```marreta
schema order_item
    product_id: integer
    quantity: integer

schema order_payload
    items: list of order_item    # array of objects
    tags?: list of string        # array of primitives (optional)
    billing: address
```

Errors on array elements include the index: `"field 'items[1].quantity' is required"`.

> **Circular references** (schema A references schema B which references schema A) are detected at startup. The server does not start and emits a descriptive error: `"circular schema reference detected: address → order_payload → address"`.

### Type Binding (`as`)

The `as` keyword binds a schema to a variable. It is used consistently in three contexts:

```marreta
# 1. Payload binding — validate HTTP request body
route POST "/orders" take payload as order_payload
    reply 201, { ok: true }

# 2. Response binding — serialize HTTP response (v0.3.3)
route POST "/orders" take payload as order_payload
    reply 201 as order_created, { order_created: true, ... }

# 3. Task contract — validate task arguments (v0.4.0)
task get_coupon_rate(order as order_payload)
    match order.coupon
        "SAVE10" -> 0.10
        fallback  -> 0.0
```

If the request body violates the schema (missing required field or wrong type), the engine returns `HTTP 422 Unprocessable Entity`:

```json
{ "error": "field 'billing.city' is required" }
```

A task contract violation (wrong argument shape) returns `HTTP 500 TypeError` — it is a programmer error, not a client error.

### Response Schema (v0.3.3)

`reply CODE as schema_name, expr` serializes the response through the schema: declared fields are included, undeclared fields are stripped, missing optional fields are omitted, missing required fields become `null`.

```marreta
export schema order_created
    order_created: boolean
    item_count: integer
    city: string

route POST "/orders" take payload as order_payload
    reply 201 as order_created, { order_created: true, item_count: 2, city: "São Paulo", extra_field: "stripped" }
# → { "order_created": true, "item_count": 2, "city": "São Paulo" }
```

### Auto-Documentation (Swagger UI)

When `MARRETA_DOCS_ENABLED=true` (default), the engine exposes:

- **`GET /openapi.json`** — OpenAPI 3.0 spec generated from routes and schemas
- **`GET /docs`** (configurable via `MARRETA_DOCS_PATH`) — Swagger UI (requires internet for CDN assets)

Nested schemas are emitted as `$ref` in `components/schemas`. Typed lists are emitted as `{ "type": "array", "items": ... }`. Swagger UI renders nested objects as collapsible trees.

```env
MARRETA_DOCS_ENABLED=true
MARRETA_DOCS_PATH=/docs
```

---

## 4. DB Module (Relational Database)

`db` is a global reserved word for **relational databases** (PostgreSQL). The engine connects automatically using the credentials in `marreta.env` — the developer never references the driver in `.marreta` code.

The namespace separation is intentional: `db.*` carries relational semantics (tables, rows, transactions, SQL), `doc.*` carries document semantics (collections, flexible schema), and `cache.*` carries key-value semantics. Mixing them into one namespace would mask the impedance mismatch and lead to surprising behavior.

### 4.1 Two Styles — Direct and Pipeline

`db` supports two composable styles. Both are always available; choose based on complexity.

**Direct operations** — for simple, single-step cases:

```marreta
user  = db.users.find(id)
user  = db.users.save(payload)
all   = db.users.find_all()
db.users.update(id, { name: "New Name" })
db.users.delete(id)
```

**Pipeline composition** — for queries with filters, joins, ordering, and pagination:

```marreta
rows = db.orders
    >> where(status: "active", total_gt: 1000)
    >> join("users", on: "user_id")
    >> order_by("created_at desc")
    >> limit(20)
    >> fetch
```

`db.TABLE` returns a lazy `QueryBuilder` — no database call is made until a **terminal operation** is reached. Pipeline steps accumulate clauses; the terminal executes the SQL and resolves the value.

### 4.2 Two-Context Pipeline Model

The pipeline operator `>>` works in two distinct contexts when used with `db.*`. Understanding this boundary is the key mental model:

```
db.TABLE                        ← opens query context (QueryBuilder)
    >> where(...)               ← SQL context: accumulates WHERE clause
    >> join("table", on: "fk")  ← SQL context: accumulates JOIN clause
    >> order_by("col desc")     ← SQL context: accumulates ORDER BY clause
    >> fetch                    ← terminal: executes SQL, closes query context
    >> map row                  ← language context: transforms data in memory
        row.label = "..."
        keep row
    >> calculate_tax             ← language context: calls a task
```

**Before the terminal** — every `>>` step is in **query context**: it adds a SQL clause to the `QueryBuilder` and returns the updated `QueryBuilder`. No I/O happens.

**The terminal** (`fetch`, `fetch_one`, `count`, `exists`, `update`, `delete`) is the boundary. It executes the accumulated SQL query, resolves the result to a plain `Value` (`List[Map]`, `Integer`, `Boolean`, etc.), and **returns that value to the language pipeline**.

**After the terminal** — every `>>` step is in **language context**: `map`, `keep`, tasks, `*>>`, and all other pipeline operations work exactly as they do everywhere else in the language. There is no special DB mode — it is just a list being processed.

This means the full expressive power of the language is available after any terminal:

```marreta
result = db.orders
    >> where(status: "active")
    >> join("users", on: "user_id")
    >> fetch
    >> map order
        order.display = "#{order.users.name} — R$ #{order.total}"
        order.net = order.total * 0.9
        keep order
    >> apply_discount_rules
```

Trying to use `map`, `keep`, or a task **before** a terminal is a startup error — those operations expect a `Value::List`, not a `QueryBuilder`.

### 4.3 Pipeline Steps (Query Context)

| Step | Description |
|---|---|
| `>> where(filters)` | Add WHERE clauses. Multiple `where` steps are AND-joined. |
| `>> join("table", on: "fk_col")` | INNER JOIN. |
| `>> left_join("table", on: "fk_col")` | LEFT JOIN. |
| `>> select(cols...)` | Columns to include in SELECT. Default is `*`. |
| `>> order_by("col asc\|desc")` | ORDER BY clause. |
| `>> limit(n)` | LIMIT clause. |
| `>> offset(n)` | OFFSET clause. |

`>> select(...)` accepts column names and computed aliases:

```marreta
rows = db.orders
    >> where(status: "active")
    >> select(id, status, net: "total * 0.9")   # → SELECT id, status, (total * 0.9) AS net
    >> fetch
```

### 4.4 Terminal Operations

Terminals close the query context, execute the SQL, and return a plain value back to the language pipeline.

| Terminal | Returns | SQL |
|---|---|---|
| `>> fetch` | `List[Map]` | SELECT matching rows |
| `>> fetch_one` | `Map \| null` | SELECT ... LIMIT 1 |
| `>> count` | `Integer` | SELECT COUNT(*) |
| `>> exists` | `Boolean` | SELECT EXISTS(...) |
| `>> update({ fields })` | nothing | UPDATE matching rows |
| `>> delete` | nothing | DELETE matching rows |

```marreta
# Read
rows  = db.orders >> where(status: "active") >> fetch
first = db.orders >> where(user_id: id) >> order_by("created_at desc") >> fetch_one
total = db.orders >> where(status: "active") >> count
found = db.users  >> where(email: email) >> exists

# Bulk mutation
db.orders >> where(status: "draft", created_at < cutoff) >> delete
db.orders >> where(status: "pending") >> update({ status: "processing" })
```

### 4.5 Filter Expressions

`where(...)` accepts **boolean expressions** directly — the same operators already used in the rest of the language. Key-value pairs (`key: value`) remain shorthand for equality.

```marreta
# Equality shorthand — key: value
rows = db.orders >> where(status: "active") >> fetch

# Comparison expressions — reads like the language, not like a DSL
rows = db.orders >> where(total > 1000, status: "active") >> fetch

# LIKE and IN as dedicated pipeline steps
users = db.users >> like("name", "João%") >> fetch
users = db.users >> in("status", ["active", "pending"]) >> fetch

# Mixing steps freely — all clauses AND-join
rows = db.orders
    >> where(status: "active", total > 1000)
    >> like("note", "%urgent%")
    >> order_by("created_at desc")
    >> limit(20)
    >> fetch
```

All `where`, `like`, and `in` steps are AND-joined. Multiple steps of the same kind chain correctly. For OR conditions, use `native_query`.

**Filter steps:**

| Step | SQL |
|---|---|
| `>> where(key: value)` | `key = value` |
| `>> where(col > val)` | `col > val` |
| `>> where(col >= val)` | `col >= val` |
| `>> where(col < val)` | `col < val` |
| `>> where(col <= val)` | `col <= val` |
| `>> where(col != val)` | `col != val` |
| `>> like("col", "pattern")` | `col LIKE 'pattern'` |
| `>> in("col", [v1, v2])` | `col IN (v1, v2)` |

### 4.6 Joins

`>> join(...)` defaults to INNER JOIN; `>> left_join(...)` is a LEFT JOIN. The `on:` value is the **foreign key column on the left table** — the engine infers `left_table.fk = right_table.id`.

For multi-level or complex joins, use `native_query`.

**Result shape:** columns are prefixed with the table name to prevent silent collision:

```json
{ "orders.id": 42, "orders.status": "active", "users.id": 7, "users.name": "Ana" }
```

For cleaner key names, use `native_query` with SQL aliases.

### 4.7 Native Query (escape hatch)

When the pipeline API is not enough, `db.native_query` sends raw SQL and returns a list of maps.

```marreta
results = db.native_query("SELECT * FROM orders WHERE total > 1000 ORDER BY created_at DESC LIMIT 10")
```

Interpolated expressions are bound as prepared statement parameters — never string-concatenated. Any Marreta Lang expression is valid inside `#{}`:

```marreta
# Variable
results = db.native_query("SELECT * FROM users WHERE email = #{email}")
# → SELECT ... WHERE email = $1   (params: [email])

# Method call or arithmetic
results = db.native_query("SELECT * FROM orders WHERE total > #{min_total * 1.1} AND active = #{true}")
# → SELECT ... WHERE total > $1 AND active = $2   (params: [min_total * 1.1, true])

# Path parameter
rows = db.native_query("SELECT * FROM items WHERE name = #{params.name}")
```

**Result type mapping:**

| PostgreSQL type | `Value` |
|---|---|
| `int2 / int4 / int8` | `Value::Integer` |
| `float4 / float8 / numeric` | `Value::Float` |
| `text / varchar / char` | `Value::String` |
| `bool` | `Value::Boolean` |
| `NULL` | `Value::Null` |

Each row is a `Value::Map` keyed by column name. Use SQL aliases to control key names in joins or computed columns: `SELECT o.id AS order_id, u.name AS user_name ...`

### 4.8 Transactions

`transaction` groups DB operations into an atomic unit. Any failure inside the block rolls back the entire block automatically.

```marreta
transaction
    db.accounts.update(sender_id,   { balance: sender.balance   - amount })
    db.accounts.update(receiver_id, { balance: receiver.balance + amount })
    db.transfers.save({ from: sender_id, to: receiver_id, amount: amount })
```

Pipeline queries can appear inside `transaction`:

```marreta
transaction
    db.orders >> where(id: order_id) >> update({ status: "paid" })
    db.invoices.save({ order_id: order_id, amount: total })
```

A `transaction` block can appear anywhere inside a route or task body. It cannot be nested.

### 4.9 Conventions

- `db.TABLE` — table name is the identifier after `db.`, used as-is (snake_case, singular or plural).
- Filter suffixes are stripped before building SQL: `total_gt` → column `total`, operator `>`.
- `save` returns the full persisted record via `RETURNING *` — includes `id`, `created_at`, etc.
- `find` (direct) and `fetch_one` (pipeline) return the record or `null` — always guard the null case.
- `update` and `delete` (both direct and pipeline) are fire-and-forget — they do not return a value.
- A `QueryBuilder` that is never terminated is a **startup error** — detected during the load phase.

---

---

## 5. Doc Module (Non-Relational / Document)

`doc` is a global reserved word for **document databases** (MongoDB). It carries document semantics: collections instead of tables, flexible schema per document, no joins at the language level, no transactions (requires replica set — out of scope).

The namespace separation is intentional: `db.*` carries relational semantics (tables, rows, SQL), `doc.*` carries document semantics (collections, BSON). Merging them would mask the impedance mismatch. See `docs/spec/docs/spec/010_DOC_MODULE.md` for the full architectural rationale and implementation details.

> **v0.7.0** — Layer 1 (CRUD) + Layer 2 (Query Pipeline).
> **v0.7.1** — Env var separation (`MARRETA_DB_*` / `MARRETA_DOC_*` independent); `DocPoolConfig`; dual engine (`db.*` + `doc.*` coexist).
> **v0.7.2** — Layer 3 (Aggregation: `group_by`, `sum`, `avg`, `min`, `max`, post-group `order`/`limit`).
> **v0.7.3** — Layer 4 (Power Pipeline: `doc.pipeline` escape hatch).

### 5.1 Syntax Divergence from `db.*`

`doc.*` intentionally diverges from `db.*` in three places:

1. **String field names in `where`:** `>> where("status" == "pending")` — MongoDB field names can contain dots (`"address.city"`) and other characters not valid as Marreta identifiers.
2. **`order` instead of `order_by`:** `>> order("created_at", "desc")` — function-call form with direction as a separate argument, avoiding `asc`/`desc` as lexer tokens.
3. **`fetch_all` instead of `fetch`:** explicit cardinality — `fetch_all` (all matching) vs `fetch_one` (single document or null).

### 5.2 CRUD Operations (Layer 1 — v0.7.0)

Atomic single-document operations. No pipeline required.

```marreta
# CREATE — returns persisted document with _id as String
order = doc.save("orders", { user_id: 42, total: 199.90, status: "pending" })
order = doc.save("orders", payload)

# READ by _id — returns null if not found
order = doc.find("orders", params.id)

# READ all documents in collection (no filter — use pipeline for filtered reads)
all_orders = doc.find_all("orders")

# PARTIAL UPDATE — $set semantics; returns updated document (find_one_and_update)
updated = doc.update("orders", params.id, { status: "shipped" })

# DELETE by _id — returns true if deleted, false if not found
deleted = doc.delete("orders", params.id)
```

**Design decisions:**
- `doc.save` always generates `_id` (MongoDB ObjectId → String in Marreta). Developer never provides `_id` to `save`.
- `doc.update` is always `$set` — partial merge. Full document replacement is not exposed.
- `doc.find` returns `null` for missing documents, consistent with `db.find`.
- `doc.find_all` has no filter argument. Filtered reads use the pipeline: `doc.query(col) >> where(...) >> fetch_all`.
- `doc.update` uses `find_one_and_update` with `ReturnDocument::After` — one atomic round-trip.

### 5.3 Query Pipeline (Layer 2 — v0.7.0)

Multi-condition queries built through `>>` steps. All steps are function calls.

```marreta
# Equality filter
results = doc.query("orders")
    >> where("status" == "pending")
    >> fetch_all

# Multiple where steps (AND semantics)
results = doc.query("orders")
    >> where("status" == "paid")
    >> where("total" > 100)
    >> fetch_all

# Nested field access via dot-notation string
results = doc.query("orders")
    >> where("address.city" == "São Paulo")
    >> fetch_all

# Ordering and pagination
results = doc.query("orders")
    >> where("status" == "pending")
    >> order("created_at", "desc")
    >> limit(20)
    >> offset(0)
    >> fetch_all

# Field projection
results = doc.query("orders")
    >> where("user_id" == params.user_id)
    >> pick(["_id", "total", "status"])
    >> fetch_all

# Terminals: fetch_one, count, exists
order = doc.query("orders") >> where("ref" == params.ref) >> fetch_one
total = doc.query("orders") >> where("status" == "pending") >> count
has   = doc.query("orders") >> where("user_id" == id) >> exists

# Terminals: upsert, update, delete
doc.query("orders") >> where("ref" == payload.ref) >> upsert({ status: "pending", total: payload.total })
doc.query("orders") >> where("status" == "pending") >> update({ status: "expired" })
doc.query("orders") >> where("user_id" == deleted_user_id) >> delete
```

| Marreta step | MongoDB equivalent |
|---|---|
| `>> where("field" == val)` | `{ field: { $eq: val } }` |
| `>> where("field" != val)` | `{ field: { $ne: val } }` |
| `>> where("field" > val)` | `{ field: { $gt: val } }` |
| `>> where("field" >= val)` | `{ field: { $gte: val } }` |
| `>> where("field" < val)` | `{ field: { $lt: val } }` |
| `>> where("field" <= val)` | `{ field: { $lte: val } }` |
| `>> where("field" in [...])` | `{ field: { $in: [...] } }` |
| `>> like("field", "pattern")` | `{ field: { $regex: "pattern" } }` |
| `>> order("field", "asc")` | `sort: { field: 1 }` |
| `>> order("field", "desc")` | `sort: { field: -1 }` |
| `>> limit(N)` | `limit: N` |
| `>> offset(N)` | `skip: N` |
| `>> pick(["f1", "f2"])` | `projection: { f1: 1, f2: 1 }` |
| `>> fetch_all` | `find(...).to_vec()` |
| `>> fetch_one` | `find_one(...)` |
| `>> count` | `count_documents(...)` |
| `>> exists` | `count_documents(...) > 0` |
| `>> upsert({ ... })` | `update_many(..., { $set: {...} }, upsert: true)` |
| `>> update({ ... })` | `update_many(..., { $set: {...} })` |
| `>> delete` | `delete_many(...)` |

**`_id` smart-cast:** when `>> where("_id" == val)` is used, the driver auto-detects if the string is a valid ObjectId hex (24-char hex string) and casts accordingly. Non-hex strings are passed as plain String `_id` values — no error is raised. This supports databases with UUID, slug, or numeric `_id` values.

### 5.4 Aggregation (Layer 3 — v0.7.1)

```marreta
# Group by field with accumulators
revenue = doc.query("orders")
    >> where("status" == "paid")
    >> group_by("user_id")
    >> sum("total", as: "revenue")
    >> count(as: "order_count")
    >> fetch_all

# Global aggregation (no group_by)
totals = doc.query("orders")
    >> where("status" == "paid")
    >> sum("total", as: "revenue")
    >> avg("total", as: "avg_order")
    >> fetch_one

# Post-aggregation ordering
top_users = doc.query("orders")
    >> where("status" == "paid")
    >> group_by("user_id")
    >> sum("total", as: "revenue")
    >> order("revenue", "desc")
    >> limit(10)
    >> fetch_all
```

Write terminals (`update`, `upsert`, `delete`) after aggregation steps produce an interpreter error.

### 5.5 Power Pipeline (Layer 4 — v0.7.2)

For aggregations exceeding Layers 1–3. Developer writes MQL pipeline stages as Marreta maps. Keys are plain identifiers (no `$`); field-reference values use `$field` where MQL requires it.

```marreta
results = doc.pipeline("orders", [
    { match:  { status: "paid" } },
    { lookup: { from: "users", local: "user_id", foreign: "_id", as: "user" } },
    { unwind: "user" },
    { group:  { by: "user.country", total: { sum: "$total" } } },
    { sort:   { total: -1 } },
    { limit:  10 }
])
```

### 5.6 Error Handling

All MongoDB driver errors are translated at the module boundary — no `mongodb::error::Error` propagates. Doc errors use `error.code = "db_error"` (same as relational) with `error.op = "doc.{collection}.{operation}"`. All `doc.*` operations are `rescue`-compatible.

### 5.7 Conventions

- `doc.save(collection, map)` — collection name is passed as a string argument.
- No `transaction` block — MongoDB transactions require replica sets (out of scope).
- No `native_query` equivalent — `doc.pipeline` (Layer 4) is the structured escape hatch.
- `save` returns the persisted document including the generated `_id` field (as String).
- Dual-engine support (v0.7.1): `MARRETA_DB_*` governs `db.*`; `MARRETA_DOC_*` governs `doc.*`. Both engines can coexist in the same deployment — configure both structured env sets to enable both namespaces simultaneously.

---

## 6. Error Handling

Marreta Lang has a deliberate, three-part error system designed to keep error handling expressive, readable, and free of infrastructure noise. The three mechanisms are complementary and cover the full error-handling surface of a REST API language:

- **`raise`** — intentionally signal an error from inside a task or operation, without coupling to an HTTP response code
- **`rescue`** — catch and recover from any error (runtime, `raise`, or infrastructure) in a pipeline or expression
- **Marreta Error Identity** — all errors, caught or not, speak Marreta Lang — no Rust internals, no driver error codes, no stack traces ever reach the developer

### 6.1 The Design Principle

The critical distinction in Marreta Lang error handling is between **HTTP errors** and **domain errors**:

- `fail CODE, MSG` → *"I know what HTTP response to return"* — immediate route termination, carries HTTP semantics
- `raise MSG` → *"Something went wrong, caller decides what to do"* — propagates up, no HTTP code, no route termination

This separation means tasks can signal errors without coupling to the HTTP layer. The route (or pipeline) decides what the HTTP response will be.

### 6.2 Raising Errors (`raise`)

`raise` signals an intentional error from inside a task or pipeline. It propagates up through the call stack until caught by a `rescue` or until it reaches the route level.

```marreta
task charge_card(card, amount)
    raise "Invalid card" if not card.valid
    raise "Amount must be positive" if amount <= 0
    payment_gateway.charge(card.token, amount)
```

**Syntax:**

```marreta
raise "message"                 # unconditional raise
raise "message" if condition    # conditional raise (modifier style)
```

**Semantics:**
- `raise` does not carry an HTTP status code — that is the caller's decision
- A `raise` that is not caught by any `rescue` and reaches the route level becomes an automatic HTTP 500 with the raise message
- Works inside tasks, pipeline steps, and task bodies — anywhere in the language
- Compatible with `require ... else raise MSG` for domain validation inside tasks — note: `require ... else rescue` is **not valid**; `rescue` is a pipeline/expression concept, not a `require` branch

```marreta
task validate_order(order)
    require order.total > 0 else raise "Total must be positive"
    require order.items else raise "Order has no items"
    order
```

### 6.3 Recovering from Errors (`rescue`)

`rescue` intercepts errors from preceding pipeline steps or expressions. It implements a Railway Oriented Programming model: any error at any preceding step diverts to the `rescue` handler, bypassing all intermediate steps.

#### 6.3.1 `rescue` as a terminal pipeline step

The most common form — catches any error from any step above it in the pipeline:

```marreta
result = payload.items
    >> map item
        item.total = calculate_tax(item)
        keep item
    >> db.orders.save
    >> queue.push("invoices")
    >> rescue fail 503, "Could not process orders"
```

If `db.orders.save` fails, `queue.push` fails, or `calculate_tax` raises — execution goes directly to `rescue`, bypassing all subsequent steps.

#### 6.3.2 `rescue` block (multi-line recovery)

When recovery requires more than one expression, `rescue` opens an indented block — consistent with `map` and `task` block syntax:

```marreta
result = payload.items
    >> db.orders.save
    >> rescue
        cache.set("pending_orders", payload)
        notify_ops("Order pipeline failed")
        fail 503, "Order queued for retry"
```

The block can call tasks, access the `error` variable, write to cache, push to queues, or reply with an alternative response.

#### 6.3.3 `rescue` as an expression modifier

For single expressions outside a pipeline:

```marreta
# Task call with rescue
result = charge_card(payload.card, payload.total) rescue fail 402, error.message

# DB operation with rescue
user = db.users.save(payload) rescue fail 409, "User already exists"

# Calling a recovery task
result = risky_operation(data) rescue handle_failure(payload)
```

Reads as: *"do this, and if it fails, do that"*.

**The handler is any valid expression.** This includes literals, maps, task calls, `fail`, `reply`, and blocks. If the handler evaluates to a value (rather than terminating via `fail` or `reply`), that value substitutes the result of the failed expression and execution continues normally:

```marreta
total  = calculate(payload) rescue 0                      # fallback to zero
items  = fetch_items(id)    rescue []                     # fallback to empty list
meta   = get_meta(q)        rescue null                   # optional — caller guards
config = load_config(path)  rescue { version: "default" } # fallback map
```

This makes `rescue` the idiomatic pattern for **optional operations** — equivalent to `or` for null, but for errors.

#### 6.3.4 The `error` variable in `rescue` scope

Inside any `rescue` block or expression, the `error` variable is automatically available as a `Map`:

| Field | Type | Description |
|---|---|---|
| `error.message` | `string` | Human-readable description of what failed |
| `error.op` | `string` | The Marreta operation that failed (`db.users.save`, `queue.push`, etc.) |
| `error.code` | `string` | Semantic Marreta error code — see table below |

```marreta
result = db.orders.save(order)
    >> rescue
        log_error("DB save failed: #{error.message}")
        fail 503, error.message
```

**Semantic codes (`error.code`):**

| Code | Meaning |
|---|---|
| `raise_error` | Developer used `raise` keyword intentionally |
| `db_error` | Database operation failure |
| `type_error` | Type mismatch or wrong argument type |
| `reference_error` | Undefined variable, task, property, or non-callable |
| `arity_error` | Wrong number of arguments to a task |
| `arithmetic_error` | Arithmetic fault (e.g. division by zero) |
| `io_error` | File system or I/O failure |
| `config_error` | Startup-time conflict (routes, exports, schemas) |
| `infrastructure_error` | Queue, cache, or HTTP client failure |
| `runtime_error` | General interpreter or engine failure |

`error.code` is never a Postgres error code, an HTTP status code, or a Rust type name.

#### 6.3.5 Per-step granular recovery

When different steps require different error handling, isolate the risky step in a task with its own `rescue`:

```marreta
task push_invoice_safe(order)
    queue.push("invoices") rescue null   # non-critical — fail silently
    order                                # pass order through regardless

result = payload.items
    >> db.orders.save                    # critical — rescued at pipeline level
    >> push_invoice_safe                 # non-critical — handled inside task
    >> rescue fail 503, "Could not save orders"
```

### 6.4 Error Propagation Model

```
raise / runtime error
      ↓
  rescue in current pipeline?
      ↓ yes → execute rescue body
      ↓ no  → propagate up to caller
               ↓
           rescue at caller level?
               ↓ yes → execute rescue body
               ↓ no  → propagate to route level
                            ↓
                    HTTP 500 with error.message
```

`fail CODE, MSG` bypasses this chain entirely — it terminates the request immediately regardless of any `rescue` handlers.

**`rescue` is not a safe zone.** A `fail` or `reply` issued inside a `rescue` block terminates the request immediately, exactly as it would anywhere else in a route. `rescue` does not intercept `fail` or `reply` — it only intercepts `raise` and runtime errors. There is no outer boundary that catches a `fail` inside a `rescue`.

### 6.5 Marreta Error Identity

All errors in Marreta Lang — caught or uncaught, from infrastructure or the interpreter — present themselves in Marreta Lang terms. The Rust engine is invisible.

**What the developer never sees:**
- Rust type names (`sqlx::Error`, `tokio::Error`, `std::io::Error`)
- Rust file paths or line numbers (`src/interpreter.rs:247`)
- Raw database error codes (`23505`, `ECONNREFUSED`)
- Rust panic output or stack traces
- Driver-specific error messages

**What the developer always sees:**
```
[marreta] db.users.save failed — duplicate value on field 'email'
  → routes/users.marreta:12
```
```
[marreta] TypeError at routes/orders.marreta:34
  expected map, got null — did you guard with require?
```
```
[marreta] DB unavailable — could not connect to postgres
  → check MARRETA_DB_HOST / MARRETA_DB_PORT / MARRETA_DB_NAME in marreta.env
```

Every error references the `.marreta` file and line, the Marreta operation that failed, and a human-readable description. Never a Rust artifact.

### 6.6 Conventions

- Use `require / reject else fail CODE, MSG` for HTTP guard clauses at the top of routes — the idiomatic way to validate input.
- Use `raise MSG` inside tasks for domain validation — the route decides the HTTP code.
- Use `rescue` at the pipeline level for infrastructure failures (DB, queue, cache) — not for every possible error.
- Do not use `rescue` to swallow errors silently without a visible side effect — always `fail`, `reply`, log, or perform a fallback action.
- `fail` without a preceding `require`/`rescue` is always intentional — it is a deliberate response decision, not error handling.

> **v0.6.0** — `raise`, `rescue`, and Marreta Error Identity. See `docs/spec/docs/spec/007_ERROR_HANDLING.md`.

---

## 7. Queue Module (Messaging)

> **v0.8** — implemented. See `docs/spec/013_QUEUE.md` for full design and `examples/functional_tests/routes/queue.marreta` for working examples.

`queue` and `on` are global reserved words. The provider is configured via
environment variables — application code is broker-agnostic.

```
MARRETA_QUEUE_PROVIDER=rabbitmq
MARRETA_QUEUE_HOST=queue.internal
MARRETA_QUEUE_PORT=5672
MARRETA_QUEUE_USER=app_user
MARRETA_QUEUE_PASSWORD=secret
```

### 7.1 Consumers (`on`)

`on` declares a background handler that runs continuously, consuming messages
from a queue or topic. It is not an HTTP route.

```marreta
# Point-to-point — one consumer processes each message
on queue "orders.processing" take message
    order_id = message.order_id

# Point-to-point with schema validation
on queue "orders.processing" take message as order_payload
    # message validated on arrival; mismatch → automatic nack (no requeue)

# Pub/sub — all subscribers to the exact topic receive each message
on topic "payments.approved" take message

# Pub/sub with schema
on topic "orders.created" take message as order_event
```

Topics are exact strings. Wildcards (`*`, `#`) are rejected; consume multiple
topics by declaring multiple `on topic` handlers.

#### Ack / Nack semantics

| Outcome | Behavior |
|---|---|
| Handler completes without error | Implicit ack |
| `nack` statement | Nack, no requeue |
| `nack requeue` statement | Nack, requeue message |
| Schema validation fails on delivery | Nack, no requeue |
| Runtime error (`fail`, `raise`, unhandled) | Nack, no requeue |

```marreta
on queue "orders.processing" take message as order_payload
    require message.order_id else nack           # discard, malformed
    require message.amount > 0 else nack requeue # retry, transient
    # implicit ack on clean exit
```

### 7.2 Producers

```marreta
# Point-to-point
queue.push "orders.processing", { order_id: 123, amount: 49.90 }

# Point-to-point with schema — strips fields not in schema before sending
queue.push "orders.processing" as order_payload, { order_id: 123, amount: 49.90 }

# Pub/sub
topic.publish "payment.approved", { order_id: 123 }

# Pub/sub with schema — strips fields not in schema before sending
topic.publish "payment.approved" as payment_event, { order_id: 123 }
```

### 7.3 Pipeline support

`queue.push` and `topic.publish` **return the published value** (after schema filtering, if any), making them pipeline-friendly:

```marreta
# Push result of a DB query to a queue in a single pipeline
db.orders.find(id) >> queue.push "invoices" >> reply 200

# Publish to a topic inside a pipeline
payload >> topic.publish "order.created" >> reply 201
```

When used in a pipeline, the payload is received from the left side — no second argument is needed. With an explicit payload (the `queue.push "q", data` form), the second argument takes precedence.

### 7.4 Schema contract (optional)

Schema binding on `on`/`queue.push`/`topic.publish` follows the same rules as
routes: without `as`, any map is accepted/sent; with `as`, the schema is enforced
on arrival (consumer) or applied as a filter before sending (producer).

---

## 8. Cache Module

`cache` is a global reserved word for **key-value stores**. Initial provider target: **Redis**. The architecture mirrors `db.*` / `doc.*` / `queue.*` — the provider is declared via environment variables, keeping application code agnostic. The semantics are intentionally minimal: get, set, delete, TTL, counters, bulk ops. No filter operators, no pagination, no hashes/lists/sorted sets, no pub/sub — if you need those, use `db.*`, `doc.*`, or `queue.*`.

> **v0.9** — `cache.*` implemented. Redis driver via `redis 1.2`. See `docs/spec/014_CACHE.md` for the full design document.

### 8.1 Core Operations

```marreta
# SET — store a value; returns the value stored (pipeline-friendly)
cache.set("key", value)
cache.set("session:#{token}", user, ttl: 3600)

# SET if absent — atomic "store only if no key exists"
# Returns the value on success, null if the key already existed.
# Use for idempotency keys, distributed locks, webhook dedup.
stored = cache.set("idempotency:#{key}", result, ttl: 86400, only_if_absent: true)
require stored else fail 409, "duplicate request"

# GET — returns the value or null on miss / expiry
value = cache.get("key")

# GET with fallback — the `or` short-circuits on miss
user = cache.get("user:#{id}") or db.users.find(id) >> cache.set("user:#{id}", ttl: 300)

# DELETE — returns true if the key existed, false otherwise
cache.delete("key")

# EXISTS — returns boolean without fetching the value
alive = cache.exists("session:#{token}")

# TTL — seconds remaining; null if the key has no TTL or does not exist
remaining = cache.ttl("session:#{token}")

# EXPIRE — refresh TTL without re-writing the value (sliding sessions)
cache.expire("session:#{token}", ttl: 3600)

# INCREMENT / DECREMENT — atomic counter operations, return the new value
cache.incr("page_views:#{slug}")
cache.incr("rate_limit:#{ip}", by: 1, ttl: 60)
count = cache.decr("credits:#{user_id}")
```

### 8.2 Bulk Operations

```marreta
# GET many — returns a map keyed by the requested keys; misses are null
users = cache.get_many(["user:1", "user:2", "user:3"])

# SET many — shared TTL for all keys in the batch
cache.set_many({ "user:1": user1, "user:2": user2 }, ttl: 300)
```

### 8.3 Schema Contracts (optional)

`cache.set` / `cache.get` accept optional `as schema_name` binding, consistent with `db.*`, `doc.*`, and `queue.*`:

```marreta
# On write — strip fields not declared in the schema before serializing
cache.set("user:#{id}" as user_schema, payload, ttl: 300)

# On read — validate the cached payload against the schema
# Mismatch is treated as a cache miss (returns null, does not raise),
# so stale values written by a previous deploy are self-healing.
user = cache.get("user:#{id}") as user_schema
```

### 8.4 Conventions

- Keys are strings. String interpolation (`#{}`) is the standard way to build dynamic keys.
- Values can be any `Value` type — the engine serializes to JSON before writing, deserializes on read.
- `cache.get` returns `null` on miss, expiry, or (with `as`) schema mismatch — always guard with `or` or an explicit `require`.
- `cache.set` returns the value it stored, enabling pipeline flow: `db.users.find(id) >> cache.set("user:#{id}", ttl: 300)`.
- `cache.incr` / `cache.decr` are atomic at the provider level — safe for counters and rate limiting under concurrency.
- Connection failures, timeouts, and serialization errors **raise** like `db.*` / `queue.*`. For soft-fail behavior ("cache is a hint"), use `rescue null`: `cache.get("k") rescue null`.
- No scan, no pattern matching, no flush — those are anti-patterns at scale. Use `db.*` for queryable data.
- If `MARRETA_CACHE_PREFIX` is set, every key is transparently prefixed — enables multi-tenant / multi-env isolation on a shared cache cluster.

---

## 9. HTTP Client Module

> **v0.10** — `http_client.*` implemented. `reqwest` driver. See `docs/spec/015_HTTP_CLIENT.md` for the full design document.

`http_client` is a global reserved word for **outbound HTTP calls**. Routes and tasks use it to call external APIs — payment gateways, microservices, webhooks, third-party services. Every call returns a response envelope with three fields: `status`, `body`, and `headers`. HTTP 4xx/5xx responses are **not errors** — the developer guards with `require`.

### 9.1 Request Verbs

```marreta
response = http_client.get("https://api.example.com/users/#{id}")
response = http_client.post("https://api.example.com/orders", payload)
response = http_client.put("https://api.example.com/orders/#{id}", payload)
response = http_client.patch("https://api.example.com/orders/#{id}", { status: "shipped" })
response = http_client.delete("https://api.example.com/orders/#{id}")
```

### 9.2 Response Envelope

```marreta
response.status     # integer — 200, 201, 404, 500, etc.
response.body       # parsed JSON (map/list/string/integer) or raw string
response.headers    # map of lowercase header names → string values
```

### 9.3 Named Parameters

```marreta
# Custom headers
response = http_client.post("https://pay.stripe.com/charges", payload,
    headers: { "Authorization": "Bearer #{env.STRIPE_KEY}" })

# Query string — appended to the URL
response = http_client.get("https://api.example.com/search",
    query: { q: term, page: 1 })

# Per-request timeout in milliseconds
response = http_client.get("https://slow-api.com/report", timeout: 10000)
```

### 9.4 Pipeline Integration

```marreta
# Input: piped value becomes the request body (POST/PUT/PATCH)
payload >> http_client.post("https://orders.service/orders")

# Output: .body flows forward into db/cache/queue/tasks
response = http_client.get("https://catalog.service/products/#{id}")
require response.status == 200 else fail 502, "catalog unavailable"
response.body >> cache.set("product:#{id}", ttl: 300)
```

### 9.5 Error Guard

```marreta
response = http_client.get("https://users.service/users/#{id}")
require response.status == 200 else fail 502, "user service failed"
reply 200, response.body
```

### 9.6 Environment Variables

```
MARRETA_HTTP_TIMEOUT_MS=30000    # optional, default 30000 (30s)
```

A single global safety net. Per-request `timeout:` overrides for that specific call.

### 9.7 Error Semantics

| Failure | Behavior |
|---|---|
| HTTP 4xx/5xx response | Not an error — returns the response Map normally |
| Connection refused / DNS failure | Raises `HttpClientError` |
| Timeout exceeded | Raises `HttpClientError` |
| TLS/SSL failure | Raises `HttpClientError` |
| Invalid URL | Raises `HttpClientError` |
| Non-JSON body | `body` is the raw string (no error) |

All `http_client.*` errors use `error.code = "infrastructure_error"`. No `reqwest::` internals ever surface in error messages.

---

## 10. DB Migrations

> **v0.12 baseline + v0.13b direction** — implemented migrations plus current persistence-by-convention model. Full design rationale and phased details live in `docs/spec/018_DB_MIGRATIONS.md`, `docs/spec/018b_MIGRATION_HYGIENE.md`, and `docs/spec/025_PERSISTENCE_BY_CONVENTION_AND_QUERY_NAVIGATION.md`.

Marreta Lang allows persistent relational schemas to be modeled directly in
`schema` blocks. The developer defines domain entities in Marreta Lang, and the
CLI handles relational planning, SQL file generation, and migration state
tracking for Postgres.

### 10.1 Persistent schemas

A persistent schema uses `db:` to declare the backing table.

```marreta
schema Customer
    db: customers

    id: integer
    name: string
    email: string
    orders: list of ServiceOrder

schema ServiceOrder
    db: orders

    id: integer
    customer: Customer
    description: string
    total_amount: float
    status: string
```

Rules in the current implementation:

- `schema` remains the only schema construct; `db:` marks relational persistence
- schema names must use PascalCase
- schemas without `db:` remain contract-only
- `db:` is metadata only; it does not appear in API validation, serialization, or OpenAPI
- persistent schemas must declare `id: integer`
- `id` is the generated primary key for relational storage
- singular references from one persistent schema to another become `<field>_id` foreign-key columns
- `list of <persistent>` does not generate local storage; it is inverse/navigation metadata when inference is unambiguous
- persistent schema fields cannot reference non-persistent schemas singularly
- Postgres is the only migration driver in the current implementation
- schema-level `@...` annotations are not part of the current language direction

### 10.2 `marreta migrate` CLI

```bash
marreta migrate diff
marreta migrate generate
marreta migrate status
marreta migrate apply
marreta migrate rollback
```

When executed from the project root, these commands resolve `./app.marreta`
automatically. An explicit entrypoint path remains supported as an override.

Implemented behavior:

1. Load the project and collect persistent schemas
2. Build the desired relational model from `db:` schemas by convention
3. Reconstruct the current local schema from existing migration files
4. Compute the diff (`CREATE TABLE`, `ADD COLUMN`, `ADD FOREIGN KEY`)
5. `diff` prints the planned SQL only and works source-first
6. `generate` writes `up.sql` and `down.sql` files into the project's `migrations/` directory and works source-first
7. `apply` ensures `_marreta_migrations`, executes pending `up.sql` files in order, and records version + checksum
8. `status` compares local files with `_marreta_migrations` and classifies `applied`, `pending`, `changed`, and `missing_local`
9. `rollback` executes the last applied matching `down.sql` and removes the applied record

### 10.3 Migration files and state

Generated files live in a project-local `migrations/` directory:

```text
migrations/
  20260410_153001_create_users.up.sql
  20260410_153001_create_users.down.sql
```

Applied state is recorded in:

```text
_marreta_migrations
```

Each applied record stores:

- `version`
- `name`
- `checksum`
- `applied_at`

Checksums are mandatory. If the local file content differs from an already
applied record with the same version, `status` reports it as `changed` and
`apply` refuses to continue.

For the remaining design and future phases, see `docs/spec/018_DB_MIGRATIONS.md`.

---

## 12. Infrastructure Configuration

The binding between language abstractions (`db`, `queue`, `cache`) and concrete providers is done exclusively via configuration file. The `.marreta` code **never** references providers.

### 10.1 `marreta.env` File

```env
# HTTP Server
MARRETA_HOST=0.0.0.0
MARRETA_PORT=8080

# Relational database (db.*)
MARRETA_DB_PROVIDER=postgres
MARRETA_DB_HOST=localhost
MARRETA_DB_PORT=5432
MARRETA_DB_NAME=mydb
MARRETA_DB_USER=marreta
MARRETA_DB_PASSWORD=marreta-secret

# Connection pool tuning (all optional — sqlx defaults shown for postgres;
# reused for MongoDB where applicable)
MARRETA_DB_POOL_MAX_CONNECTIONS=10        # max simultaneous connections
MARRETA_DB_POOL_MIN_CONNECTIONS=0         # connections kept open when idle
MARRETA_DB_POOL_ACQUIRE_TIMEOUT_SECS=30   # seconds before "pool timed out" error
MARRETA_DB_POOL_IDLE_TIMEOUT_SECS=600     # seconds idle before connection is closed
MARRETA_DB_POOL_MAX_LIFETIME_SECS=1800    # seconds before connection is recycled
MARRETA_DB_POOL_TEST_BEFORE_ACQUIRE=true  # ping connection before use

# Document database (doc.*)
MARRETA_DOC_PROVIDER=mongodb
MARRETA_DOC_HOST=localhost
MARRETA_DOC_PORT=27017
MARRETA_DOC_NAME=mydocs
MARRETA_DOC_USER=marreta
MARRETA_DOC_PASSWORD=marreta-secret
MARRETA_DOC_AUTH_SOURCE=admin             # optional, advanced

# Messaging (queue.*)
MARRETA_QUEUE_PROVIDER=rabbitmq
MARRETA_QUEUE_HOST=localhost
MARRETA_QUEUE_PORT=5672
MARRETA_QUEUE_USER=guest
MARRETA_QUEUE_PASSWORD=guest

# Cache (cache.*)
MARRETA_CACHE_PROVIDER=redis                # required if cache.* is used
MARRETA_CACHE_HOST=localhost
MARRETA_CACHE_PORT=6379
MARRETA_CACHE_PASSWORD=redis-secret         # optional unless auth is enabled
MARRETA_CACHE_DB=0                          # optional, advanced
MARRETA_CACHE_PREFIX=myapp:prod:            # optional — auto-prefix for all keys
MARRETA_CACHE_DEFAULT_TTL=3600              # optional — safety net (seconds)
MARRETA_CACHE_POOL_SIZE=10                  # optional, default 10
MARRETA_CACHE_CONNECT_TIMEOUT_MS=2000       # optional, default 2000
MARRETA_CACHE_OPERATION_TIMEOUT_MS=1000     # optional, default 1000
MARRETA_CACHE_RECONNECT_MAX_RETRIES=10      # optional, default 10

# Authentication providers
MARRETA_AUTH_CUSTOMER_ISSUER=https://accounts.example.com
MARRETA_AUTH_CUSTOMER_AUDIENCE=customer-api
MARRETA_AUTH_PARTNER_PUBLIC_KEY_FILE=secrets/partner-public.pem
MARRETA_AUTH_INTERNAL_API_KEY_HASH=$argon2id$v=19$m=...
```

The developer provides structured provider configuration in `marreta.env` or via
process environment variables. The engine resolves the driver, initializes the
connection/client, and applies pool or reconnect tuning where the abstraction
supports it.

Process environment variables provided by CI/CD, containers, or the host
override values from `marreta.env`. This applies to infrastructure config and
auth config alike, including JWT public key file paths and API key hashes.

### 10.2 Planned Providers

| Abstraction | v0.5.0 | Future |
|---|---|---|
| `db` | PostgreSQL | MySQL, SQLite |
| `doc` | MongoDB | DynamoDB, Firestore |
| `queue` | RabbitMQ | Kafka, AWS SQS |
| `cache` | Redis | Memcached |

---

## 13. Reserved Words

Marreta Lang has a minimal set of reserved words:

### Core
`task`, `match`, `fallback`, `map`, `keep`, `require`, `reject`, `if`, `else`, `and`, `or`, `not`, `true`, `false`, `null`, `export`

### Error Handling (v0.6.0)
`raise`, `rescue`

### HTTP
`route`, `take`, `reply`, `fail`, `listen`

### Schema & AutoDoc
`schema`, `as` — type binding keyword (`take payload as schema`, `reply 201 as schema`, `task f(param as schema)`)

**Schema type keywords:** `string`, `integer`, `float`, `boolean`, `list`, `map`

**Schema composition (v0.4.0):** `of` — used in `list of <Type>` typed list declarations

### Infrastructure
`db`, `doc`, `feature`, `cache`, `queue`, `topic`, `transaction`, and the `env` accessor — all
reserved at the lexer (Spec 068), so a variable cannot shadow a documented namespace. They are free
in a name position (after `.`, a map key, a schema field name, a named-arg name, a `select` column)
and blocked only as a binder. (The full two-layer reserved/contextual writeup lands with Spec 069.)

### HTTP Verbs
`GET`, `POST`, `PUT`, `PATCH`, `DELETE`

**Total: ~30 reserved words** (compared to ~50+ in Java and ~35 in JavaScript).

---

## 14. Marreta Project Structure

```
my-project/
├── marreta.env          # Infrastructure configuration
├── app.marreta          # Entry point — project metadata and global declarations
├── routes/
│   ├── users.marreta
│   └── orders.marreta
├── schemas/
│   └── payloads.marreta
└── tasks/
    ├── validations.marreta
    └── calculations.marreta
```

`marreta serve` — when executed from the project root, the engine uses `./app.marreta` as the entrypoint and **automatically scans all `.marreta` files** in subdirectories. Explicit entrypoint paths remain supported as an override.

### 12.1 Multi-file Scoping (`export`)

By default, all symbols (variables, tasks, schemas) are **file-private** — no collisions across files.

File-private means:
- a route/consumer can use non-exported tasks, schemas, and top-level variables from the **same file**
- private symbols from one file are **not** visible in other files
- `export` controls only cross-file visibility, not same-file survival

The `export` keyword makes a symbol globally available to all other files:

```marreta
# schemas/payloads.marreta
export schema user_payload
    name: string
    age: integer
    email?: string

# tasks/calculations.marreta
export task calculate_discount(price, category)
    rate = match category
        "vip"     -> 0.15
        "premium" -> 0.10
        fallback  -> 0.0
    price * rate

# routes/users.marreta — uses exported symbols directly, no imports
route POST "/users" take payload as user_payload
    total = calculate_discount(payload.price, payload.category)
    reply 201, { total: total }
```

### 12.2 `app.marreta` — The Entrypoint

`app.marreta` is the only file where everything is **implicitly global** — no `export` needed:

```marreta
# app.marreta
project_name = "Payments API"
project_version = "1.0.0"
```

### 12.3 Two-pass Loading

The engine loads multi-file projects as module runtimes:
1. each file is parsed as its own module
2. top-level declarations execute in the module's private runtime
3. exported symbols are published into the shared global runtime
4. route/consumer execution resolves names as: local scope → defining file → global public scope

This guarantees that:
- same-file private helpers work without `export`
- exported tasks keep access to private helpers/constants/schemas from their own file
- exported symbols remain available across files regardless of file scan order

---

## 15. Full Example — Checkout API

```marreta
# routes/checkout.marreta

task calculate_discount(item, category)
    rate = match category
        "VIP" -> 0.15
        "PREMIUM" -> 0.10
        fallback -> 0.0
    item.price * rate

route POST "/checkout" take payload

    # Validations (Guards)
    require payload.user_id else fail 400, "User is required"
    require payload.items else fail 400, "Cart is empty"

    # Client lookup and validation
    client = db.users.find(payload.user_id)
    require client else fail 404, "User not found"
    reject client.delinquent else fail 402, "Payment pending"

    # Processing pipeline
    saved_orders = payload.items
        >> map item
            item.discount = calculate_discount(item, client.category)
            item.total = item.price - item.discount
            item.status = "processed"
            keep item
        >> db.orders.save

    # Broadcast to async services
    saved_orders *>>
        -> queue.push("payment_processing")
        -> queue.push("invoice_emission")
        -> cache.set("last_order:#{client.id}")

    # Response
    reply 201, saved_orders


route GET "/checkout/:id"
    order = db.orders.find(id)
    require order else fail 404, "Order not found"
    reply 200, order
```

---

## 16. Feature Roadmap

| Phase | Scope |
|---|---|
| **v0.1 — Core** | Lexer, Parser, AST, variables, operators, conditionals, tasks, basic REPL |
| **v0.2 — HTTP** | Web server, routes, request/response, URL and body parameters |
| **v0.2.1 — HTTP Expansion** | Multiple take bindings, env object, form/raw types, response modifiers, CORS, 204 fix |
| **v0.3.1 — Schema & AutoDoc** | `schema` declarations, payload type validation (422), OpenAPI 3.0 generation, Swagger UI at `/docs` |
| **v0.3.2 — Multi-file** | `export` keyword, auto-discovery of `.marreta` files, two-pass loading, file-private scope |
| **v0.3.3 — Response Schema** | `reply CODE as schema_name` — response serialization, field filtering, OpenAPI response refs |
| **v0.4.0 — Advanced Schemas** | Nested schema references, `list of Type`, task contracts (`param as schema`), circular reference detection |
| **v0.5 — DB (Relational)** | PostgreSQL connector (`db.*`), CRUD operations, `native_query`, `transaction` block |
| **v0.6 — Error Handling** | `raise` (domain errors), `rescue` (pipeline recovery), Marreta Error Identity (no Rust leakage) |
| **v0.7.0 — Doc CRUD + Query** | MongoDB connector (`doc.*`), Layer 1 CRUD, Layer 2 Query Pipeline |
| **v0.7.1 — Doc Engine Separation** | `MARRETA_DB_*` / `MARRETA_DOC_*` independent env vars; `DocPoolConfig`; dual engine (db.* + doc.* coexist) |
| **v0.7.2 — Doc Aggregation** | Layer 3: `group_by`, `sum`, `avg`, `min`, `max`, post-aggregation ordering |
| **v0.7.3 — Doc Power Pipeline** | Layer 4: `doc.pipeline` escape hatch for advanced MQL aggregations |
| **v0.8 — Queue** | RabbitMQ connector; `on queue/topic` consumers; `queue.push/publish` producers; optional schema contracts; ack/nack semantics |
| **v0.9 — Cache** | Redis connector (`cache.*`); get/set/delete/exists/ttl/expire; atomic incr/decr; bulk get_many/set_many; `set only_if_absent:` for idempotency & locks; optional schema contracts; key prefix + default TTL config |
| **v0.10 — HTTP Client** | Outbound HTTP calls (`http_client.get/post/put/patch/delete`) — compose external APIs from within a route; `reqwest` driver; pipeline in/out; rescue; fire-and-forget |
| **v0.11 — Iteration & Accumulation** | `range(n)` / `range(start, end)` sequence generator; `reduce` pipeline stage; `while` loop (with 10k safety limit); task recursion with depth limit; list methods (`sum`, `mean`, `median`, `std_dev`, `zip`); scalar conversions (`to_integer`, `to_float`, `to_boolean`, `to_string`) |
| **v0.12 — DB Migrations** | `schema` blocks as source of truth for DDL; `marreta migrate` CLI — diff schema vs. DB, generate and apply migrations, rollback support |
| **v0.12b — Migration Hygiene** | `migrate list`, `migrate explain`, `migrate discard`, richer `status`, and explicit migration state workflow (`applied`, `pending`, `changed`, `missing_local`) |
| **v0.12c — Project Entrypoint** | `app.marreta` as canonical project entrypoint; project commands (`serve`, `migrate *`, `doctor`) resolve `./app.marreta` by convention; `project_name` and `project_version` required |
| **v0.12d — Project Metadata Unification** | `project_name` / `project_version` become the canonical metadata source for OpenAPI, `/docs`, `/_health`, and startup banner surfaces |
| **v0.12e — Secret-Aware Config** | Structured infrastructure config for `db`, `doc`, `cache`, and `queue`; project `marreta.env`; process env overrides; stricter config validation for production and CI/CD |
| **v0.12f — Project Doctor** | `marreta doctor` and `marreta doctor --connect` for project structure, intent, config, connectivity, and migration-awareness diagnostics |
| **v0.12g — API Scenario Testing** | `marreta test`; `tests/**/*_test.marreta`; REST-first `scenario` / `given` / `when` / `then`; in-memory route execution; strict external dependency stubs; API coverage reporting |
| **v0.12h — API Scenario Testing Hardening** | Follow-up for matcher ergonomics, scenario-runtime transactions, route-matcher parity, and full 023 test-plan coverage |
| **v0.13 — Security** | Project-wide `auth jwt` / `auth api_key` providers, route-level `require auth` / `allow`, automatic `auth` context, JWT validation (OIDC/JWKS/public key/HMAC), API key hashes, OpenAPI security schemes, doctor reporting, and scenario-test auth mocks |
| **v0.13b — Persistence By Convention** | Persistent schemas activated by `db:` plus `id`; singular references infer FK storage; unambiguous `list of <persistent>` infers inverse navigation; query navigation over inferred relations |
| **v0.13c — If/Else Blocks** | Block `if/else` expressions, `else if`, branch scoping, and route short-circuit preservation |
| **v0.13d — Time API** | `time` namespace, temporal values, intervals, timezone-aware runtime serialization, and formatting |
| **v0.13e — Math Namespace** | `math.*` numeric helpers for API/domain calculations |
| **v0.13f — Filesystem Namespace** | UTF-8 text-first `fs.*` helpers for controlled file I/O |
| **v0.13g — JSON Namespace** | `json.parse`, `json.stringify`, and `json.pretty` with runtime-safe serialization |
| **v0.13h — Base64 Namespace** | Standard and URL-safe Base64 encode/decode helpers |
| **v0.13i — Log Namespace** | Application-authored JSON Lines logging through `log.info/warn/error/debug` |
| **v0.13j — Request Logging** | Automatic HTTP request JSON events gated by `MARRETA_REQUEST_LOG` |
| **v0.13k — UUID Namespace** | `uuid.v4()` and `uuid.v7()` as canonical RFC UUID string generators |
| **v0.13l — W3C Trace Context** | Runtime-only W3C `traceparent`/`tracestate` for HTTP inbound, logs, and outbound `http_client.*` propagation |
| **v0.13m — Async Trace Propagation** | W3C trace context propagation through queue producers and consumers via transport metadata |
| **v0.13n — Runtime Event Log Contract** | Stable JSON event shapes for `app_log`, `request`, `consumer`, and `runtime_error` |
| **v0.13o — Project Init** | `marreta init <project-path>` container-first scaffold with recommended project layout and local runtime image |
| **v0.13p — Feature Flags** | Static boolean feature flags via `feature.enabled(...)` backed by `MARRETA_FEATURE_*` config |
| **v0.13q — Project Init Local Services** | `marreta init --with ...` local-service bootstrap for DB, cache, doc, and queue; app still runs with `marreta serve`, while Docker Compose provides selected backing services only |
| **v0.13r — Schema Constructors + HTTP Client Schemas** | Explicit `SchemaName { ... }` construction returning ordinary maps, plus `http_client.*(...) as Schema` response-body validation |
| **v0.13s — API Contract Types** | Inline schema enums and exact decimal runtime values for API contracts |
| **v0.14a — Formatter** | `marreta fmt` canonical source formatting with check/stdin modes for CI and editors |
| **v0.14b — Lint** | `marreta lint` static source-quality diagnostics with JSON/stdin modes for CI and editors |
| **v0.14c — Editor Tooling / LSP** | CLI-backed completions, hover docs, symbols, and VS Code integration using the Marreta core as source of truth |
| **v1.0 — Production** | Stability, full documentation at marreta.dev, official CLI |

---

## Design Watch Points

These are known tension points in the language design. They are not blockers — the current decisions are defensible — but they must be revisited at each version, tested carefully, and resolved deliberately if they prove problematic in practice.

### 1. ~~Context keywords `like` and `in` inside `where(...)`~~ — RESOLVED (v0.5.1)

`like` and `in` are **pipeline steps**, not context keywords: `>> like("col", "pat")` and `>> in("col", list)`. This removes all context-sensitivity from the parser — they are simply named pipeline stages that happen to produce `FilterOp::Like` / `FilterOp::In` clauses. No lexer changes, no argument list context tracking.

---

### 2. `QueryBuilder` without terminal as startup error

The spec states that a `QueryBuilder` that is never consumed by a terminal (`>> fetch`, `>> count`, etc.) should be caught at startup. The intent is correct — silent no-ops are worse than early errors.

**The risk:** static detection of "this builder is never terminated" requires data-flow analysis across the AST. A `QueryBuilder` can be assigned to a variable, passed to a task, returned from a conditional branch, or stored in a map — all before being terminated. Full coverage is hard. Partial coverage produces false negatives (missed unterminated builders) or false positives (valid code rejected).

**Current decision:** implement as a **runtime error** first. If a `QueryBuilder` reaches a point where its value is consumed as a non-query value (e.g. serialized in a `reply`, passed to `print`, used in arithmetic), the engine raises a clear error: `"QueryBuilder for table 'orders' was never executed — did you forget >> fetch?"`. Startup static analysis is a later refinement, not a v0.5.0 requirement.

**What to watch:** track how often developers hit this error in practice. If it is rare and the runtime message is clear enough, the static analysis may never be worth the complexity.

---

### 3. Pipeline universality under composition pressure

The pipeline operator `>>` is the central abstraction of the language: it works for data transformation, query building, broadcasting, and will eventually power queue consumers and cache lookups. This universality is the language's strongest bet — and its biggest risk.

**The pressure points that will test the abstraction:**
- `*>>` (parallel broadcast) combined with `transaction` — parallel DB writes inside an atomic block raises the question of how isolation interacts with concurrency
- A `QueryBuilder` passed as an argument to a `task` — the task receives a lazy value; should it be able to add steps to it, or only execute it?
- A `map` step inside a pipeline that produces `QueryBuilder` values — e.g. `items >> map item -> db.orders.find(item.id)` — does each map iteration execute independently or does the engine batch?
- Future `listen queue` consumers that pipe into `db.*` operations — the pipeline semantics must survive async consumer context

**What to watch:** at each new version that touches pipeline evaluation, explicitly test the combinations above. Do not defer these to "someday" — they will expose whether the pipeline abstraction is truly universal or needs a seam.

---

### 4. `*>>` inside `transaction` — runtime guardrail

`*>>` (parallel broadcast) and `transaction` (atomic sequential execution) are semantically incompatible. Allowing parallel branches inside an atomic block raises unanswerable questions about isolation and rollback ordering.

**Current decision (v0.5.0):** runtime error. The interpreter tracks an `inside_transaction` flag. If `*>>` is evaluated while that flag is set, execution stops immediately with:

```
RuntimeError: *>> cannot be used inside a transaction block.
Parallel execution and atomicity are mutually exclusive — use sequential >> instead.
```

The flag is already required for `transaction` implementation, so this guardrail costs nothing extra.

**Upgrade path:** a future version may promote this to a startup error via AST walk — detect `Broadcast` nodes inside `Statement::Transaction` bodies at load time. This is feasible because the constraint is lexically detectable. Not a v0.5.0 requirement.

**`*>>` outside `transaction`** — parallel DB queries are explicitly supported and encouraged:

```marreta
# Three independent queries in parallel — costs the time of the slowest, not the sum
user, orders, prefs = payload *>>
    -> db.users.find(payload.user_id)
    -> db.orders >> where(user_id: payload.user_id) >> fetch
    -> db.preferences >> where(user_id: payload.user_id) >> fetch_one
```

---

### 5. `*>>` result ordering under true parallelism

Currently `*>>` is sequential and returns results in declaration order. After the async refactor (Phase 0 of v0.5.0), results will be collected via `try_join_all` which preserves order — but the underlying execution is concurrent.

**The risk:** if any branch has side effects that depend on ordering (e.g. both branches write to the same DB row), the behavior changes silently when moving from sequential to parallel. The spec is silent on this.

**Decision to record:** `*>>` makes **no ordering guarantee on side effects**. Each branch receives an independent copy of the input value and executes concurrently. If ordering of side effects matters, use sequential `>>` pipeline steps instead. This must be documented in user-facing docs when `*>>` parallelism ships.

---

## v0.1 Implementation Notes

The following divergences exist between this specification and the v0.1 interpreter:

1. **`broadcast *>>` returns a List of results.** The spec implies the original input passes through; in practice, `*>>` collects and returns the results from each branch as a List.
2. **Pipeline `>>` with a task implicitly iterates over lists.** When a List is piped to a task, the task is applied to each element and the results are collected into a new List.
3. **`match` works as a standalone expression.** It can be used outside of assignment context (e.g., directly in a pipeline or as the last expression of a task).
4. **Built-in functions: `print()`, `type()`, `len()`.** These are available globally and are not listed in the reserved words section above.
5. **REPL accepts `quit`/`exit` without a dot prefix.** Typing `quit` or `exit` at the REPL prompt terminates the session.
6. **String interpolation (`#{}`) supports any valid expression** — variable, method call, arithmetic, or logical expression.

---

## v0.2.1 Planned Features

The following features are specified for v0.2.1 (see `docs/spec/002a_HTTP_021.md`):

1. **Multiple `take` bindings.** `Route.take` changes from `Option<TakeBinding>` to `Vec<TakeBinding>`. Comma-separated: `take payload, headers`.
2. **`env` object.** All OS environment variables plus project `marreta.env` values (loaded internally via `dotenvy`) injected as a read-only `Value::Map` named `env` into the global startup scope.
3. **`take form`.** New `TakeBinding::Form` — parses `application/x-www-form-urlencoded` bodies via `serde_urlencoded`.
4. **`take raw`.** New `TakeBinding::Raw` — delivers raw request body as `Value::String`. Useful for webhook HMAC verification.
5. **Response modifiers.** `reply html CODE, "..."` and `reply text CODE, "..."` set `Content-Type` accordingly. Optional third argument `{ Key: "val" }` adds extra response headers (enables `reply 302, null, { Location: "..." }`).
6. **CORS middleware.** `tower-http` `CorsLayer` applied globally; controlled via `MARRETA_CORS` and `MARRETA_CORS_ORIGIN` in `marreta.env`.
7. **204 No Content body fix.** `reply 204, null` now returns a 0-byte body per RFC 9110 §15.3.5.

---

## v0.2 Implementation Notes

The following notes describe the v0.2 HTTP Runtime as implemented:

1. **`take query` and `take headers` use the keyword as the variable name.** The syntax `take query` binds the query-string map to a variable named `query`. Likewise `take headers` → variable `headers`, `take payload` → variable `payload`. There is no separate variable name after the binding type.

2. **URL params are injected as `Value::String`.** Path parameters (`:name`) are always strings. Arithmetic on URL params requires a string-to-integer conversion, which is not yet built-in. Use `take payload` to receive typed numbers.

3. **Header names with hyphens are not accessible via property access.** `headers.x-custom-header` is parsed as `headers.x - custom - header` (subtraction). Use identifier-safe header names (no hyphens) when accessing headers in route bodies.

4. **`match` patterns for URL params must be strings.** Since URL params are `Value::String`, match arms should use string literals (`"200"`, `"404"`) rather than integers (`200`, `404`).

5. **`fallback` is the wildcard arm in `match`.** The `_` identifier is not a special wildcard — use the `fallback` keyword for the default arm.

6. **Multi-line map literals are not supported inside route bodies.** All `{ key: value }` maps must be on a single line. The significant-indentation lexer treats the newline as a statement separator.

7. **Per-request isolation via `Environment::clone()`.** Each HTTP request receives a fresh clone of the startup environment. Routes cannot share mutable state across requests.

8. **`Value::Map` is `Arc<RwLock<HashMap>>`.** Required for axum's `Send + Sync` constraint on handler closures. This enables concurrent read access with exclusive write access.

9. **Axum 0.8 path syntax.** Internally Marreta Lang uses `:param` syntax for URL params; the engine converts them to `{param}` format required by axum 0.8 at registration time.

10. **Route body `keep` blocks with nested indentation are not yet supported.** `>> keep` with an indented body inside a route body causes indentation parsing conflicts. Use a named task for filtering instead.
