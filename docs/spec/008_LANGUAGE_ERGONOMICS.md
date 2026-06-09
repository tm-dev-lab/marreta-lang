# Implementation Plan — v0.6.0 Language Ergonomics

> **Status:** Complete — v0.6.0 shipped 2026-03-29 (167/167 tests passing)
> **Discovered via:** functional test suite (`examples/functional_tests/`)
> **Goal:** Close the gap between what developers naturally write and what the language currently accepts, without changing the syntactic identity.

---

## Background

The functional test suite revealed seven language gaps that block natural API development. All were found by writing real routes and hitting parser or runtime errors where the syntax *looked* valid. They are grouped into four implementation phases ordered by impact and dependency.

---

## Phase 1 — `fail` as a full expression

### What changes

`fail CODE, EXPR` currently requires `EXPR` to be a string literal. It becomes any expression — variable, map literal, task call, etc.

`fail` also becomes usable as an **expression** (Never type): it can appear on the right-hand side of an assignment or as a `match` arm value. The interpreter propagates the short-circuit the same way it does today when `fail` appears as a statement.

### Syntax

```marreta
# String — already works, continues to work
fail 404, "not found"

# Map literal
fail 404, { error: "not found", code: "ITEM_NOT_FOUND" }

# Variable
fail 422, validation_errors

# Task call
fail 400, build_error("name", "required")

# As expression in match arm (Never — short-circuits the route)
label = match params.role
    "admin"  -> "Administrator"
    "user"   -> "Regular user"
    fallback -> fail 403, { error: "forbidden", role: params.role }

# require...else fail — inherits the same relaxation for free
require row          else fail 404, { error: "not found", id: params.id }
require payload.name else fail 400, { error: "name required", field: "name" }
```

### Acceptance criteria

- [x] `fail CODE, map_literal` returns the map as JSON response body
- [x] `fail CODE, variable` serializes the variable as JSON response body
- [x] `fail CODE, task_call(...)` evaluates the call, serializes result as JSON body
- [x] `fail` as expression in `match` arm short-circuits the route with the given status
- [x] `require ... else fail CODE, map_literal` works
- [x] Existing string `fail` behavior is unchanged
- [x] `examples/functional_tests/app.marreta` updated with map and variable fail cases
- [x] `test.sh` verifies all new cases pass

### Files touched

- `src/parser.rs` — relax `parse_fail` to call `parse_expression` instead of `expect_string`
- `src/parser.rs` — relax `parse_require` error body to call `parse_expression`
- `src/ast.rs` — `Statement::Fail.body: Expression` (was `String`)
- `src/interpreter.rs` — evaluate body expression before short-circuiting
- `src/interpreter.rs` — handle `fail` in expression position (match arm, assignment rhs)

---

## Phase 2 — String interpolation with expressions

### What changes

`#{}` currently performs a variable lookup only. It becomes a full expression evaluator — any valid MarretaLang expression can appear inside `#{}`.

### Syntax

```marreta
# Variable — already works
"Hello #{name}"

# Method call
"Total: #{items.length()} items"
"Path: #{request.path.upper()}"

# Arithmetic
"Price with tax: #{price * 1.1}"
"Page #{page + 1} of #{total_pages}"

# Logical / conditional
"Status: #{active or 'inactive'}"

# Nested interpolation is NOT supported — no #{} inside #{}.
# Evaluate to a variable first if nesting is needed.
```

### Acceptance criteria

- [x] `"#{expr.method()}"` evaluates the method call
- [x] `"#{a + b}"` evaluates arithmetic
- [x] `"#{a or default}"` evaluates logical expressions
- [x] `"#{task_call(arg)}"` evaluates task calls
- [x] Failed expression inside `#{}` propagates error (does not silently return null)
- [x] Simple variable `"#{name}"` still works (no regression)
- [x] `examples/functional_tests/app.marreta` route updated to use method calls in `#{}`
- [x] `test.sh` verifies interpolated expressions resolve correctly

### Files touched

- `src/interpreter.rs` — `interpolate_string`: replace `env.get(var_name)` with a full `parse_expression` + `evaluate` call on the inner string

---

## Phase 3 — Subscript access `expr[key]`

### What changes

A new `Expression::Subscript { object, key }` AST node. The `[key]` suffix can appear after any expression that resolves to a `Map` or `List`. Key can be any expression.

**Map access** — enables hyphenated keys and dynamic key access:

```marreta
# Hyphenated header names — previously inaccessible
token = headers["x-api-key"]
id    = headers["x-request-id"]

# Dynamic key from variable
field = "content-type"
ct    = headers[field]
```

**List access** — enables positional indexing:

```marreta
items[0]          # first element
items[1]          # second element
results[n]        # dynamic index from variable
```

**Out-of-bounds / missing key** — returns `null`, same as `env.get` on a missing variable. Never panics.

### Acceptance criteria

- [x] `map["string-key"]` returns the value for that key
- [x] `map[variable]` resolves variable then accesses key
- [x] `list[0]` returns first element
- [x] `list[n]` resolves `n` and returns element at that index
- [x] Out-of-bounds list index returns `null`
- [x] Missing map key returns `null`
- [x] `headers["x-request-id"]` in a route body works end-to-end
- [x] `examples/functional_tests/app.marreta` updated with subscript access on headers and lists
- [x] `test.sh` verifies subscript access cases

### Files touched

- `src/lexer.rs` — `[` and `]` tokens already exist (list literals); ensure they are emitted in expression position
- `src/parser.rs` — parse `[expr]` suffix as `Expression::Subscript` in `parse_postfix`
- `src/ast.rs` — add `Expression::Subscript { object: Box<Expression>, key: Box<Expression> }`
- `src/interpreter.rs` — evaluate `Subscript`: resolve object, resolve key, index into `Map` or `List`

---

## Phase 4 — `reply` with dynamic status + `keep`/`skip` conditionals

Two independent changes bundled together because they are both small parser relaxations.

### 4a — `reply` with dynamic status

`reply CODE, body` currently requires `CODE` to be an integer literal. It becomes any expression that evaluates to an `Integer`.

```marreta
# Literal — already works
reply 200, result

# Variable
status = 200
reply status, result

# Match expression
status = match role
    "admin" -> 200
    fallback -> 403
reply status, { data: result }
```

### Acceptance criteria (4a)

- [x] `reply variable, body` where variable holds an integer works
- [x] `reply match_expr, body` — match result used as status
- [x] Non-integer status variable returns interpreter error (not panic)
- [x] Literal `reply 200, body` is unchanged
- [x] `examples/functional_tests/app.marreta` updated with dynamic status route
- [x] `test.sh` verifies dynamic status is sent correctly

### Files touched (4a)

- `src/parser.rs` — relax `parse_reply` to call `parse_expression` instead of `expect_integer`
- `src/interpreter.rs` — evaluate status expression, assert Integer, use as HTTP status

---

### 4b — `keep expr if cond` and `skip if cond` inside `map` blocks

**Semantics (decided):**

- `keep expr if cond` — if `cond` is true: element exits with `expr`, block ends. If false: fall through to next statement.
- `keep expr` (unconditional) — always exits with `expr`, block ends.
- `skip if cond` — if `cond` is true: element is **dropped** from the result list, block ends.
- If the block ends with no `keep` or `skip` firing: element is **dropped implicitly**.

This makes multiple `keep if` a cascading alternative, and `skip if` a readable guard at the top:

```marreta
# Cascading alternatives — first matching keep wins
classified = items >> map item
    keep "premium"  if item.score > 100
    keep "standard" if item.score > 50
    keep "basic"    if item.score > 0
    # score <= 0: item is dropped

# Guard + transform
processed = orders >> map order
    skip if not order.active
    skip if order.total <= 0
    keep { id: order.id, net: order.total * 0.9 }

# Mixed: filter and transform in one pass
result = records >> map r
    skip if r.deleted
    keep r.value * 2 if r.type == "double"
    keep r.value
```

### Acceptance criteria (4b)

- [x] `keep expr if cond` — element included when true, skipped when false (no keep fires)
- [x] Multiple `keep if` — first matching arm wins, rest not evaluated
- [x] `skip if cond` — element dropped when true
- [x] `skip if` + unconditional `keep` — clean guard pattern works
- [x] Block with no `keep`/`skip` firing drops element silently
- [x] Existing unconditional `keep expr` is unchanged
- [x] `examples/functional_tests/app.marreta` updated with `keep if` and `skip if` examples
- [x] `test.sh` verifies filtering and cascading alternatives

### Files touched (4b)

- `src/parser.rs` — parse `keep EXPR if EXPR` and `skip if EXPR` inside map block body
- `src/ast.rs` — extend `MapKeep` node: `keep: Expression, condition: Option<Expression>`, add `MapSkip { condition: Expression }`
- `src/interpreter.rs` — evaluate condition before deciding to keep/skip/fall-through; add `Skip` propagation signal alongside existing `Keep`

---

## Phase 5 — String methods `starts_with` / `ends_with`

### What changes

Two new string methods in `value.rs`:

```marreta
"Bearer token123".starts_with("Bearer")   # true
"webhook-prod".ends_with("prod")           # true
"/api/v1/users".starts_with("/api")        # true
```

### Acceptance criteria

- [x] `str.starts_with("prefix")` returns `true`/`false`
- [x] `str.ends_with("suffix")` returns `true`/`false`
- [x] Works in pipeline and interpolation
- [x] `examples/functional_tests/app.marreta` updated with `starts_with`/`ends_with` in strings route
- [x] `test.sh` verifies both methods

### Files touched

- `src/value.rs` — add two arms to `string_method` match

---

---

## Phase 6 — Utility Methods

### What changes

A collection of frequently needed methods across all value types.

### String methods

- `starts_with(s)` → Boolean — already in Phase 5, moved here for grouping
- `ends_with(s)` → Boolean — already in Phase 5, moved here for grouping
- `index_of(s)` → Integer — returns byte index of first occurrence, or -1 if not found

### List methods

- `join(sep)` → String — converts each element to its display string and joins with `sep`
- `sort()` → List — sorts ascending: integers/floats numerically (cross-type), strings alphabetically, then booleans, then nulls; mixed types ordered Integer < Float < String < Boolean < Null
- `unique()` → List — deduplicates preserving insertion order
- `flatten()` → List — depth-1 flatten: List elements are inlined, non-List elements kept as-is
- `slice(from, to)` → List — returns sublist `[from, to)`, out-of-bounds indices clamped

### Map methods

- `delete(key)` → Map — returns a new Map without the specified key
- `size()` → Integer — number of entries (alias for `keys().length()`)

### Float methods

- `round(n?)` → Float — rounds to `n` decimal places (default 0)
- `floor()` → Float — rounds down to nearest integer (as Float)
- `ceil()` → Float — rounds up to nearest integer (as Float)

### Integer and Float methods

- `min(n)` → Integer or Float — returns the lesser of self and `n`
- `max(n)` → Integer or Float — returns the greater of self and `n`

### Acceptance criteria

- [x] `str.index_of("x")` returns Integer byte index, or -1 if not found
- [x] `list.join(", ")` returns a comma-separated string
- [x] `list.sort()` returns sorted list (integers/floats numerically, strings alphabetically)
- [x] `list.unique()` returns deduplicated list, preserving order
- [x] `list.flatten()` inlines nested lists one level deep
- [x] `list.slice(1, 3)` returns sublist at positions 1–2
- [x] `map.delete("key")` returns new map without that key
- [x] `map.size()` returns number of entries as Integer
- [x] `float.round(2)` rounds to 2 decimal places; `round()` rounds to 0
- [x] `float.floor()` rounds down; `float.ceil()` rounds up
- [x] `integer.min(n)` / `integer.max(n)` return the min/max compared to `n`
- [x] `float.min(n)` / `float.max(n)` work with both Integer and Float arguments
- [x] `examples/functional_tests/app.marreta` updated with routes for each method group
- [x] `test.sh` verifies all new method routes pass

### Files touched

- `src/value.rs` — add method arms to `string_method`, `list_method`, `map_method`, `integer_method`, `float_method`

---

## Implementation order

```
Phase 5/6 (trivial, value.rs only)  →  Phase 1 (fail as expr)  →  Phase 2 (interpolation)
                                                  ↓
                                         Phase 3 (subscript)
                                                  ↓
                                         Phase 4a (reply dynamic)
                                         Phase 4b (keep/skip if)
```

Phase 5/6 first because they are one file, zero risk, and get tests passing immediately.
Phases 1 and 2 are independent. Phase 3 unblocks Phase 4a ergonomically.
Phase 4b is the most complex (new AST nodes + interpreter signal).

---

## Functional test coverage plan

All changes land in `examples/functional_tests/app.marreta` as new or updated routes, verified by `test.sh`.

| Phase | New routes in app.marreta | New checks in test.sh |
|-------|--------------------------|----------------------|
| 5 | Update `/strings/methods` | +2 |
| 1 | `/errors/fail_map`, `/errors/fail_var`, `/errors/fail_match` | +5 |
| 2 | Update `/strings/interpolation` | +2 |
| 3 | `/access/map_subscript`, `/access/list_subscript`, update `/bindings/headers` | +6 |
| 4a | `/response/dynamic_status` | +2 |
| 4b | Update `/pipeline/map_filter`, add `/pipeline/skip_guard` | +4 |
| **Total** | ~8 new routes | **~21 new checks** |
