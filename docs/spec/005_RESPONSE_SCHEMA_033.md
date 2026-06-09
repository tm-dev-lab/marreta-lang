# MarretaLang — Response Schema (v0.3.3)

> Status: Delivered.

> **Meta:** Extend the schema system to cover HTTP *responses*, not just incoming payloads.
> A `reply … as schema_name` binding serializes the route's return value against a declared
> schema, filtering undeclared fields and guaranteeing the shape of every response — without
> the developer manually constructing map literals in every `reply` statement.

---

## 1. Motivation

In v0.3.1/v0.3.2, schemas only validate **input** (`take payload as schema_name`).
Routes still reply with raw map literals:

```marreta
# current — fragile, no contract on the response shape
reply 201, { order_created: true, product_id: payload.product_id, quantity: payload.quantity, total: total }
```

With response schemas, the route becomes:

```marreta
# v0.3.3 — shape declared once, reused everywhere
reply 201 as order_result, result
```

---

## 2. Syntax Design

### 2.1 Schema Declaration — No Change

Schemas are schemas. There is **no distinction in syntax** between a request schema and a
response schema — both use the same `schema` declaration. The same schema can be used as
both a `take payload as X` validator and a `reply as X` serializer if the shapes match.

```marreta
schema order_result
    order_created: boolean
    product_id: integer
    quantity: integer
    unit_price: float
    total: float
    coupon?: string
```

No `_response` suffix convention is imposed — naming is up to the developer.

### 2.2 `reply … as schema_name`

```marreta
reply 201 as order_result, result
```

- `result` must resolve to a `Value::Map` at runtime.
- The engine serializes `result` against `order_result`:
  - Fields declared in the schema **and** present in `result` → included.
  - Fields present in `result` but **not** declared in the schema → **stripped** (never leaked).
  - Fields declared in the schema but **absent** in `result` → `null` if required, omitted if optional (`?`).

### 2.3 Optional fields in response schemas

Same `?` convention as request schemas:

```marreta
schema order_result
    order_id: integer
    total: float
    coupon?: string   # omitted from response if not present in the map
```

---

## 3. Implementation Under The Hood (Rust Engine)

### Phase 1: Parser

Extend `parse_reply()` to recognise the optional `as <schema_name>` suffix before the comma:

```
reply CODE [as SCHEMA_NAME], EXPR
```

`Statement::Reply` gains a new field:

```rust
pub response_schema: Option<String>,
```

All existing `Statement::Reply` constructions gain `response_schema: None`.

### Phase 2: Serializer

Create `src/response_serializer.rs`:

```rust
pub fn serialize(value: &Value, schema: &SchemaDefinition) -> Value
```

- Iterates schema fields in declaration order.
- For each field: reads the value from the `Value::Map`.
  - If present: include as-is (no type coercion — response values are produced by the engine, not untrusted input).
  - If absent and required: include as `Value::Null`.
  - If absent and optional: omit entirely.
- Returns a new `Value::Map` containing only schema-declared fields.

### Phase 3: Server Integration

In `server.rs`, after the route body executes and produces a `reply` response:
- Check `response_schema` on the matched `Statement::Reply`.
- If `Some(name)`, look up the schema in `RouteRegistry.schemas`.
- Call `response_serializer::serialize(&body_value, schema_def)` before JSON serialization.

### Phase 4: OpenAPI

Two improvements to `openapi.rs`:

**4.1 — Response schema ref (when `reply … as schema_name` is used):**

Replace the generic `responses["200"]["description": "Success"]` with a proper content entry
using the actual HTTP status code from the `reply` statement and a `$ref` to the schema:

```json
"responses": {
  "201": {
    "description": "Success",
    "content": {
      "application/json": {
        "schema": { "$ref": "#/components/schemas/order_result" }
      }
    }
  }
}
```

**4.2 — Correct status code in responses (always):**

Currently all routes show `"200": { "description": "Success" }` regardless of the actual
status code used in `reply`. The OpenAPI builder must read `Statement::Reply.status_code`
from the route body and emit the correct code (200, 201, 204, etc.).

> **Note:** A route body may contain multiple `reply` statements (e.g. one inside an `if`
> and one outside). Emit a response entry for each distinct status code found in the body.
> If no `reply` is found, fall back to `"200": { "description": "Success" }`.

> **Implementation note — AST scanner:** The OpenAPI builder runs at startup before any
> request is served — it scans the AST statically, it does not execute the route body.
> `Statement::Reply` and `Statement::Fail` can appear inside nested structures:
> `if/else` blocks, `match` arms, and task bodies called from the route.
>
> Strategy: implement a **shallow recursive collector** that walks `Vec<Statement>` and
> recurses only into `if/else` and `match` bodies directly inside the route body. Do **not**
> follow `TaskCall` / `FunctionCall` nodes — tasks are opaque at this level and their
> replies are not directly observable without execution. Collect every `Statement::Reply`
> and `Statement::Fail` found, deduplicate by status code, and emit the union as the
> `responses` map.
>
> This may produce a conservative over-approximation (e.g. listing both `200` and `201`
> even if only one branch runs at runtime), which is acceptable and standard OpenAPI practice.

---

## 4. Acceptance Criteria

> **Status: COMPLETE (2026-03-26)**

- [x] **(Parser)** `reply 201 as order_result, result` parses correctly. `Statement::Reply.response_schema` is `Some("order_result")`. All existing `reply` statements continue to work with `response_schema: None`.

- [x] **(Serializer)** Fields not declared in the schema are stripped from the response body. Required fields absent from the map are serialized as `null`. Optional fields absent from the map are omitted.

- [x] **(Type safety)** The serializer does not validate types on response fields — it trusts that the route produced correct values. No crash on type mismatch; the value is passed through as-is.

- [x] **(OpenAPI)** Routes with `reply … as schema_name` produce a proper `responses` object with `content.$ref` pointing to the schema component. Correct status codes derived from the AST (not hardcoded 200). Schema-validated routes include 422.

- [x] **(E-commerce example — functional)** `examples/ecommerce/schemas/payloads.marreta` gains `product_created` and `order_created` response schema declarations. `routes/orders.marreta` and `routes/products.marreta` use `reply 201 as schema_name` syntax. The project is fully functional: `marreta serve examples/ecommerce/app.marreta` starts without errors, all endpoints respond correctly, and `/openapi.json` shows response schemas.

- [x] **(Swagger UI — response detail visible)** Each route shows the correct HTTP status code (201 for POST routes, 200 for GETs), a `Schema` section under `Responses` with `$ref` when a response schema is bound, and 422 for schema-validated routes.
