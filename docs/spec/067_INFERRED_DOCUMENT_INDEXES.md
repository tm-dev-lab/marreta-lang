# 067 - Inferred Document Indexes

> Status: Delivered
> Type: Language runtime (document provider, static analysis + serve startup)
> Scope: The document provider creates the indexes an app needs by inferring them from the queries the app actually runs, instead of asking the developer to declare them. Static analysis of the document query pipeline at load produces a per-collection index plan; `serve` ensures those indexes in the background at startup. No declaration, no schema marker, no migration. Document-only by design (the relational provider tolerated the launch scenario unindexed, proven in the Spec 066 benchmark). Pre-launch, supersedes a reverted declarative approach (see History).

> History: A declarative approach (an `index` / `index unique` schema directive plus a `doc:` collection marker, with relational migration indexes) was implemented and reverted pre-launch, before any release. Rationale for the U-turn: a declaration only protects the developer who already knows indexes exist, while the audience this language is built for is exactly the one who will not declare and whose app collapses under load; and the `doc:` marker added ceremony off the language's convention-over-configuration identity (it also had no call-site enforcement, so it could index the wrong collection silently). The two pieces that do not depend on declaration, the `unique_violation` 409 mapping and the document-driver ensure machinery, were preserved by cherry-pick from tag `pre-067-revert` and are reused here.

---

## 1. Purpose

The launch benchmark (Spec 066) proved the problem with numbers. The same app, capped at 1 CPU on a dedicated host, on the route that lists an account's transactions:

```ruby
route GET "/accounts/:id/transactions"
    rows = doc.transactions
    >> where("account_id" == params.id)
    >> order("_id", "desc")
    >> limit(20)
    >> fetch_all
```

- On `doc` (MongoDB) it collapsed to about 35 to 50 req/s, over 90 percent error, 60s timeouts, the app idle at about 0.7 percent CPU while MongoDB pegged at about 650 percent scanning.
- To rule out the interpreter (a lock, the connection pool, a concurrency bug), the same app was run on `db` (Postgres): it sustained about 1000 req/s, 0 percent error, about 80 percent CPU. So it is not the runtime.
- Creating the index on `transactions.account_id` by hand in MongoDB brought the `doc` run back to about 1000 req/s, 0 percent error.

The cause is the missing index, and it is specific to the document provider. An unindexed MongoDB collection scan is catastrophic under load, while Postgres tolerates the sequential scan at this volume (Postgres has no index on `account_id` either, since foreign keys are not auto-indexed). So `db` is out of scope here, proven to tolerate the launch scenario; if a relational case ever hurts, it is a separate discussion.

The lesson is not "give the developer a way to declare an index". It is "the app must not collapse because nobody declared one". The developer who would reach for an index declaration is the one who already understands the problem. The audience of a zero-ceremony language is the one who never will, and that is exactly who hits the 1000 req/s wall (we did, and we wrote the language). Index by convention is more on-identity than index by declaration.

Static inference is viable because the document query DSL is restricted by design: a filter names its field as a string literal (`where("account_id" == ...)`), and the runtime already analyzes the pipeline statically at load (an unfinished `QueryBuilder` is a startup error). The fields an app queries are readable from the source.

## 2. The change

### 2.1 Inference from the query surface

At load, after the routes and tasks parse, walk every document query pipeline (`doc.<collection> >> ...`) and collect, per collection, the shape of each query: which fields are filtered by equality, which are sorted (with direction), and which are filtered by a range comparison. This produces a deterministic per-collection index plan. Nothing about runtime traffic or data is consulted; it is pure static analysis of the code.

The filter shape feeds inference regardless of the pipeline's terminal. A `where` that scans is a scan whether it ends in `fetch_all`, `update`, `delete`, `count`, or `exists`, so write and aggregate terminals are inputs too, not only reads (the same `where("account_id" == ...)` collapse happens on a bulk update).

### 2.2 Composite indexes by the ESR rule

A single-field index on the filtered field already restored the Spec 066 run, but the route's real shape (`where("account_id" == ...)` plus `order("_id", "desc")`) wants the composite `{ account_id: 1, _id: -1 }`, and the restricted DSL hands the categories over for free. Apply the standard Mongo compound-index rule, **ESR (Equality, Sort, Range)**:

- `where` equality step → an Equality field,
- `order` step → a Sort field (with its direction),
- `where` comparison step → a Range field,
- `where ... in [...]` → a Range field: `$in` behaves like equality on its own but like a range once a sort is present, so the conservative classification places it after the Sort segment, which keeps the index serving the sort.

Per query shape, the inferred index is `[equality fields, canonicalized] + [sort fields, with direction] + [range fields]`. The Equality segment is sorted lexicographically within the shape: equality fields are interchangeable in ESR, so `where(a) + where(b)` and `where(b) + where(a)` must produce the single index `{a, b}`, not two redundant ones that prefix dedup would miss. The Sort and Range segments keep source order, where order is significant. Then deduplicate by prefix across the shapes of the same collection: `{a}` is dropped when `{a, b}` exists, because the compound index already serves the prefix. The algorithm is deterministic and small.

### 2.3 What inference deliberately excludes

- `like()` (regex): a regex uses an index only with an anchored prefix, so an index inferred from it is probably useless. Skipped.
- `doc.pipeline(...)`: the raw aggregation escape hatch is out of the analysis.
- Any shape with a non-literal field (if the grammar ever allows it): skip that shape and continue.
- Indirection through a variable (`q = doc.transactions` then `q >> where(...)`): not captured, consistent with the static-only rule (the pipeline input must be a literal `doc.<collection>` or `doc.query("collection")`). A query written this way simply gets no inferred index.

No cardinality estimation, no usage statistics, no traffic sampling. Inference reads the code, nothing else.

### 2.4 Ensure in the background, never blocking startup

For an empty or small collection (every new app) the ensure is instant. The hard case is adding a new filter to an app whose collection is already large in production: if `serve` blocked on the build, a deploy that took seconds would take minutes, the orchestrator would kill the pod on a probe timeout, and a crash loop would be triggered by an innocent route edit with no signal in the code.

So `serve` ensures the inferred indexes **in the background, concurrent with serving, never delaying the bind** (the ensure is spawned on the runtime before the server is blocked on, so a slow build runs alongside serving rather than ahead of it). MongoDB 4.2+ builds an index online (it does not lock the collection), so the app comes up and serves immediately; a brand-new query shape runs slow-but-alive until its build finishes, which is exactly today's pre-index behavior. The serve startup progress logs mark each build's start, ready, and failure (the same plain progress style as the provider-connection logs, not structured event-log contract events). A build failure is logged by `serve` and never brings down the server. Concurrent `createIndex` with the same spec across instances is idempotent in MongoDB, so there is no multi-instance race.

`doctor` is a separate process from `serve`, so it cannot observe the build lifecycle. It connects to the provider and compares the inferred plan to the indexes actually present, and reports three states: **present**, **absent** (which covers both "not built yet" and "build failed", indistinguishable from outside), and **orphan** (an owned index no longer in the plan, for a human to remove). The build lifecycle (start / ready / failure) lives in the serve logs, where it belongs.

The tradeoff is documented in one sentence for users: a new filter on a large collection serves unindexed until the background build finishes.

### 2.5 Ownership naming, and no auto-drop

Every inferred index gets a deterministic name under a Marreta-owned scheme (the naming and ownership machinery preserved from the reverted work). Marreta only ever touches indexes that match that scheme, so hand-made indexes are never disturbed.

Marreta never drops an index automatically. An inferred index that becomes orphaned (its route was refactored away) stays in place, and `doctor` reports it for a human to remove. Auto-drop driven by a code refactor is the symmetric trap of auto-create: commenting out a route temporarily must not drop an index that costs hours to rebuild.

### 2.6 Preserved from the reverted approach (cherry-picked with tests)

Two pieces do not depend on declaration and are reused as-is, lifted by cherry-pick from tag `pre-067-revert` so their hardened tests come with them (reimplementing from memory is how the log call-site bug caught in the prior review would return):

- **The 409 mapping** (`UniqueConstraintViolation`, the `pg_unique_violation` / `mongo_unique_violation` classifiers, the stable response body, `status_for_error` shared with the event log). It is provider error translation: it fires for any unique index, including one a human created by hand. It stays, and the errors documentation describes it independent of how the index was born.
- **The driver ensure machinery** (`ensure_index` / `list_index_names`, idempotency, deterministic naming and ownership) and the `doctor` plumbing. This is exactly what inference consumes, rescoped from "ensure what was declared" to "ensure what was inferred".

### 2.7 No in-language escape hatch at launch

Inference covers the 80 percent (equality plus sort, the bulk of a REST API). Advanced indexes (partial, TTL, text) are created by hand directly in the store, coexisting safely under the ownership rule of 2.5 (Marreta does not touch them). If inference ever needs an override (force an index, or suppress one on a write-heavy collection), explicit declaration can return later as a layer on top of inference, the inverse of the reverted approach: convention by default, declaration as the exception, and only with evidence of demand.

## 3. Implementation outline

- **Static analysis (`src/...` load path):** hook the per-collection shape collection into the existing load-time pipeline analysis; build the index plan (ESR + prefix dedup).
- **Serve (`src/main.rs` / serve startup):** spawn the ensure off the request path, after the port binds; drive it through the preserved driver machinery; structured build logs; `doctor` per-index state.
- **Preserved pieces:** cherry-pick the 409 mapping and the ensure/ownership machinery (with tests) from tag `pre-067-revert` as the first commits of the branch, then rescope the machinery's caller from declared schemas to the inferred plan.
- **Removed (the reverted declarative surface):** the `index` / `index unique` directive and `doc:` marker (lexer/parser/AST), the document index validations in `persistent_schema`, and the relational migration index machinery (dead once no surface triggers it). Git history retains it via the tag.
- **No relational change:** `db` is untouched.

## 4. Out of scope, and what is consciously lost

`db` is out of scope (proven to tolerate the launch scenario). Beyond that, three things are given up deliberately, and are named so the decision is conscious, not accidental:

- **Uniqueness has no language expression again.** It is a domain rule, not inferable from queries, and Spec 066 was about performance, not integrity. The `unique_violation` 409 stays alive for an index a human marks unique by hand. A minimal uniqueness declaration is a possible future concern.
- **A declared relational index has no language path.** The workaround is `db.native_query`, which accepts DDL, and the benchmark proved `db` tolerates the launch scenario without one.
- **There is no opt-out.** A write-heavy collection with a rarely-run query gets an index anyway, write cost included. That is the price of convention and it is the 20 percent; the answer is the future override (2.7), not added complexity now.

## 5. Acceptance criteria

1. Indexes are inferred from the document query surface at load (static analysis of `doc.<collection> >> where/order/...`), with no declaration, no `doc:` marker, and no migration.
2. The inferred index for an equality filter plus a sort is the ESR composite (for the Spec 066 route, `{ account_id: 1, _id: -1 }`), and shapes are deduplicated by prefix per collection.
3. `like()`, `doc.pipeline`, and non-literal-field shapes are excluded from inference.
4. `serve` ensures the inferred indexes in the background, concurrent with serving, never delaying the bind; the app serves immediately even when a build is in progress; build start/ready/failure are logged by `serve`; a build failure does not bring it down. `doctor` (a separate process) reports each inferred index as present or absent and flags orphan owned indexes; it does not observe the build lifecycle, which lives in the serve logs.
5. Inferred indexes carry deterministic Marreta-owned names; Marreta never touches an index outside that scheme; an orphaned inferred index is reported by `doctor`, never auto-dropped.
6. The preserved 409 mapping and ensure/ownership machinery come from tag `pre-067-revert` by cherry-pick, with their tests; their semantics are preserved. The ensure signature is extended to carry index direction (`keys: &[(String, bool)]`), and the cherry-picked tests adapt to it rather than being rewritten.
7. The reverted declarative surface (`index`/`unique`/`doc:`, the `persistent_schema` doc index validations, the relational migration index machinery) is gone; `db` is unchanged.
8. The History note is present in this spec, the CHANGELOG records the rewind ("supersedes a reverted pre-release declarative approach, see tag `pre-067-revert`"), and SPEC.md §1.3 records the house rule (a spec that never reached a release may be rewound; a shipped spec gets superseded).
9. **Functional coverage of the new behavior** (the house rule: exercise the new behavior end-to-end, not just no-regression): a functional test against a real MongoDB asserts that, for the Spec 066 query shape, the inferred composite index exists on the collection by its Marreta-owned name and with keys `{ account_id: 1, _id: -1 }`, read back through `list_index_names`. The old declarative fixtures vanish in the rewind, so this is the feature's only live coverage.
10. Standard gates: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, the full test suite, and the runtime tier (`functional_tests`, `migrations_functional` — which verifies AC7's "db is unchanged", since the cherry-picked 409 classifier touches `src/db/postgres.rs` — and `e2e`); document guide pages are Spec 069.

---

## Delivery notes

Delivered. The document provider creates the indexes an app needs by inferring them from the query surface, with no declaration, no `doc:` marker, and no migration.

- **Inference engine** (`src/doc/index_inference.rs`): a total exhaustive AST walker collects each `doc.<collection>` / `doc.query("collection")` pipeline's shape and builds the index by the ESR rule (Equality canonicalized, Sort with direction, Range, `in` as Range), deduplicated by prefix per collection. `like`, `doc.pipeline`, non-literal fields, and variable indirection are excluded; multi-argument `where` and non-literal `order` direction are handled exactly as the runtime parses them. Deterministic `idx_`-owned names with a hash fallback.
- **Serve** (`src/main.rs`, `src/file_loader.rs`): the load infers the plan once (per-collection dedup is global) onto `LoadedProject`; serve spawns the ensure on the runtime before blocking on the server, so it runs concurrent with serving and never delays the bind. A new query shape on a redeploy is ensured idempotently on the next startup; a build failure is logged and never crashes serve.
- **Doctor** (`src/doctor.rs`): a "Document indexes" section lists the inferred plan, and with a live connection reports present / absent / orphan (it cannot observe the build lifecycle, which lives in the serve logs). The orphan line tells a human to verify nothing else uses it (doc.pipeline aggregations are not analyzed) before dropping.
- **Preserved from the reverted declarative approach** (cherry-picked with their hardened tests from tag `pre-067-revert`): the `unique_violation` 409 mapping and the document-driver ensure machinery. Per AC6, their semantics are preserved; the ensure signature was extended from `keys: &[String]` to `keys: &[(String, bool)]` to carry index direction (the ESR composite has `{_id:-1}`), and the cherry-picked tests adapted to the new signature rather than being rewritten.
- **Rewind**: this spec supersedes a reverted pre-release declarative approach (`index` / `index unique` / `doc:` plus relational migration indexes), preserved at tag `pre-067-revert`. The rewind followed the §1.3 house rule (a spec that never reached a release may be rewound, a shipped spec gets superseded), verified clean (no release tag descends from the rewound commits, the merge had no PR artifact).
- **Gates** all green: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, the unit suite, `functional_tests` (567/567, including the AC9 assertion that the inferred composite `{ account_id: 1, _id: -1 }` is physically present in real MongoDB by its owned name), `e2e`, and `migrations_functional` (db unchanged).
- **Follow-up (cross-repo)**: update the references to "Spec 067" in the marreta-lang-stealth notes (security section and backlog) from the old declarative meaning to the inference meaning, closing the rewind loop.

## P.S. Do not forget the docs of record

On delivery, update both `CHANGELOG.md` and `docs/spec/SPEC.md` (see SPEC.md §1.3), including the rewind line and the new §1.3 house rule. Guide documentation is Spec 069. Reserved-word normalization (`doc`/`feature`/`env`) is the trimmed Spec 068. After the force-push, ping the reviewer to update the external stealth references to "Spec 067".
