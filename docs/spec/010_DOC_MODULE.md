# MarretaLang — Implementation Plan: Doc Module (v0.7.0 / v0.7.1 / v0.7.2 / v0.7.3)

> Status: Delivered.

**Reviewed and refined: 2026-03-31**
All 14 peer-review issues addressed. Implementation may begin.

**v0.7.0 implemented and reviewed: 2026-03-31**
Layers 1 and 2 shipped. All Phase 1/2/5/6 acceptance criteria checked. See session log in CHANGELOG.md.

**v0.7.1 implemented: 2026-04-01**
`MARRETA_DB_*` / `MARRETA_DOC_*` env var separation. MongoDB pool config via `DocPoolConfig`. Both engines coexist.

**v0.7.2 implemented: 2026-04-01**
Layer 3 Aggregation shipped. `group_by`, `sum/avg/min/max/count` accumulators, `$group` MQL construction, post-group `order`/`limit`. Lexer fix: `>>` at line start suppresses preceding newline. Parser fix: `as` keyword accepted as named arg. 29/29 functional tests passing.

**v0.7.3 implemented: 2026-04-01**
Layer 4 Power Pipeline shipped. `doc.pipeline(collection, list)` dispatched in interpreter. `translate_pipeline_stage` handles 11 stage keys (match, sort, limit, skip, unwind, add_fields, project, count, lookup, group, bucket). Parser fix: reserved keywords accepted as map literal keys (`{ match: {...} }` now valid). Stage validation pass before driver call — unknown-key errors surface in unit tests (MockDocDriver). 38/38 functional tests passing.

---

## 1. Context and Design Rationale

### 1.1 Why a separate module

`db.*` is relational: tables, rows, schemas, joins, SQL. `doc.*` is non-relational: collections,
documents, flexible schema, no joins at the language level. They share operational vocabulary
(`save`, `find`, `update`, `delete`) but differ in query model, data shape, and consistency
guarantees.

Merging both behind a single namespace (`db.*`) would create abstraction leakage — joins make no
sense on MongoDB, transactions mean something different, and column-level typing does not apply to
documents. Separate namespaces (`db.*` and `doc.*`) make the distinction explicit and allow each
module to evolve independently.

### 1.2 The core challenge

The relational DB module has `native_query` as its escape hatch — any SQL that the pipeline cannot
express is written as raw SQL. MongoDB does not have an equivalent that maps cleanly to a string:
MQL is a JSON structure, not a query string. A `native_query` equivalent for MongoDB would require
the developer to write BSON-equivalent maps in Marreta with MongoDB operator names (`$match`,
`$lookup`, `$group`) — which directly leaks driver internals into the language.

This forces a different design philosophy:

> **The `doc.*` pipeline must be expressive enough to cover 90% of real use cases natively. The
> remaining 10% must be reachable through `doc.pipeline()` — a structured escape hatch where Mongo
> concepts are visible by contract, but Mongo syntax (`$`) is minimized.**

### 1.3 Language identity constraints

Every design decision in this plan is evaluated against four constraints derived from the
MarretaLang spec:

1. **No driver internals in developer-facing code** — `$match`, `$lookup`, `ObjectId`, BSON types
   must not appear in `.marreta` files
2. **No JSON strings in code** — maps are Marreta maps, not string-encoded JSON
3. **Pipeline is the natural idiom** — query building is done through `>>` steps, not nested
   function arguments
4. **Errors speak Marreta** — all MongoDB driver errors are translated at the module boundary; no
   `mongodb::error::Error` propagates

### 1.4 Syntax divergence from `db.*` — intentional and documented

`doc.*` query syntax diverges from `db.*` in three places. This is intentional, not an oversight.

---

**`where` field names are strings, not bare identifiers:**

```marreta
# db.* — bare identifiers (SQL column names are always valid identifiers)
db.items >> where(active: true)
db.items >> where(id > 0)

# doc.* — string field names
doc.query("orders") >> where("status" == "pending")
doc.query("orders") >> where("address.city" == "SP")
```

Rationale: MongoDB field names can contain dots (nested path notation: `"address.city"`), hyphens,
and other characters that are not valid Marreta identifiers. String field names are the only
consistent choice across all `doc.*` query operations. A dev familiar with `db.*` who writes
`where(status: "pending")` on a doc query will get a parse error with a clear message.

The parser syntax is unchanged — `where(expr)` is still a function call. The difference is the
LHS of the BinaryOp argument: `Identifier` for `db.*`, `StringLiteral` for `doc.*`. The
interpreter detects the input value type (`QueryBuilder` vs `DocQueryBuilder`) and routes to the
appropriate filter extractor. No new AST variants or lexer tokens are required.

---

**`order` instead of `order_by`, function-call form:**

```marreta
# db.* — order_by with direction encoded in string
db.items >> order_by("name asc")

# doc.* — order with direction as second argument
doc.query("orders") >> order("created_at", "desc")
```

Rationale: `order("field", "asc"|"desc")` avoids encoding sort direction inside a string and
removes ambiguity about cardinality. Using a function call form (vs keyword `order "field" desc`)
avoids adding `asc`/`desc` as new lexer tokens, keeping the implementation entirely within the
interpreter. `db.*` may adopt this form in a future version.

---

**`fetch_all` instead of `fetch`:**

```marreta
# db.* — fetch returns all rows
db.items >> where(active: true) >> fetch

# doc.* — fetch_all is explicit about cardinality
doc.query("orders") >> where("status" == "pending") >> fetch_all
```

Rationale: `fetch` is ambiguous — it could mean one or all. `doc.*` uses `fetch_all` (returns all
matching documents) and `fetch_one` (returns a single document or null). The naming eliminates
cardinality ambiguity. `db.*` may adopt `fetch_all`/`fetch_one` naming in a future version.

---

## 2. Version Split

Following peer review, the original single v0.7.0 scope is split into three sub-versions:

| Version | Scope | Rationale |
|---|---|---|
| **v0.7.0** | Layer 1 (CRUD) + Layer 2 (Query Pipeline) | Core functionality. Covers 80%+ of real-world MongoDB usage. Parser-clean. |
| **v0.7.1** | Env var separation (`MARRETA_DOC_*`) + MongoDB pool config | `MARRETA_DB_*` and `MARRETA_DOC_*` are fully independent; both engines can coexist. |
| **v0.7.2** | Layer 3 (Aggregation) | `group_by`, `sum/avg/min/max/count` accumulators, `$group` MQL construction, post-group `order`/`limit`. |
| **v0.7.3** | Layer 4 (`doc.pipeline`) | Power-user escape hatch. Stage key translation, `$` passthrough contract. Ships after Layers 1–3 are battle-tested. |

---

## 3. API Surface — Four Layers

### Layer 1 — Direct CRUD (v0.7.0)

Atomic single-document operations. No pipeline required.

All arguments are expressions — variables, map literals, task results, or any valid Marreta
expression. The interpreter evaluates them before passing to the driver.

```marreta
# INSERT — returns persisted document with _id as String
order = doc.save("orders", { user_id: 42, total: 199.90, status: "pending" })
order = doc.save("orders", payload)

# READ by _id — returns null if not found
order = doc.find("orders", params.id)

# READ all documents in collection (no filter — use pipeline for filtered reads)
all_orders = doc.find_all("orders")

# PARTIAL UPDATE — $set semantics; returns updated document (find_one_and_update)
updated = doc.update("orders", params.id, { status: "shipped" })
updated = doc.update("orders", params.id, payload)

# DELETE by _id — returns true if deleted, false if not found
deleted = doc.delete("orders", params.id)
```

**Design decisions:**

- `doc.save` always generates `_id` (MongoDB ObjectId → String in Marreta). Developer never
  provides `_id` to `save`.
- `doc.update` is always `$set` — partial merge. Full document replacement is not exposed.
  Silent full-replacement is a common source of data loss in MongoDB usage.
- `doc.find` returns `null` for missing documents, consistent with `db.find`.
- `doc.find_all` has no filter argument. Filtered reads use the pipeline: `doc.query(col) >>
  where(...) >> fetch_all`. The two-argument form `find_all(col, filter_map)` was considered and
  rejected to keep Layer 1 minimal and push filtering to the pipeline (see Section 9).
- `doc.update` uses `find_one_and_update` with `ReturnDocument::After` to return the updated
  document, consistent with the `→ Map` return contract. This is one atomic round-trip in MongoDB.

---

### Layer 2 — Query Pipeline (v0.7.0)

Multi-condition queries built through `>>` steps. All steps are function calls — no new lexer
tokens required.

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

# Comparison operators: ==, !=, >, >=, <, <=
results = doc.query("products")
    >> where("price" >= 10)
    >> where("price" <= 100)
    >> fetch_all

# in — field value is one of the listed values
results = doc.query("orders")
    >> where("status" in ["pending", "processing"])
    >> fetch_all

# like — regex match (case-sensitive)
results = doc.query("users")
    >> like("email", "@gmail.com")
    >> fetch_all

# Ordering and pagination
results = doc.query("orders")
    >> where("status" == "pending")
    >> order("created_at", "desc")
    >> limit(20)
    >> offset(0)
    >> fetch_all

# Field projection — return only specified fields
results = doc.query("orders")
    >> where("user_id" == params.user_id)
    >> pick(["_id", "total", "status", "created_at"])
    >> fetch_all

# _id lookup via pipeline (driver converts string to ObjectId, smart-cast)
order = doc.query("orders")
    >> where("_id" == params.id)
    >> fetch_one

# Terminal: single document (null if not found)
order = doc.query("orders")
    >> where("ref" == params.ref)
    >> fetch_one

# Terminal: count
total = doc.query("orders")
    >> where("status" == "pending")
    >> count

# Terminal: exists
has_pending = doc.query("orders")
    >> where("user_id" == user_id)
    >> where("status" == "pending")
    >> exists

# Terminal: upsert — updates matching or inserts if none match
doc.query("orders")
    >> where("user_id" == params.user_id)
    >> where("ref" == payload.ref)
    >> upsert({ status: "pending", total: payload.total })

# Terminal: bulk update all matching documents
doc.query("orders")
    >> where("status" == "pending")
    >> where("created_at" < cutoff_date)
    >> update({ status: "expired" })

# Terminal: delete all matching documents
doc.query("orders")
    >> where("user_id" == deleted_user_id)
    >> delete
```

**Pipeline steps and their MongoDB translation:**

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

**`_id` smart-cast:**

When `>> where("_id" == val)` is encountered, the driver applies smart-cast logic instead of
hard-rejecting non-ObjectId values:

```rust
fn string_to_id_filter(val: &str) -> Bson {
    if val.len() == 24 && val.chars().all(|c| c.is_ascii_hexdigit()) {
        match ObjectId::parse_str(val) {
            Ok(oid) => Bson::ObjectId(oid),
            Err(_)  => Bson::String(val.to_string()),
        }
    } else {
        Bson::String(val.to_string())
    }
}
```

This ensures compatibility with MongoDB databases that use non-ObjectId `_id` values (UUIDs,
slugs, numeric strings). If the value looks like a valid ObjectId hex, it is cast; otherwise it is
passed as a plain string. No error is raised for non-hex strings.

---

### Layer 3 — Aggregation (v0.7.2)

Group, summarize, and compute across documents.

```marreta
# Group by field — sum and count per group
revenue = doc.query("orders")
    >> where("status" == "paid")
    >> group_by("user_id")
    >> sum("total", as: "revenue")
    >> count(as: "order_count")
    >> fetch_all

# Global aggregation — no group_by, single result document
totals = doc.query("orders")
    >> where("status" == "paid")
    >> sum("total", as: "revenue")
    >> avg("total", as: "avg_order")
    >> count(as: "total_orders")
    >> fetch_one

# Post-aggregation ordering and limit
top_users = doc.query("orders")
    >> where("status" == "paid")
    >> group_by("user_id")
    >> sum("total", as: "revenue")
    >> order("revenue", "desc")
    >> limit(10)
    >> fetch_all
```

`DocQueryState` for aggregation requires post-group fields to distinguish pre-group from
post-group `order`/`limit` steps (see Section 6.3).

---

### Layer 4 — Power Pipeline (v0.7.3)

For aggregations exceeding Layers 1–3. Developer writes MQL pipeline stages as Marreta maps. Keys
are plain Marreta identifiers (no `$`); field-reference values use `"$field"` (string literals)
where MQL requires a field reference — `$` is not a valid identifier character in Marreta.

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

#### 3a. Field reference convention

Values that are string literals starting with `$` are passed through to MQL as-is.
The developer uses `"$fieldName"` wherever MongoDB expects a field reference:

```marreta
# These become { "$sum": "$total" } in MQL
{ group: { by: "status", revenue: { sum: "$total" } } }

# Direct field reference in match expression
{ match: { "$expr": { "$gt": ["$price", "$cost"] } } }
```

The `$` prefix has no special meaning in Marreta syntax — it is just the first character of a
string value. The driver passes it through unchanged.

#### 3b. Stage-by-stage translation rules

`translate_pipeline_stage(stage_map)` receives a single-key Marreta map and returns a
`bson::Document`. The outer key (stage name) gains a `$` prefix. The inner value is translated
recursively via `translate_stage_value`.

**Supported stage keys and their inner translation:**

| Marreta key | MQL stage | Inner value translation |
|---|---|---|
| `match` | `$match` | Recursive map → BSON document (field names kept as-is, values via `value_to_bson`) |
| `sort` | `$sort` | Map of `field: 1` or `field: -1` (Integer) |
| `limit` | `$limit` | Integer |
| `skip` | `$skip` | Integer |
| `unwind` | `$unwind` | String → `"$fieldName"` (adds `$` prefix if not present) |
| `add_fields` | `$addFields` | Recursive map |
| `project` | `$project` | Recursive map |
| `count` | `$count` | String (output field name) |
| `lookup` | `$lookup` | See below |
| `group` | `$group` | See below |
| `bucket` | `$bucket` | See below |

**`lookup` translation:**

```marreta
{ lookup: { from: "users", local: "user_id", foreign: "_id", as: "user" } }
```
Maps to:
```json
{ "$lookup": { "from": "users", "localField": "user_id", "foreignField": "_id", "as": "user" } }
```
Keys `local` → `localField`, `foreign` → `foreignField`. Other keys passed through as-is.

**`group` translation:**

```marreta
{ group: { by: "status", total: { sum: "$amount" }, n: { count: 1 } } }
```
Maps to:
```json
{ "$group": { "_id": "$status", "total": { "$sum": "$amount" }, "n": { "$sum": 1 } } }
```
- `by` → `_id`. Value gets `$` prefix if it is a plain string (field reference). Null for global.
- All other keys → accumulator fields. Sub-map value keys (`sum`, `avg`, `min`, `max`, `first`,
  `last`, `push`, `addToSet`) gain `$` prefix.

**`bucket` translation:**

```marreta
{ bucket: { by: "$price", boundaries: [0, 50, 100, 500], default: "other", output: { count: { sum: 1 } } } }
```
Maps to:
```json
{ "$bucket": { "groupBy": "$price", "boundaries": [0,50,100,500], "default": "other", "output": { "count": { "$sum": 1 } } } }
```
- `by` → `groupBy`
- `output` sub-keys are accumulator maps (same translation as `group`)

**Unknown stage key:** returns `MarretaError::DbError` with message:
`"unknown doc.pipeline stage 'xyz' — supported: match, sort, limit, skip, unwind, add_fields, project, count, lookup, group, bucket"`

#### 3c. `group` in `doc.pipeline` vs `group_by` in Layer 3

They are two different APIs for the same MongoDB `$group`:

| | Layer 3 | Layer 4 |
|---|---|---|
| Entry point | `doc.query(col) >> group_by(...)` | `doc.pipeline(col, [{ group: {...} }])` |
| Syntax | DSL steps with named args | Raw map literal |
| Use case | Common grouping patterns | Full MQL control, multi-stage, cross-collection |
| `_id` field | Auto-added from `group_by` arg | Must be expressed via `by:` key |
| Accumulators | `sum/avg/min/max/count` steps | Sub-map keys: `{ sum: "$field" }` |

They are additive — Layer 3 is the ergonomic path, Layer 4 is the escape hatch.

#### 3d. E2E — routes to add to `app.marreta` (Phase 8)

```marreta
# POST /pipeline/seed — seed orders + users for join test
route POST "/pipeline/seed"
    doc.orders_p.save({ user_id: "u1", status: "paid",    amount: 100 })
    doc.orders_p.save({ user_id: "u1", status: "paid",    amount: 200 })
    doc.orders_p.save({ user_id: "u2", status: "pending", amount:  50 })
    doc.users_p.save({ _id: "u1", country: "BR" })
    doc.users_p.save({ _id: "u2", country: "US" })
    reply 201, { seeded: true }

# GET /pipeline/match — $match filter
route GET "/pipeline/match"
    result = doc.pipeline("orders_p", [
        { match: { status: "paid" } }
    ])
    reply 200, result

# GET /pipeline/group — $group with accumulator
route GET "/pipeline/group"
    result = doc.pipeline("orders_p", [
        { group: { by: "status", total: { sum: "$amount" }, n: { count: 1 } } }
    ])
    reply 200, result

# GET /pipeline/sort-limit — $sort + $limit
route GET "/pipeline/sort-limit"
    result = doc.pipeline("orders_p", [
        { sort:  { amount: -1 } },
        { limit: 2 }
    ])
    reply 200, result

# GET /pipeline/add-fields — $addFields computed field
route GET "/pipeline/add-fields"
    result = doc.pipeline("orders_p", [
        { add_fields: { doubled: { sum: "$amount" } } }
    ])
    reply 200, result
```

Functional test ACs (Phase 8):
- AC-8.1: `POST /pipeline/seed` returns `201 { seeded: true }`
- AC-8.2: `GET /pipeline/match` returns only `status=paid` documents
- AC-8.3: `GET /pipeline/group` returns list with `_id`, `total`, `n` keys per group
- AC-8.4: `GET /pipeline/sort-limit` returns at most 2 documents, ordered by amount desc
- AC-8.5: `GET /pipeline/add-fields` result documents have `doubled` field
- AC-8.6: Unknown stage key in `doc.pipeline` returns `500` with descriptive error message
- AC-8.7: All existing 29 functional tests continue to pass — zero regressions

---

## 4. API Reference Summary

```
# Layer 1 — Direct CRUD
doc.save(collection, map)               → Map
doc.find(collection, id)                → Map | null
doc.find_all(collection)                → List
doc.update(collection, id, partial_map) → Map          (find_one_and_update)
doc.delete(collection, id)              → Boolean

# Layer 2 — Query Pipeline
doc.query(collection)
    >> where("field" OP value)          # OP: ==, !=, >, >=, <, <=
    >> where("field" in [...])
    >> like("field", "pattern")
    >> order("field", "asc"|"desc")
    >> limit(N)
    >> offset(N)
    >> pick(["field", ...])
    >> fetch_all                        → List
    >> fetch_one                        → Map | null
    >> count                            → Integer
    >> exists                           → Boolean
    >> upsert({ partial_map })          → Integer (upserted/updated count)
    >> update({ partial_map })          → Integer (affected count)
    >> delete                           → Integer (deleted count)

# Layer 3 — Aggregation (v0.7.2)
doc.query(collection)
    >> where("field" OP value)          # pre-group filter (optional)
    >> group_by("field")                # optional — omit for global aggregation
    >> sum("field",  as: "alias")       # Accumulator — alias required
    >> avg("field",  as: "alias")
    >> min("field",  as: "alias")
    >> max("field",  as: "alias")
    >> count(as: "alias")               # no field arg
    >> order("alias", "asc"|"desc")     # post-group sort on alias
    >> limit(N)                         # post-group limit
    >> fetch_all                        → List
    >> fetch_one                        → Map | null (global aggregation)

# Layer 4 — Power Pipeline (v0.7.3)
doc.pipeline(collection, List)          → List
```

---

## 5. Error Handling

All MongoDB driver errors are translated at the module boundary. No `mongodb::error::Error`
propagates beyond `src/doc/mongodb.rs`.

```rust
fn translate_mongo_error(err: mongodb::error::Error, collection: &str, op: &str) -> MarretaError {
    let operation = format!("doc.{}.{}", collection, op);
    let message = match err.kind.as_ref() {
        ErrorKind::InvalidArgument { message, .. } => message.clone(),
        ErrorKind::Write(WriteFailure::WriteError(e)) => e.message.clone(),
        ErrorKind::Command(e) => e.message.clone(),
        _ => err.to_string(),
    };
    MarretaError::DbError { message, operation }
}
```

- `error.code` for doc errors: `"db_error"` — same as relational DB errors.
- `error.op` carries `"doc.{collection}.{operation}"` (e.g. `"doc.orders.save"`,
  `"doc.users.query"`) to distinguish DB from Doc errors in `rescue` handlers.
- All `doc.*` operations are `rescue`-compatible:

```marreta
order  = doc.find("orders", id) rescue null
result = doc.query("events") >> where("type" == "click") >> count rescue 0
```

---

## 6. Configuration

Document and relational databases use **separate** environment variable namespaces so both can
coexist in the same deployment:

```
# Relational (PostgreSQL)
MARRETA_DB_PROVIDER=postgres
MARRETA_DB_URL=postgres://user:pass@host:5432/database
MARRETA_DB_POOL_MAX_CONNECTIONS=10
MARRETA_DB_POOL_MIN_CONNECTIONS=1
MARRETA_DB_POOL_ACQUIRE_TIMEOUT_SECS=30
MARRETA_DB_POOL_IDLE_TIMEOUT_SECS=600
MARRETA_DB_POOL_MAX_LIFETIME_SECS=1800
MARRETA_DB_POOL_TEST_BEFORE_ACQUIRE=true

# Document (MongoDB)
MARRETA_DOC_PROVIDER=mongodb
MARRETA_DOC_URL=mongodb://user:pass@host:27017/database
MARRETA_DOC_POOL_MAX_CONNECTIONS=10
MARRETA_DOC_POOL_MIN_CONNECTIONS=0
MARRETA_DOC_POOL_CONNECT_TIMEOUT_MS=10000
MARRETA_DOC_POOL_SERVER_SELECTION_TIMEOUT_MS=30000
```

`DocEngine::from_config` reads `MARRETA_DOC_PROVIDER` and `MARRETA_DOC_URL` independently of
`DbEngine::from_config`. Both engines can be active simultaneously. Setting neither set of vars
leaves the corresponding engine as `None` (disabled).

---

## 7. Architecture

### 7.1 New files

```
src/doc/
  mod.rs       — module declaration, DocEngine, from_config, re-exports
  mongodb.rs   — MongoDbDriver: connect, CRUD, query execution
  query.rs     — DocQueryState, DocFilter, SortDirection, DocQueryMode
```

### 7.2 `Value::DocQueryBuilder` — new interpreter value variant

`doc.query("collection")` returns a new `Value` variant carrying the accumulated query state:

```rust
Value::DocQueryBuilder(Box<DocQueryState>)
```

`Box<DocQueryState>` matches the existing `Value::QueryBuilder(Box<QueryState>)` pattern.
`Arc<RwLock<>>` is unnecessary — the builder is always used linearly within a single pipeline
evaluation and carries no shared ownership.

The interpreter's `evaluate_pipeline_stage` is extended:

```rust
// Existing db.* path (unchanged):
let input = if let Value::DbTable(table) = input {
    Value::QueryBuilder(Box::new(QueryState::new(table)))
} else { input };

// New doc.* path:
let input = if let Value::DocNamespace = input {
    // doc.query("col") is a FunctionCall evaluated by dispatch_doc_direct
    input
} else { input };

// Dispatch:
if let Value::QueryBuilder(ref q) = input {
    if let PipelineStage::Expression(expr) = stage {
        return self.apply_query_pipeline_stage(q, expr);  // existing
    }
}
if let Value::DocQueryBuilder(ref q) = input {
    if let PipelineStage::Expression(expr) = stage {
        return self.apply_doc_pipeline_stage(q, expr);    // new
    }
}
```

### 7.3 `DocQueryState` struct (v0.7.0 — Layers 1–2)

```rust
pub struct DocQueryState {
    pub collection:  String,
    pub filters:     Vec<DocFilter>,
    pub projection:  Option<Vec<String>>,
    pub sort:        Option<(String, SortDirection)>,
    pub limit:       Option<i64>,
    pub offset:      Option<i64>,
    pub mode:        DocQueryMode,
}

pub enum SortDirection { Asc, Desc }

pub enum DocQueryMode {
    Fetch,             // fetch_all / fetch_one
    Count,
    Exists,
    Update(Value),     // >> update { map }
    Upsert(Value),     // >> upsert { map }
    Delete,
}

pub enum DocFilter {
    Eq(String, Value),
    Ne(String, Value),
    Gt(String, Value),
    Gte(String, Value),
    Lt(String, Value),
    Lte(String, Value),
    In(String, Vec<Value>),
    Like(String, String),
}
```

**For v0.7.2 (Aggregation)**, `DocQueryState` gains post-group fields to handle `order`/`limit`
steps appearing after accumulators:

```rust
// Added in v0.7.2:
pub group_by:      Option<String>,    // None = global aggregation (no $group _id)
pub accumulators:  Vec<Accumulator>,
pub post_sort:     Option<(String, SortDirection)>,
pub post_limit:    Option<i64>,
// DocQueryMode gains: Aggregate
```

### 7.3a `Accumulator` enum (v0.7.2)

Each accumulator step in the pipeline corresponds to a MongoDB `$group` accumulator field.

```rust
pub enum Accumulator {
    Sum   { field: String, alias: String },  // >> sum("amount", as: "revenue")
    Avg   { field: String, alias: String },  // >> avg("amount", as: "avg_order")
    Min   { field: String, alias: String },  // >> min("amount", as: "min_order")
    Max   { field: String, alias: String },  // >> max("amount", as: "max_order")
    Count { alias: String },                 // >> count(as: "total") — no field arg
}
```

**Parsing rules for accumulator steps:**

All accumulators accept a named `as:` argument that sets the output field alias. The `as:` arg is
required — omitting it produces an interpreter error:
`"sum() requires a named 'as:' argument (e.g. sum(\"amount\", as: \"revenue\"))"`.

```marreta
>> sum("amount", as: "revenue")     # Accumulator::Sum { field: "amount", alias: "revenue" }
>> avg("total",  as: "avg_price")   # Accumulator::Avg { field: "total",  alias: "avg_price" }
>> min("score",  as: "min_score")   # Accumulator::Min { field: "score",  alias: "min_score" }
>> max("score",  as: "max_score")   # Accumulator::Max { field: "score",  alias: "max_score" }
>> count(as: "n")                   # Accumulator::Count { alias: "n" } — no field
```

`count` takes only one argument (`as:`), no positional field. Passing a positional arg to `count`
is an interpreter error: `"count() does not accept a field argument — use count(as: \"alias\")"`.

### 7.3b Interpreter dispatch for aggregation steps (v0.7.2)

`apply_doc_pipeline_stage` is extended to recognise the new step names. Steps are categorised into
three phases determined by the order in which they appear in the pipeline chain:

**Pre-group phase** — steps that apply before `$group` (same as Layer 2 filters):
- `where`, `like`, `in` → append to `DocQueryState.filters`

**Group definition phase:**
- `group_by("field")` → sets `DocQueryState.group_by = Some(field)`. Once set, subsequent
  accumulator steps attach to this group. May appear only once; a second `group_by` in the same
  chain is an interpreter error.

**Accumulator phase** — steps that define `$group` output fields:
- `sum`, `avg`, `min`, `max`, `count` → append to `DocQueryState.accumulators`.
  These steps also set `DocQueryState.mode = DocQueryMode::Aggregate`.

**Post-group phase** — steps that apply after `$group` (on the aggregated result set):
- `order("alias", "asc"|"desc")` → sets `DocQueryState.post_sort`
- `limit(N)` → sets `DocQueryState.post_limit`

**Validation rules:**
1. An accumulator step before any `group_by` activates **global aggregation** (no `_id` in
   `$group`). This is valid — `group_by` is optional.
2. A `group_by` after an accumulator step is an interpreter error:
   `"group_by() must appear before accumulator steps (sum, avg, min, max, count)"`.
3. A write terminal (`update`, `upsert`, `delete`) after any accumulator step is an interpreter
   error: `"write terminals (update/upsert/delete) cannot follow aggregation steps"`.
4. `pick` after an accumulator is an interpreter error:
   `"pick() cannot be used in aggregation pipelines — use accumulator aliases directly"`.

### 7.3c MQL `$group` construction (v0.7.2)

`MongoDbDriver` gains a `query_aggregate` method. When `DocQueryState.mode == Aggregate`, the
driver builds an aggregation pipeline instead of a find query:

```
Stage 1 — $match (if filters present):
    { "$match": <filter_doc> }           ← same as build_query_filter()

Stage 2 — $group:
    {
      "$group": {
        "_id": "$field"                  ← None → null (global aggregation)
        "alias": { "$sum": "$field" }    ← for Accumulator::Sum
        "alias": { "$avg": "$field" }    ← for Accumulator::Avg
        "alias": { "$min": "$field" }    ← for Accumulator::Min
        "alias": { "$max": "$field" }    ← for Accumulator::Max
        "alias": { "$sum": 1 }           ← for Accumulator::Count (counts docs)
      }
    }

Stage 3 — $sort (if post_sort present):
    { "$sort": { "alias": 1 | -1 } }

Stage 4 — $limit (if post_limit present):
    { "$limit": N }
```

The result documents from `$group` always include `_id` (the group key). This is exposed as-is in
the `Value::Map` returned to the interpreter — the developer accesses it as `result._id`.

For global aggregation (`group_by = None`), `_id` is `null` in the result. The developer typically
uses `fetch_one` to get the single result document.

**Driver method signature:**

```rust
async fn query_aggregate(&self, q: &DocQueryState) -> DocResult<Vec<DocRow>>;
```

Added to the `DocDriver` trait. `MockDocDriver` implements it returning a configurable `Vec<DocRow>`.

### 7.4 Filter extraction — how `doc.*` `where` differs from `db.*`

The existing `extract_filter_from_expr` (interpreter.rs) reads raw AST and currently requires
`Expression::Identifier` on the LHS of a BinaryOp. For `doc.*`, a parallel
`extract_doc_filter_from_expr` is added that accepts `Expression::StringLiteral` on the LHS:

```rust
fn extract_doc_filter_from_expr(&mut self, expr: &Expression) -> Result<DocFilter, MarretaError> {
    match expr {
        Expression::BinaryOp { left, operator, right } => {
            let field = match left.as_ref() {
                Expression::StringLiteral(s) => s.clone(),  // "address.city"
                _ => return Err(MarretaError::TypeError {
                    message: "doc where() filter: left side must be a string field name \
                              (e.g. where(\"status\" == \"pending\"))".to_string(),
                    ..
                }),
            };
            let op = match operator { /* same mapping as db.* */ };
            let value = self.evaluate(right)?;
            Ok(DocFilter::from(field, op, value))
        }
        // in: Expression::FunctionCall { name: "in", ... } — separate path
        _ => Err(...)
    }
}
```

No new AST variants, no new lexer tokens. The disambiguation between `db.*` and `doc.*` filter
extraction is done entirely by the value type of the pipeline input.

### 7.5 BSON ↔ Marreta `Value` mapping

| BSON type | Marreta `Value` | Notes |
|---|---|---|
| `ObjectId` | `Value::String` (24-char hex) | Always; developer never handles `ObjectId` |
| `String` | `Value::String` | |
| `Int32` / `Int64` | `Value::Integer` | |
| `Double` | `Value::Float` | |
| `Boolean` | `Value::Boolean` | |
| `Null` | `Value::Null` | |
| `Array` | `Value::List` | |
| `Document` | `Value::Map` | |
| `DateTime` | `Value::String` (ISO 8601) | Date arithmetic not in scope for v0.7.0 |
| `Decimal128` | `Value::Float` | Precision may be lost — documented limitation |

### 7.6 MongoDB client lifecycle

`MongoDbDriver` holds a single `mongodb::Client` instance created once during `connect()` and
shared via `Arc` through the engine. The MongoDB Rust driver manages its own internal connection
pool. A new `Client` must never be created per-request or per-query — doing so exhausts file
descriptors under load. This mirrors the `PostgresDriver` pattern (`PgPool` held in struct,
shared via `Arc<dyn DbDriver>`).

### 7.7 MockDocDriver for unit tests

Following the same pattern as `MockDriver` for `db.*` in `interpreter.rs`, a `MockDocDriver`
(and `DocDriver` trait) is added in `#[cfg(test)]` blocks. The mock supports configurable return
values for all operations so that interpreter tests for `doc.*` require no real MongoDB instance.

---

## 8. Acceptance Criteria

### Phase 1 — Direct CRUD (v0.7.0)

- [x] AC-1.1: `doc.save(col, map)` inserts document, returns map with `_id` as String
- [x] AC-1.2: `doc.save(col, variable)` works — variable is evaluated before driver call
- [x] AC-1.3: `doc.find(col, id)` returns document map for valid id; `null` for missing
- [x] AC-1.4: `doc.find_all(col)` returns List of all documents
- [x] AC-1.5: `doc.update(col, id, partial)` updates only listed fields (`$set`), returns updated document
- [x] AC-1.6: `doc.update(col, id, variable)` works — variable evaluated before driver call
- [x] AC-1.7: `doc.delete(col, id)` returns `true` when deleted, `false` when not found
- [x] AC-1.8: `_id` from `doc.save` is usable in subsequent `doc.find` call

### Phase 2 — Query Pipeline (v0.7.0)

- [x] AC-2.1: `>> where("field" == val)` filters by equality
- [x] AC-2.2: `>> where("field" > val)` / `>=` / `<` / `<=` / `!=` filter correctly
- [x] AC-2.3: `>> where("address.city" == "SP")` accesses nested field via dot notation
- [x] AC-2.4: `>> where("status" in ["a", "b"])` filters by list of values
- [x] AC-2.5: `>> like("email", "@gmail.com")` filters by regex (case-sensitive)
- [x] AC-2.6: `>> order("created_at", "desc")` returns documents in descending order
- [x] AC-2.7: `>> order("created_at", "asc")` returns documents in ascending order
- [x] AC-2.8: `>> limit(N)` + `>> offset(M)` returns correct page
- [x] AC-2.9: `>> pick(["_id", "total"])` returns only specified fields
- [x] AC-2.10: `>> fetch_one` returns single document or `null`
- [x] AC-2.11: `>> count` returns Integer count
- [x] AC-2.12: `>> exists` returns `true`/`false`
- [x] AC-2.13: `>> where("_id" == hex_string)` smart-casts to ObjectId; non-hex string passed as plain String
- [x] AC-2.14: `>> upsert({ map })` inserts when no match; updates when match found
- [x] AC-2.15: `>> update({ partial })` updates all matching documents, returns Integer count
- [x] AC-2.16: `>> delete` deletes all matching documents, returns Integer count
- [x] AC-2.17: Multiple `>> where` steps combine with AND semantics
- [x] AC-2.18: `db.*` call when provider is `mongodb` returns clear error message

### Phase 3 — Aggregation (v0.7.2)

- [x] AC-3.1: `>> group_by("field") >> sum("f", as: "a") >> fetch_all` returns grouped list with alias key and `_id` key
- [x] AC-3.2: `>> group_by("field") >> count(as: "a")` counts documents per group
- [x] AC-3.3: `>> group_by("field") >> avg("f", as: "a")` computes average per group
- [x] AC-3.4: `>> group_by("field") >> min("f", as: "a")` and `>> max("f", as: "a")` compute min/max per group
- [x] AC-3.5: Global aggregation (no `group_by`) with `>> sum("f", as: "a") >> fetch_one` returns single map with `_id: null`
- [x] AC-3.6: `>> group_by >> sum >> order("alias", "desc") >> limit(N) >> fetch_all` applies sort/limit after grouping
- [x] AC-3.7: `>> where` before `>> group_by` is applied as pre-group `$match`
- [x] AC-3.8: Multiple accumulators in the same chain produce all fields in the result
- [x] AC-3.9: `group_by` after an accumulator step returns interpreter error
- [x] AC-3.10: Write terminal (`update`/`upsert`/`delete`) after accumulator returns interpreter error
- [x] AC-3.11: `pick` after accumulator returns interpreter error
- [x] AC-3.12: `count` with a positional field arg returns interpreter error
- [x] AC-3.13: Any accumulator without `as:` arg returns interpreter error with descriptive message
- [x] AC-3.14: `MockDocDriver.query_aggregate` returns configurable `Vec<DocRow>` — no real MongoDB needed for unit tests

### Phase 4 — Power Pipeline (v0.7.3)

- [x] AC-4.1: `doc.pipeline(col, [{ match: {...} }])` filters documents
- [x] AC-4.2: `lookup` stage key is accepted and translated (local→localField, foreign→foreignField)
- [x] AC-4.3: `unwind` stage adds `$` prefix to field name if not already present
- [x] AC-4.4: `group` stage translates `by` → `_id`, accumulator sub-keys gain `$` prefix; `count: N` → `$sum: N`
- [x] AC-4.5: `sort` and `limit` stages apply ordering and pagination
- [x] AC-4.6: `add_fields` stage translates to `$addFields` with recursive value translation
- [x] AC-4.7: `bucket` stage translates `by` → `groupBy`, `output` sub-keys as accumulator maps
- [x] AC-4.8: Unknown stage key produces `DbError` with descriptive message (not panic); validated before driver call

### Phase 5 — Error Handling (v0.7.0)

- [x] AC-5.1: MongoDB driver errors translated at module boundary — no `mongodb::` types in `MarretaError`
- [x] AC-5.2: `doc.find` on missing document returns `null`, not error
- [x] AC-5.3: `doc.save` with invalid data returns `DbError` with driver message
- [x] AC-5.4: `doc.*` operations are `rescue`-compatible
- [x] AC-5.5: `error.code` for doc errors is `"db_error"`
- [x] AC-5.6: `error.op` for doc errors is `"doc.{collection}.{operation}"`

### Phase 6 — Examples + E2E (v0.7.0)

- [x] AC-6.1: `examples/doc_ops/docker-compose.yml` created with MongoDB service
- [x] AC-6.2: `examples/doc_ops/app.marreta` covers all CRUD and query pipeline routes
- [x] AC-6.3: 25/25 functional tests passing against real MongoDB (manual + docker)
- [x] AC-6.4: All existing tests continue to pass — zero regressions

### Phase 8 — Examples + E2E for Power Pipeline (v0.7.3)

New routes added to `examples/doc_ops/app.marreta` — Section 4 of the test app.
Functional tests added to `examples/doc_ops/test.sh`.

- [x] AC-8.1: `POST /pipeline/seed` returns `201 { seeded: true }`
- [x] AC-8.2: `GET /pipeline/match` returns only `status=paid` documents (all items have `.status == "paid"`)
- [x] AC-8.3: `GET /pipeline/group` returns list with `_id`, `total`, `n` keys per group
- [x] AC-8.4: `GET /pipeline/sort-limit` returns at most 2 documents, ordered by amount desc
- [x] AC-8.5: `GET /pipeline/add-fields` result documents have `doubled` field
- [x] AC-8.6: Unknown stage key in `doc.pipeline` produces `DbError` with descriptive message — validated by unit tests in interpreter.rs (MockDocDriver path)
- [x] AC-8.7: All existing 29 functional tests continue to pass — zero regressions

### Phase 7 — Examples + E2E for Aggregation (v0.7.2)

New routes added to `examples/doc_ops/app.marreta` — same app, aggregation section appended.
Functional tests added to `tests/http_integration_tests.rs`.

Routes to add to `app.marreta`:

```marreta
# POST /agg/seed — insert batch of documents for aggregation tests
route POST "/agg/seed"
    doc.save("sales", { product: "A", category: "electronics", amount: 100 })
    doc.save("sales", { product: "B", category: "electronics", amount: 200 })
    doc.save("sales", { product: "C", category: "clothing",    amount:  50 })
    doc.save("sales", { product: "D", category: "clothing",    amount:  75 })
    reply 201, { seeded: true }

# GET /agg/by-category — group by category, sum amount
route GET "/agg/by-category"
    result = doc.query("sales")
        >> group_by("category")
        >> sum("amount", as: "total")
        >> count(as: "items")
        >> order("total", "desc")
        >> fetch_all
    reply 200, result

# GET /agg/totals — global aggregation (no group_by)
route GET "/agg/totals"
    result = doc.query("sales")
        >> sum("amount",  as: "grand_total")
        >> avg("amount",  as: "avg_amount")
        >> count(as: "total_items")
        >> fetch_one
    reply 200, result

# GET /agg/top-electronics — pre-group filter + aggregation
route GET "/agg/top-electronics"
    result = doc.query("sales")
        >> where("category" == "electronics")
        >> group_by("product")
        >> sum("amount", as: "revenue")
        >> order("revenue", "desc")
        >> limit(10)
        >> fetch_all
    reply 200, result
```

Functional test assertions (`tests/http_integration_tests.rs`):

- [x] AC-7.1: `POST /agg/seed` returns `201 { seeded: true }`
- [x] AC-7.2: `GET /agg/by-category` returns list with 2 groups; electronics `total=300`, clothing `total=125`
- [x] AC-7.3: `GET /agg/by-category` each group has `_id`, `total`, and `items` keys
- [x] AC-7.4: `GET /agg/totals` returns single map with `grand_total=425`, `avg_amount=106.25`, `total_items=4`
- [x] AC-7.5: `GET /agg/totals` result has `_id: null` (global aggregation marker)
- [x] AC-7.6: `GET /agg/top-electronics` returns only electronics items; `revenue` field present
- [x] AC-7.7: All existing 25 functional tests continue to pass — zero regressions

---

## 9. Implementation Steps

### Phase 1 — Driver scaffolding (v0.7.0)

1. Add `mongodb` crate to `Cargo.toml` (async, feature `tokio-runtime`)
2. Create `src/doc/mod.rs`, `src/doc/mongodb.rs`, `src/doc/query.rs`
3. Implement `DocDriver` trait with CRUD and query methods. **Note:** `DocDriver` is NOT a mirror
   of `DbDriver`. `DbDriver` is SQL-centric (`QueryState` with joins, `select_cols`, `begin()`
   returning `DbTx`). `DocDriver` is MongoDB-native: `query_fetch` receives `DocQueryState` (not
   `QueryState`), includes `aggregate(pipeline: Vec<Document>)` for v0.7.1, and has no `begin()`
   (MongoDB transactions require replica set — out of scope). The two traits share naming
   conventions but are separate interfaces.
4. Implement `MongoDbDriver` with `connect(url)`, singleton `mongodb::Client` held in struct
5. Implement BSON ↔ Marreta `Value` conversion: `bson_to_value`, `value_to_bson`
6. Implement `translate_mongo_error()` — no `mongodb::error::Error` propagates beyond module
7. Add `"mongodb"` branch to `DbEngine::from_config` in `src/db/mod.rs`
7b. Add runtime guard: when `db.*` is called and provider is `mongodb`, return
    `DbError { message: "db.* operations require a relational provider (postgres). Current
    provider: mongodb", operation: "db.connect" }`. This guard is checked in the interpreter at
    `db.*` dispatch time, not in `from_config`.
8. Unit tests: `translate_mongo_error` for all `ErrorKind` variants; BSON conversion for all
   type mappings; `db.*` guard under mongodb provider returns expected error

### Phase 2 — CRUD interpreter dispatch (v0.7.0)

9. Add `Value::DocQueryBuilder(Box<DocQueryState>)` and `Value::DocNamespace` variants to `value.rs`
10. Add `doc.*` method recognition in interpreter: `dispatch_doc_direct` handles `doc.save`, `doc.find`, `doc.find_all`, `doc.update`, `doc.delete`, `doc.query`
11. `doc.query("collection")` returns `Value::DocQueryBuilder(Box::new(DocQueryState::new(collection)))`
12. Add `MockDocDriver` in `#[cfg(test)]` following `MockDriver` pattern — configurable return values, no MongoDB dependency

### Phase 3 — Query pipeline (v0.7.0)

13. Extend `evaluate_pipeline_stage` to branch on `Value::DocQueryBuilder`
14. Implement `apply_doc_pipeline_stage` — accumulates filters, sort, limit, offset, pick into `DocQueryState`
15. Implement `extract_doc_filter_from_expr`: accepts `StringLiteral` LHS (vs `Identifier` for `db.*`)
16. Implement `_id` smart-cast in `DocFilter::Eq` when field name is `"_id"`
17. Implement terminal dispatch: `fetch_all`, `fetch_one`, `count`, `exists`, `update`, `upsert`, `delete`
18. Implement `>> like("field", "pattern")` → `DocFilter::Like`
19. Implement `>> order("field", "asc"|"desc")` → `SortDirection`
20. Implement `>> pick(["f1", "f2"])` → `DocQueryState.projection`
21. Unit tests for filter translation (`DocFilter` → BSON document), all operators

### Phase 4 — Aggregation (v0.7.2)

22. Add `Accumulator` enum to `src/doc/query.rs` (Sum/Avg/Min/Max/Count with `field`/`alias` fields)
23. Extend `DocQueryState` with `group_by: Option<String>`, `accumulators: Vec<Accumulator>`,
    `post_sort: Option<(String, SortDirection)>`, `post_limit: Option<i64>`
24. Add `DocQueryMode::Aggregate`; activated in `apply_doc_pipeline_stage` when any accumulator
    step or `group_by` is processed
25. Extend `apply_doc_pipeline_stage` in interpreter:
    - `"group_by"` → set `group_by`, validate not called after accumulators
    - `"sum"`, `"avg"`, `"min"`, `"max"` → parse `(field_str, as: alias_str)`, push to `accumulators`,
      set mode to `Aggregate`
    - `"count"` → parse `(as: alias_str)` only, reject positional field arg, push `Accumulator::Count`
    - `"order"` when mode is `Aggregate` → set `post_sort` (not `sort`)
    - `"limit"` when mode is `Aggregate` → set `post_limit` (not `limit`)
    - Validate write terminals, `pick` after aggregation
26. Add `query_aggregate(&self, q: &DocQueryState) -> DocResult<Vec<DocRow>>` to `DocDriver` trait
27. Implement `query_aggregate` in `MongoDbDriver`:
    - Stage 1: `$match` from `build_query_filter(q)` (skip if no filters)
    - Stage 2: `$group` — `_id` from `group_by` (or `null`), one field per accumulator
    - Stage 3: `$sort` from `post_sort` (if present)
    - Stage 4: `$limit` from `post_limit` (if present)
    - Execute via `collection.aggregate(pipeline)`
28. Implement `query_aggregate` in `MockDocDriver` (returns configurable `Vec<DocRow>`)
29. Terminal dispatch: when `mode == Aggregate`, route `fetch_all`/`fetch_one` to `query_aggregate`
30. Unit tests (MockDocDriver, no real MongoDB):
    - All 5 accumulator variants produce correct `DocQueryState`
    - `group_by` after accumulator → error
    - write terminal after accumulator → error
    - `pick` after accumulator → error
    - `count` with positional arg → error
    - accumulator without `as:` → error
    - global aggregation (no `group_by`) produces `_id: null` in `$group`
    - post-group `order`/`limit` populate `post_sort`/`post_limit`

### Phase 5 — Power pipeline (v0.7.3)

30. Add `doc.pipeline(collection, list)` to interpreter dispatch
31. Implement `translate_pipeline_stage(map: &Value) -> Result<Document, MarretaError>`
32. Implement field-reference passthrough — values starting with `$` passed through as-is
33. Return `DbError` for unknown stage keys (not panic)
34. Unit tests for each supported stage key

### Phase 6 — Examples + E2E (v0.7.0)

35. Add MongoDB service to `examples/functional_tests/docker-compose.yml`
36. Add `examples/functional_tests/app.marreta` sections: CRUD, query pipeline (filters, pagination, projection)
37. Update `test.sh` with assertions for all new routes
38. Run full suite — 0 regressions required

---

## 10. Design Decisions Closed

| Question | Decision |
|---|---|
| **Single provider v0.7.0** | Accepted. Path to multi-provider is `DriverRegistry` — not premature. |
| **`$` in Layer 4 values** | Accepted. `$` in values is the Layer 4 contract — field-reference convention surfaces only here. |
| **`upsert` as 3-arg Layer 1 vs pipeline terminal** | Pipeline terminal. `>> upsert({ data })` after `>> where` steps is unambiguous. Three-arg form with two positional maps of same type is rejected. |
| **`like` case sensitivity** | Case-sensitive by default (MongoDB `$regex` default). `>> like_i` deferred — not in v0.7.0. |
| **DateTime as ISO 8601 String** | Accepted for v0.7.0. Date arithmetic deferred. |
| **`_id` handling** | Smart-cast: 24-char hex → attempt ObjectId conversion; fallback to plain String. No hard rejection. Compatible with UUID/slug/numeric _id values. |
| **`Box<DocQueryState>` vs `Arc<RwLock<>>`** | `Box` — matches `Value::QueryBuilder` pattern. Builder is used linearly; no shared ownership or lock contention needed. |
| **No new AST variants for pipeline steps** | Option A adopted. All `doc.*` pipeline steps are `PipelineStage::Expression(FunctionCall {...})`, same as `db.*`. Disambiguation at runtime by input value type. |
| **No new lexer tokens for `order`** | `order("field", "asc"\|"desc")` function-call form avoids adding `asc`/`desc` as lexer keywords. |
| **`fetch_all` vs `fetch`** | `doc.*` uses `fetch_all`/`fetch_one` for explicit cardinality. `db.*` keeps `fetch`/`fetch_one` for now. May unify in a future version. |
| **`find_all` without filter** | `doc.find_all(col)` has no filter argument. Filtered reads use `doc.query(col) >> where(...) >> fetch_all`. Two-argument form rejected to keep Layer 1 minimal. |
| **`doc.update` return type** | Returns updated `Map` via `find_one_and_update` with `ReturnDocument::After`. Consistent with `db.*` `update_by_id` `RETURNING *` contract. One atomic operation in MongoDB. |
| **MockDocDriver** | Added to implementation steps. All interpreter tests for `doc.*` must work without a real MongoDB instance, following the `MockDriver` pattern from `db.*`. |
| **Aggregation post-group steps** | Deferred to v0.7.1. `DocQueryState` extended with `post_sort`/`post_limit` when v0.7.1 is implemented. |
| **Write-after-aggregation guard** | Deferred to v0.7.1, AC-3.8: `>> update`/`>> upsert`/`>> delete` after `>> group_by` or accumulator → interpreter error. |
| **Keywords as map literal keys** | During v0.7.3 implementation, `{ match: {...} }` failed to parse because `match` tokenizes as `TokenKind::Match`, not `Identifier`. Fixed in `parse_map_literal` via a new `expect_identifier_or_keyword_as_key` helper that accepts any reserved keyword token as a map key. All keyword tokens are now valid map keys — they use their lexeme string as the key. This affects all map literals in the language, not only `doc.pipeline` calls. |
| **`count` inside `$group` accumulator** | `count: N` in a `doc.pipeline` group accumulator translates to `$sum: N` (not `$count: N`). MongoDB's `$count` is a pipeline stage, not a `$group` accumulator. Discovered during v0.7.3 functional testing. `accumulator_mql_key("count")` returns `"$sum"`. |
| **Stage validation before driver call** | `doc.pipeline` stages are validated via `translate_pipeline_stage` in the interpreter, before calling `driver.raw_pipeline`. This ensures unknown-key errors surface for all drivers (including `MockDocDriver` in unit tests), not only when a real MongoDB connection is present. |

---

## 11. Out of Scope

**v0.7.0:**
- Multi-provider (Postgres + MongoDB simultaneously)
- Aggregation (Layer 3) — deferred to v0.7.1
- Power Pipeline (Layer 4) — deferred to v0.7.2
- `>> like_i` (case-insensitive regex)
- Native date/time type in Marreta

**All versions:**
- `$where` JavaScript expressions — security boundary; will never be supported
- GridFS (binary file storage)
- Change streams / watch
- Transactions (require replica set — deferred)
- Index management
- `$or` / `$nor` at the pipeline level (Layer 4 `match` stage handles these)
- Geospatial operators
- Full-text search (`$text`)
