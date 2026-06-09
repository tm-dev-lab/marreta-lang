# 041 — Schema Constructors and HTTP Client Schemas

> Status: Approved
> Type: Language expression / HTTP client contract completion
> Scope: Construct schema-shaped values explicitly and apply schemas to outbound HTTP responses

---

## 1. Purpose

Marreta already uses schemas at boundaries:

```marreta
route POST "/orders" take payload as OrderRequest
    reply 201 as OrderResponse, result

queue.push "orders.created" as OrderCreatedEvent, event
```

But application code often needs to build schema-shaped values before they
reach a boundary. Today, examples usually create builder tasks that return
plain maps:

```marreta
export task build_order_created_event(order, customer)
    {
        order_id: order.id,
        customer_id: customer.id,
        total: order.total
    }
```

That is valid, but it hides the contract until a later `reply as`,
`queue.push as`, or task parameter validation.

This spec introduces schema constructor expressions:

```marreta
event = OrderCreatedEvent {
    order_id: order.id,
    customer_id: customer.id,
    total: order.total
}
```

The constructed value is a normal Marreta `Map`. It carries no hidden runtime
type metadata. That keeps it transparent across existing language surfaces:

```marreta
db.order_events.save(event)
doc.order_events.save(event)
queue.push "orders.created" as OrderCreatedEvent, event
reply 201 as OrderCreatedEvent, event
```

The spec also closes the existing HTTP client schema gap by allowing response
body validation on outbound calls:

```marreta
charge = ChargeRequest {
    amount: payload.amount,
    currency: "BRL",
    customer_id: payload.customer_id
}

response = http_client.post("https://payments.example/charges", charge) as ChargeResponse
```

---

## 2. Design Principles

1. **Schemas stay structural.**
   A schema constructor returns a regular map. No class, DTO, object identity,
   or hidden schema tag is introduced.

2. **Construction is explicit.**
   The reader sees where a value becomes schema-shaped. Boundary code does not
   secretly repair arbitrary maps.

3. **Constructor semantics are stricter than response serialization.**
   `reply CODE as Schema` shapes an outgoing response. A constructor builds a
   domain value and should fail when the value is not valid.

4. **Existing boundaries keep working.**
   Because the result is a map, `db`, `doc`, `queue`, `cache`, `reply`,
   task contracts, and `http_client` receive it as ordinary data.

5. **No schema instances.**
   This spec does not add `type(value) == "User"` or runtime reflection for
   schema names.

6. **HTTP client schemas are contracts, not transport magic.**
   Request payloads should be constructed explicitly. Response schemas shape
   `response.body`; they do not alter `response.status` or `response.headers`.

7. **Ingress validation is consistent.**
   Data arriving from an upstream HTTP service is external input, just like a
   route payload. `http_client.*(...) as Schema` should therefore use the same
   validation semantics as `take payload as Schema`, not the more permissive
   `reply as` response serialization semantics.

---

## 3. Schema Constructor Expression

### 3.1 Syntax

```marreta
value = SchemaName {
    field_a: expr_a,
    field_b: expr_b
}
```

Inline usage is allowed anywhere an expression is allowed:

```marreta
queue.push "orders.created" as OrderCreatedEvent, OrderCreatedEvent {
    order_id: order.id,
    total: order.total
}
```

The schema name must resolve using the same visibility rules as existing
schema-bound surfaces:

- exported schemas are visible project-wide
- private schemas are visible only within their defining module scope
- unresolved schema names are project validation errors when statically known,
  or runtime errors if reached through dynamically loaded code paths

### 3.2 Runtime Value

The expression returns `Value::Map`.

It does not return a new `Value::SchemaInstance`.

This is intentional:

```marreta
user = User {
    name: payload.name,
    email: payload.email
}

db.users.save(user)        # receives a map
doc.users.save(user)       # receives a map
cache.set("user:1", user)  # receives a map
reply 201 as User, user    # receives a map
```

No downstream module should need to know the value was created by a schema
constructor.

### 3.3 Validation Rules

Constructor validation reuses the existing recursive schema coercion engine
used by route payloads and task contracts.

Recommended behavior:

| Case | Behavior |
|---|---|
| Required field missing | error |
| Required field present with wrong type | error |
| Optional field missing | omitted |
| Optional field present with wrong type | error |
| Nested schema field | recursively constructed/coerced |
| `list of Schema` field | each element recursively coerced |
| Extra undeclared field | error |
| Persistent schema `id` missing during construction | allowed for creation flows |

Extra fields are rejected because constructors represent intentional value
creation. If a caller wants response-style stripping, they should explicitly
shape at the boundary with `reply as` or future dedicated shaping syntax.

Persistent schemas are allowed to omit `id` in constructors. Insert flows should
not force the developer to write placeholder IDs or `id: null`; the persistence
provider remains responsible for assigning IDs in `db.*.save` paths.

Example:

```marreta
schema PaymentRequest
    amount: integer
    currency: string
    metadata?: map

payment = PaymentRequest {
    amount: payload.amount,
    currency: "BRL"
}
```

Invalid:

```marreta
payment = PaymentRequest {
    amount: payload.amount,
    currency: "BRL",
    internal_note: "not declared"
}
```

The invalid example should raise an error such as:

```text
schema constructor PaymentRequest received undeclared field 'internal_note'
```

### 3.4 Relationship To Existing `as`

This spec does not turn `as` into a general cast operator.

The following existing surfaces remain unchanged:

```marreta
route POST "/x" take payload as Payload
reply 200 as Response, body
task calculate(input as Input)
queue.push "x" as Event, event
on queue "x" take event as Event
```

Schema construction is expression-level value creation:

```marreta
event = Event {
    id: uuid.v7(),
    total: order.total
}
```

---

## 4. HTTP Client Schema Contracts

### 4.1 Outgoing Payloads

Outgoing payloads should use schema constructors before the call:

```marreta
charge = ChargeRequest {
    amount: payload.amount,
    currency: "BRL",
    customer_id: payload.customer_id
}

response = http_client.post("https://payments.example/charges", charge)
```

Inline construction is valid:

```marreta
response = http_client.post("https://payments.example/charges", ChargeRequest {
    amount: payload.amount,
    currency: "BRL",
    customer_id: payload.customer_id
})
```

This keeps the payload contract visible without adding special payload syntax
inside `http_client`.

### 4.2 Incoming Responses

`as SchemaName` after an `http_client.*` call validates/coerces `response.body`:

```marreta
response = http_client.get("https://users.example/users/#{id}") as UserProfile
```

The returned value remains the standard HTTP client envelope:

```marreta
{
    status: 200,
    body: { ... validated as UserProfile ... },
    headers: { ... }
}
```

Only `body` is validated/coerced. `status` and `headers` are never affected by
the schema.

### 4.3 Response Validation Semantics

HTTP response schema semantics should match `take payload as Schema`:

| Case | Behavior |
|---|---|
| Extra field in `response.body` | preserved, matching route payload ingress |
| Required field missing | error |
| Optional field missing | omitted |
| Type mismatch | error unless existing payload coercion accepts it |

Rationale: inbound HTTP data is external. It enters the application from the
world outside the Marreta process, just like route payloads. The language should
treat both ingress boundaries consistently. A malformed upstream response is a
contract violation and should fail before business code consumes it.

```marreta
response = http_client.get("https://users.example/users/#{id}") as UserProfile
require response.status == 200 else fail 502, "user service failed"
```

### 4.4 Superseding The Previous Outgoing `payload as Schema` Note

Spec 015 described this syntax for outgoing payloads:

```marreta
http_client.post(url, payload as OrderRequest)
```

That syntax is not implemented today. This spec prefers schema constructors as
the explicit outgoing-payload mechanism:

```marreta
http_client.post(url, OrderRequest { ... })
```

This syntax should remain unsupported. It delays validation until the HTTP
boundary and creates pressure for a general expression-level `as` cast. Schema
constructors are clearer because the value is validated on the exact line where
it is created.

The `as Schema` suffix remains appropriate for opaque inbound data, such as
`http_client.*(...) as Schema`, because the value already exists outside the
Marreta process and is being validated as it enters the application.

---

## 5. Examples

### 5.1 Reply

```marreta
schema GreetingResponse
    message: string

route GET "/greetings"
    greeting = GreetingResponse {
        message: "Hello, Marreta!"
    }

    reply 200 as GreetingResponse, greeting
```

### 5.2 DB

```marreta
schema User
    db:
    id: integer
    name: string
    email: string

route POST "/users" take payload
    user = User {
        name: payload.name,
        email: payload.email
    }

    saved = db.users.save(user)
    reply 201 as User, saved
```

### 5.3 Doc

```marreta
schema AuditEvent
    event_id: string
    kind: string
    payload: map

event = AuditEvent {
    event_id: uuid.v7(),
    kind: "user.created",
    payload: user
}

doc.audit_events.save(event)
```

### 5.4 Queue

```marreta
schema UserCreatedEvent
    event_id: string
    user_id: integer
    email: string

event = UserCreatedEvent {
    event_id: uuid.v7(),
    user_id: saved.id,
    email: saved.email
}

queue.push "users.created" as UserCreatedEvent, event
```

### 5.5 HTTP Client

```marreta
schema ChargeRequest
    amount: integer
    currency: string
    customer_id: string

schema ChargeResponse
    id: string
    status: string

route POST "/payments/charge" take payload
    charge = ChargeRequest {
        amount: payload.amount,
        currency: "BRL",
        customer_id: payload.customer_id
    }

    response = http_client.post("https://payments.example/charges", charge) as ChargeResponse
    require response.status == 201 else fail 502, "payment service failed"

    reply 201 as ChargeResponse, response.body
```

---

## 6. Parser And AST

### 6.1 Parser

Add a schema constructor expression form:

```text
IDENT "{" field_list "}"
```

This should parse only in expression contexts. The parser must disambiguate it
from a regular map literal:

```marreta
{ name: "Ana" }      # map literal
User { name: "Ana" } # schema constructor
```

The `IDENT` is not a new keyword. It is resolved later against the visible
schema registry.

### 6.2 AST

Add an expression variant similar to:

```rust
Expression::SchemaConstructor {
    schema_name: String,
    fields: Vec<(String, Expression)>,
}
```

No new runtime `Value` variant should be added.

### 6.3 Visibility

Schema constructor resolution should use the same module-aware schema registry
already used for:

- `take payload as Schema`
- task contracts
- `reply CODE as Schema`
- queue producer/consumer schema bindings

---

## 7. Interpreter

Evaluation steps:

1. Resolve `schema_name` against the visible schema registry.
2. Evaluate each field expression left-to-right.
3. Build a temporary map from evaluated fields.
4. Validate/coerce recursively against the schema.
5. Reject undeclared fields.
6. Return the resulting `Value::Map`.

Errors should use existing Marreta error identity and avoid Rust internals.

Example errors:

```text
schema constructor User missing required field 'email'
schema constructor User field 'id' expected integer, got string
schema constructor User received undeclared field 'internal_note'
unknown schema 'UserProfile'
```

---

## 8. HTTP Client Implementation

### 8.1 Response Schema Annotation

The parser should support:

```marreta
http_client.get(url) as SchemaName
http_client.post(url, payload) as SchemaName
```

The annotation applies to the HTTP client response body only.

Implementation options:

1. Add a generic expression annotation for `Expression as SchemaName` but only
   allow it initially when the expression is an `http_client.*` method call.
2. Add a specific `http_client` response schema annotation in the existing
   method-call parsing path.

Current recommendation: use the smallest implementation that does not create a
general cast operator. A general `expr as Schema` surface should require a
separate spec.

### 8.2 Dispatch

After receiving `HttpClientResponse`, validate/coerce the body through the same
ingress behavior used by `take payload as Schema`.

Pseudo-flow:

```text
response = driver.execute(request)
if response_schema:
    response.body = validate_ingress_body(response.body, response_schema)
return { status, body, headers }
```

Schema failures at this boundary should use the same error family as route
payload validation where possible, while preserving the HTTP client operation in
the error context:

```text
error.op = "http_client.get"
```

This keeps the error taxonomically aligned with schema validation while making
the external integration boundary obvious in logs and stack traces.

---

## 9. Non-Goals

- No runtime schema instance type.
- No classes, constructors with methods, or object identity.
- No general `expr as Schema` cast operator.
- No automatic schema inference from map literals.
- No automatic DB/doc persistence behavior from constructed values.
- No generated migrations or schema declarations.
- No HTTP request body schema annotation syntax beyond explicit constructors.

---

## 10. Test Plan

### Phase 1 — Parser

- Parses `User { name: "Ana" }`.
- Parses nested constructors.
- Parses constructors inside function arguments.
- Parses `http_client.get(url) as UserProfile`.
- Rejects invalid constructor syntax with clear parser errors.

### Phase 2 — Interpreter Constructor Semantics

- Constructor returns `Value::Map`.
- Required missing field errors.
- Wrong type errors.
- Extra undeclared field errors.
- Optional missing field succeeds.
- Nested schema succeeds.
- `list of Schema` succeeds.
- Private schema visibility is respected.
- Exported schema visibility works across files.

### Phase 3 — Existing Boundary Compatibility

Constructed schema values must be exercised in all major schema-aware or
schema-adjacent surfaces:

- `reply CODE as Schema, constructed`
- task parameter contract: `task f(input as Schema)` called with constructed
  value
- `db.table.save(constructed)` with a persistent schema
- `doc.collection.save(constructed)`
- `queue.push "name" as Schema, constructed`
- `queue.publish "name" as Schema, constructed`
- `cache.set(key, constructed)` and `cache.get(key)` as transport roundtrip

The purpose is to prove the constructor result is transparent map data, not a
new runtime type that every module must understand.

### Phase 4 — HTTP Client Response Schemas

- `http_client.get(url) as Schema` validates/coerces `response.body`.
- Extra upstream fields are preserved, matching route payload ingress.
- Missing required upstream fields fail.
- Wrong upstream field types fail unless existing payload coercion accepts them.
- `response.status` remains unchanged.
- `response.headers` remains unchanged.
- `http_client.post(url, ConstructedRequest { ... }) as ResponseSchema`
  works end-to-end.

### Phase 5 — Functional Examples

Update `examples/functional_tests` with routes and scenario tests covering:

- schema constructor to reply
- schema constructor to DB
- schema constructor to doc
- schema constructor to queue
- schema constructor to cache roundtrip
- schema constructor as HTTP client request payload
- HTTP client response validation with `as ResponseSchema`

---

## 11. Approved Decisions

1. Schema constructors reject extra fields.
2. Schema constructors return ordinary maps, not schema instances.
3. Persistent schema constructors may omit `id` for creation flows.
4. `http_client.*(...) as Schema` uses ingress validation semantics aligned
   with `take payload as Schema`.
5. `http_client.*(...) as Schema` schema failures use the same error family as
   route payload validation where possible and include
   `error.op = "http_client.{verb}"`.
6. `http_client.post(url, payload as Schema)` remains unsupported; outbound
   payload contracts should use `SchemaName { ... }` constructors.
