# 042 — API Contract Types: Enum and Decimal

> Status: Delivered
> Type: Schema contract / runtime numeric type
> Scope: Add enum fields and exact decimal values for API contracts

---

## 1. Purpose

Marreta schemas already describe API payload shape:

```marreta
schema Charge
    amount: float
    currency: string
    status: string
```

Two common API contract needs are still too loose:

1. **Closed string sets.** Fields such as status, kind, mode, state, and type
   are usually constrained to a known list of values.
2. **Exact decimal values.** Monetary APIs should not use binary floating point
   for amounts.

This spec introduces:

```marreta
schema Charge
    amount: decimal
    currency: string
    status: enum ["pending", "paid", "cancelled"]
```

The intent is to improve API contracts without turning Marreta into a domain
finance framework.

---

## 2. Design Principles

1. **Enums are schema validation, not runtime identity.**
   An enum field evaluates to a normal string after validation. No enum object,
   enum namespace, or nominal type is introduced in the first cut.

2. **Decimals are exact runtime values.**
   A decimal field evaluates to a native `decimal` runtime value, not a float
   disguised as decimal text.

3. **Money is an application schema, not a primitive.**
   Marreta provides `decimal`; applications model money with schemas such as:

   ```marreta
   schema Money
       amount: decimal
       currency: string
   ```

4. **No hidden lossy coercion.**
   Floats do not coerce into decimals automatically. If a value was born as a
   binary float, it has already lost the exactness this feature exists to
   preserve.

5. **JSON interop stays precision-safe.**
   Decimal values serialize to JSON strings by default.

---

## 3. Enum Schema Fields

### 3.1 Syntax

Enum fields are declared inline in schemas:

```marreta
schema Order
    status: enum ["pending", "paid", "cancelled"]
```

Optional enum fields use the existing optional marker:

```marreta
schema Order
    status?: enum ["pending", "paid", "cancelled"]
```

### 3.2 Runtime Representation

After validation, enum values are normal strings:

```marreta
if order.status == "paid"
    reply 200, { settled: true }
```

`match` works with existing string semantics:

```marreta
label = match order.status
    "pending" -> "Waiting"
    "paid" -> "Paid"
    "cancelled" -> "Cancelled"
    fallback -> "Unknown"
```

### 3.3 Validation Rules

Schema declaration validation:

- Enum value list must be literal.
- Enum value list must not be empty.
- Enum values must be strings.
- Duplicate enum values are a startup/doctor error.

Payload/runtime validation:

- Value must be a string.
- Value must match one of the declared enum values exactly.
- Matching is case-sensitive.
- Optional enum fields may be absent or `null`.

### 3.4 Examples

```marreta
schema Shipment
    status: enum ["created", "packed", "shipped", "delivered"]

route POST "/shipments" take payload as Shipment
    reply 201 as Shipment, payload
```

Invalid payload:

```json
{ "status": "shiped" }
```

Expected result: schema validation error.

### 3.5 Non-Goals

- No nominal enum declarations:

  ```marreta
  enum OrderStatus
      pending
      paid
  ```

- No enum namespace or introspection in the first cut.
- No enum-to-integer mapping.
- No automatic OpenAPI enum reuse component unless the existing OpenAPI builder
  can emit inline enum constraints cleanly.

---

## 4. Decimal Runtime Type

### 4.1 Syntax

Schema fields:

```marreta
schema Charge
    amount: decimal
    currency: string
```

Explicit construction in code:

```marreta
amount = decimal("19.90")
fee = decimal("2.50")
total = amount + fee
```

### 4.2 Rust Representation

Use `rust_decimal::Decimal` for `Value::Decimal`.

Rationale:

- Exact base-10 arithmetic.
- Mature crate with serde/sqlx ecosystem support.
- Enough magnitude for monetary APIs.
- Fixed-size representation with predictable memory and performance.
- Avoids arbitrary-precision `BigDecimal` heap growth and denial-of-service
  risk from unbounded decimal calculations.
- With 2 decimal places, max value is approximately:

  ```text
  792281625142643375935439503.35
  ```

This is sufficient for normal financial APIs without taking on arbitrary-size
`BigDecimal` runtime cost.

### 4.3 Accepted Inputs

`decimal("...")` accepts strings in canonical decimal form:

```marreta
decimal("19.90")
decimal("-19.90")
decimal("0")
decimal("1000000000000000000.01")
```

Schema coercion accepts:

- String decimals: `"19.90"`
- Integers: `19` -> `decimal("19")`

Schema coercion rejects:

- Floats: `19.90`
- Non-numeric strings
- Scientific notation such as `"1e3"`
- Values outside the decimal range

Rejecting floats is intentional: it prevents binary floating-point imprecision
from entering decimal contracts silently.

Rejecting scientific notation is also intentional. Decimal API contracts should
favor plain monetary/base-10 notation over magnitude-oriented float notation.

### 4.4 JSON Serialization

Decimal values serialize as JSON strings:

```marreta
reply 200, { amount: decimal("19.90") }
```

Response:

```json
{ "amount": "19.90" }
```

This protects JavaScript and JSON clients from precision loss.

### 4.5 Operators

Decimal supports arithmetic:

```marreta
total = decimal("19.90") + decimal("2.50")
discounted = total - decimal("1.00")
line_total = decimal("9.99") * 3
share = total / decimal("2")
```

Supported:

- `decimal + decimal`
- `decimal - decimal`
- `decimal * decimal`
- `decimal / decimal`
- `decimal * integer`
- `integer * decimal`
- `decimal / integer`

Mixed `decimal` and `float` arithmetic is a type error.

Division by zero uses the existing arithmetic error family.

### 4.6 Comparisons

Decimal supports:

```marreta
amount == decimal("19.90")
amount != decimal("0")
amount > decimal("100.00")
amount >= decimal("100.00")
amount < decimal("1000.00")
amount <= decimal("1000.00")
```

Allowed comparisons:

- decimal vs decimal
- decimal vs integer, by exact integer-to-decimal coercion

Decimal vs float comparison is a type error.

### 4.7 Methods

Decimal supports the basic expected numeric methods in the first cut:

```marreta
amount.round(places: 2)
amount.floor()
amount.ceil()
amount.abs()
amount.trunc()
amount.scale()
amount.to_string()
amount.to_integer()
amount.to_float()
```

Semantics:

| Method | Behavior |
|---|---|
| `round(places: n)` | rounds to `n` decimal places using Half Even / banker's rounding |
| `floor()` | largest integer decimal <= value |
| `ceil()` | smallest integer decimal >= value |
| `abs()` | absolute value |
| `trunc()` | removes fractional digits toward zero |
| `scale()` | returns the number of fractional digits |
| `to_string()` | returns canonical decimal string |
| `to_integer()` | truncates toward zero and returns integer |
| `to_float()` | returns float, explicitly lossy |

`to_integer()` truncates instead of failing:

```marreta
decimal("19.90").to_integer()   # 19
decimal("-19.90").to_integer()  # -19
decimal("19.00").to_integer()   # 19
```

If the developer wants rounding before integer conversion, they must say so:

```marreta
rounded = decimal("19.90").round(places: 0).to_integer()  # 20
floored = decimal("19.90").floor().to_integer()           # 19
ceiled = decimal("19.10").ceil().to_integer()             # 20
```

`to_float()` is allowed because API/application code sometimes needs explicit
interop with existing float calculations, but it must remain visibly lossy in
the code.

### 4.8 Persistence and Transport

Enum fields:

- Persistent enum fields are stored as text columns.
- PostgreSQL maps `enum ["a", "b"]` fields to `TEXT`, not native PostgreSQL
  `ENUM`.
- Marreta validation enforces allowed values at schema boundaries.
- No generated `CHECK (...)` constraint in the first cut.

Rationale:

- Enum is a schema constraint, not a nominal DB type.
- `TEXT` keeps migrations simple and provider-portable.
- Native PostgreSQL `ENUM` and `CHECK` constraints can be considered later if
  real migration/versioning pressure justifies database-level enforcement.

Relational DB:

- PostgreSQL maps `decimal` fields to `NUMERIC`.
- DB rows load `NUMERIC` back as `Value::Decimal`.

Document DB:

- MongoDB stores decimals as BSON Decimal128.
- Decimal128 is required in the first cut so MongoDB aggregation and comparison
  operators such as `$sum`, `$gt`, and `$lt` preserve numeric semantics.
- Loaded Decimal128 values map back to `Value::Decimal`.

Cache / Queue / HTTP:

- Decimal values serialize as JSON strings.
- Schema validation can coerce string decimals back into `Value::Decimal`.

---

## 5. Schema Interaction

The following surfaces must understand `enum` and `decimal` schema fields:

- `take payload as Schema`
- `reply CODE as Schema`
- `SchemaName { ... }` constructors
- task parameters `param as Schema`
- `queue.push/publish ... as Schema`
- `on queue/topic ... take msg as Schema`
- `http_client.*(...) as Schema`
- scenario test in-memory execution and mocks
- DB migration generation for persistent schemas
- OpenAPI generation
- `marreta doctor`

Constructor strictness from spec 041 applies:

```marreta
charge = Charge {
    amount: decimal("19.90"),
    currency: "BRL",
    status: "paid"
}
```

Invalid enum value:

```marreta
charge = Charge {
    amount: decimal("19.90"),
    currency: "BRL",
    status: "payed"
}
```

Expected result: constructor validation error.

---

## 6. OpenAPI

Enum fields should emit inline enum constraints:

```yaml
status:
  type: string
  enum:
    - pending
    - paid
    - cancelled
```

Decimal fields should emit string decimal format:

```yaml
amount:
  type: string
  format: decimal
```

Rationale: OpenAPI `number` would encourage JSON clients to treat decimal
values as binary floating-point numbers.

---

## 7. Errors

Enum validation errors should reuse schema validation semantics:

```json
{
  "error": "field 'status' must be one of: pending, paid, cancelled"
}
```

Decimal parse/coercion errors should be explicit:

```json
{
  "error": "field 'amount' must be a decimal string"
}
```

Invalid `decimal(...)` calls:

- wrong arity -> `wrong_arity`
- non-string argument -> `type_error`
- invalid decimal string -> runtime/type error with clear message

Implementation should preserve existing operation labels where applicable:

- `schema constructor Charge`
- `http_client.post`
- `queue.push`
- `db.items.save`

---

## 8. Non-Goals

- No `money` type.
- No currency validation.
- No global decimal scale configuration.
- No decimal literal suffix such as `19.90d`.
- No implicit float-to-decimal coercion.
- No arbitrary precision `BigDecimal` promise in the first cut.
- No nominal/exportable enum declarations.
- No `set` type.

---

## 9. Implementation Plan

### Phase 1 — AST and Parser

- Add `SchemaType::Enum(Vec<String>)`.
- Add `SchemaType::Decimal`.
- Parse inline enum field declarations:

  ```marreta
  status: enum ["pending", "paid"]
  ```

- Parse decimal schema fields:

  ```marreta
  amount: decimal
  ```

- Add `decimal("...")` as a built-in function or namespace-free constructor.

### Phase 2 — Value and Runtime Semantics

- Add `Value::Decimal(rust_decimal::Decimal)`.
- Add JSON serialization as string.
- Add display/type-name support.
- Add arithmetic and comparison support.
- Add decimal methods:
  - `round`
  - `floor`
  - `ceil`
  - `abs`
  - `trunc`
  - `scale`
  - `to_string`
  - `to_integer`
  - `to_float`

### Phase 3 — Schema Validation

- Extend schema validator for enum fields.
- Extend schema validator for decimal fields.
- Reject float-to-decimal coercion.
- Accept string decimal and integer inputs.
- Preserve nested/list validation paths.

### Phase 4 — Persistence and OpenAPI

- Map persistent enum fields to PostgreSQL `TEXT`.
- Do not emit native PostgreSQL `ENUM` types or enum `CHECK` constraints in
  the first cut.
- Map persistent decimal fields to PostgreSQL `NUMERIC`.
- Load NUMERIC as decimal where schema metadata is available.
- Emit OpenAPI enum constraints.
- Emit decimal fields as `type: string`, `format: decimal`.

### Phase 5 — Cross-Surface Coverage

Validate enum and decimal across:

- route payloads
- response schemas
- schema constructors
- task contracts
- queue producer/consumer schemas
- HTTP client response schemas
- DB persistence
- doc/cache/queue JSON transport
- scenario tests

---

## 10. Test Plan

### Unit Tests

- Parser accepts enum schema fields.
- Parser rejects invalid enum declarations.
- Parser accepts decimal schema fields.
- `decimal("19.90")` creates `Value::Decimal`.
- Decimal rejects non-string constructor args.
- Decimal arithmetic and comparisons work.
- Decimal methods match documented behavior.
- Decimal + float is rejected.
- Enum validation accepts valid values.
- Enum validation rejects invalid values.

### Integration Tests

- `take payload as Schema` validates enum and decimal.
- `reply as Schema` serializes decimal as string.
- Constructor validates enum/decimal fields.
- Task parameter schema validates enum/decimal fields.
- Persistent enum schema migration emits `TEXT`.
- Persistent enum schema migration does not emit PostgreSQL `ENUM` or
  `CHECK (...)`.
- Persistent schema migration emits `NUMERIC`.
- DB round-trip preserves decimal value.
- DB round-trip preserves enum value as string.
- OpenAPI emits enum and decimal metadata.

### Functional Tests

Add examples to `examples/functional_tests` proving:

- POST payload with decimal string and enum status.
- Constructor-created charge object.
- Decimal total calculation.
- Decimal methods.
- DB migration diff/generate/apply for a persistent enum field.
- DB save/find of decimal field.
- DB save/find of enum field.
- Queue publish with decimal payload schema.
- HTTP client response schema containing decimal and enum.
- Scenario tests with mocked dependencies.

---

## 11. Closed Design Decisions

1. **Decimal division scale:** use the natural precision/scale limit of
   `rust_decimal` (up to 28 decimal digits). Do not add a separate Marreta
   scale policy in this spec.
2. **Scientific notation:** reject strings such as `"1e3"`. Decimal contracts
   use plain base-10 notation.
3. **MongoDB representation:** use BSON Decimal128 in the first cut, not
   strings, so document queries and aggregations preserve numeric semantics.
4. **Rounding mode:** `round(places:)` uses Half Even / banker's rounding.
