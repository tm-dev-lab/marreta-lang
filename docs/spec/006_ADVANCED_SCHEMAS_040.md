# MarretaLang — Advanced Schemas & Task Contracts (v0.4.0)

> Status: Delivered.

> **Meta:** Elevate the schema system from simple HTTP payload validation to a robust internal type system. Enable schema composition (nesting schemas and lists of schemas) for complex enterprise APIs, and introduce Task Contracts — allowing developers to shield internal functions with the exact same schemas used in routes, catching errors early and bringing static-like guarantees to a dynamic environment.

---

## 1. Motivation

Up until v0.3.3, MarretaLang schemas are flat (only primitive types) and only gatekeep HTTP boundaries (requests and responses). However, real-world APIs receive highly nested JSON structures (e.g., an order with a list of items and a billing address).

Furthermore, data passed internally between `route` and `task` relies on "Duck Typing", which means a developer has to mentally map properties or risk runtime crashes if a task is called with the wrong map structure.

v0.4.0 solves this by making schemas recursive and allowing tasks to enforce them.

---

## 2. Syntax Design

### 2.1 Nested Schemas (Composition)

A schema can now reference another declared schema by using its name as a field type.

```marreta
schema address
    street: string
    city: string
    zipcode: string

schema user_payload
    name: string
    age: integer
    billing: address    # referencing another schema by name
```

### 2.2 Lists of Typed Data

Use `list of Type` (or `list of Schema`) to validate arrays of objects or arrays of primitives.
The syntax follows the established keyword-driven style of the language — no angle brackets or
generics notation, just plain English composition.

```marreta
schema order_item
    product_id: integer
    quantity: integer

schema order_payload
    client_id: integer
    items: list of order_item    # validating an array of objects
    tags?: list of string        # validating an array of primitives (optional)
```

> **Why `list of X` instead of `list[X]`?** Every type composition in MarretaLang uses
> keywords, not symbols (`take payload as schema`, `reply 201 as result`). Bracket generics
> would be a syntactic outlier. `list of X` reads naturally and stays consistent with the
> rest of the language.

### 2.3 Task Contracts (Signature Validation)

Tasks can optionally bind a schema to any parameter using `as <schema_name>` inline — the
same `as` pattern already used for payload bindings and reply schemas. If the task is called
with a value that does not conform to the bound schema, the engine halts execution with a
descriptive `TypeError` before the task body runs.

```marreta
# as qualifies the parameter, just like:
#   take payload as order_payload   (payload binding)
#   reply 201 as order_result, ...  (reply binding)
task apply_taxes(order as order_payload)
    order.total * 1.15

# multiple params — schemas are per-param, unbound params are unrestricted
task process(order as order_payload, discount)
    order.total * (1 - discount)
```

Tasks continue to use implicit return (last expression) and indentation-based blocks —
no `return` keyword, no `end` keyword.

> **Circular schema references** (A references B, B references A) are detected at startup
> and reported as a configuration error. Self-referential schemas are not supported in v0.4.0.

---

## 3. Implementation Under The Hood (Rust Engine)

### Phase 1: Lexer & AST Update

**`token.rs` & `lexer.rs`:**
- Add `TokenKind::Of` keyword to support `list of Type` syntax.

**`ast.rs`:**
- Enhance `SchemaType` enum to support composition:

```rust
pub enum SchemaType {
    StringType,
    IntegerType,
    FloatType,
    BooleanType,
    MapType,
    ListType,
    Reference(String),          // e.g., `billing: address`
    TypedList(Box<SchemaType>), // e.g., `items: list of order_item`
}
```

- Update `Statement::TaskDef` parameters from `Vec<String>` to `Vec<ParamDef>`:

```rust
pub struct ParamDef {
    pub name: String,
    pub schema: Option<String>, // e.g., `order as order_payload` → Some("order_payload")
}
```

### Phase 2: Recursive Validator

Update `validator.rs`:
- When a field's `SchemaType` is `Reference(schema_name)`:
  - Look up `schema_name` in the schema registry.
  - Recursively validate the nested `Value::Map` against that schema.
- When a field's `SchemaType` is `TypedList(inner)`:
  - Expect a `Value::List`.
  - For each element, run the inner type validation rules.
- Error paths must be accumulative: report `billing.zipcode is required` instead of just `zipcode is required`.

**Circular reference detection** runs at startup (route loader phase), before any request is
served, by traversing the schema reference graph and detecting cycles. A descriptive startup
error is emitted: `"circular schema reference detected: address → user_payload → address"`.

### Phase 3: Task Contract Enforcement

Update `interpreter.rs` at the task call evaluation step:
- After arguments are evaluated but **before** the task body executes, check each `ParamDef`
  for a bound schema.
- If `schema` is `Some(name)`, look up the schema and invoke `validator::validate(arg_value, schema_def)`.
- On failure, return `MarretaError::TypeError` with a message such as:
  `"Task 'apply_taxes' expected argument 'order' to match schema 'order_payload'. Field 'total' is missing."`
- The HTTP status code for a `TypeError` triggered inside a route is **500** — this is a
  programmer error (wrong internal call), not a client error.

### Phase 4: OpenAPI Refinement

Refactor `schema_type_to_openapi` in `openapi.rs` — the current signature
`(&str, Option<&str>)` cannot represent nested structures. Change it to return `serde_json::Value`
directly:

```rust
fn schema_type_to_openapi(t: &SchemaType) -> serde_json::Value
```

- `SchemaType::Reference("address")` → `{ "$ref": "#/components/schemas/address" }`
- `SchemaType::TypedList(inner)` → `{ "type": "array", "items": <inner mapped recursively> }`
- Primitive types → `{ "type": "string" }`, `{ "type": "integer", "format": "int64" }`, etc.

The Swagger UI will render nested schemas as collapsible property trees natively.

---

## 4. Acceptance Criteria

- [x] **(Parser — Nesting)** `billing: address` parses to `SchemaType::Reference("address")`.

- [x] **(Parser — Lists)** `items: list of order_item` parses to `SchemaType::TypedList(Reference("order_item"))`. `tags?: list of string` parses to `SchemaType::TypedList(StringType)` with `optional: true`.

- [x] **(Parser — Task Contract)** `task apply_taxes(order as order_payload)` parses correctly. `ParamDef { name: "order", schema: Some("order_payload") }`. Tasks without schema contracts parse as before (`schema: None`).

- [x] **(Recursive Validation)** A POST payload with a nested object missing required sub-schema fields returns HTTP 422. Error message navigates the path: `"field 'billing.city' is required"`.

- [x] **(List Validation)** HTTP 422 is returned when an array element violates the inner schema (e.g., a string where `list of integer` is expected).

- [x] **(Circular Reference Detection)** A startup error is emitted — not a panic — when schemas form a reference cycle. The server does not start.

- [x] **(Task Contract Enforcement)** Calling a schema-bound task with a non-conforming argument halts execution before the task body runs and returns HTTP 500 with a descriptive `TypeError` naming the task, the parameter, and the failing field.

- [x] **(OpenAPI Generation)** `/openapi.json` emits `$ref` for nested schema properties and `{ "type": "array", "items": ... }` for typed lists. Swagger UI renders nested objects as collapsible trees.

- [x] **(E-commerce example — functional)** `examples/ecommerce/` is extended to exercise all v0.4.0 features and must remain fully functional (`marreta serve examples/ecommerce/app.marreta` starts without errors, all endpoints respond correctly):
   - A nested schema is introduced — `address` schema referenced inside `order_payload` (`billing: address`).
   - A typed list is introduced — `items: list of order_item` inside `order_payload`.
   - At least one task uses a schema contract — `get_coupon_rate(order as order_payload)`.
   - `POST /orders` accepts and validates the extended payload (nested object + list of items).
   - `/openapi.json` shows nested `$ref` and `array` types for the updated schemas.
   - Swagger UI renders the nested order structure as a collapsible tree.
