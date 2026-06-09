# 046 — OpenAPI Docs Refinement

> Status: Delivered
> Type: Runtime documentation / OpenAPI generation
> Scope: Make `/docs` and `/openapi.json` trustworthy enough to describe Marreta applications as API contracts, not just route catalogs

---

## 1. Purpose

Marreta already serves:

```text
GET /docs
GET /openapi.json
```

The current implementation is alive and useful as a route inventory, but a
validation pass against `examples/functional_tests` exposed gaps that make the
generated OpenAPI incomplete or misleading for real API consumers.

This spec refines the OpenAPI generator so that:

- every generated `$ref` resolves;
- every route that accepts a body exposes a request body;
- auth routes document auth failure responses;
- dynamic or unknown response shapes do not lie as precise contracts;
- recent language types (`enum`, `decimal`, temporal types) remain correctly
  represented;
- the Swagger UI remains a thin viewer over a valid `/openapi.json`.

This is not a documentation-comment system. It is a correctness pass over the
contract Marreta can infer from source.

---

## 2. Validation Baseline

The validation used `examples/functional_tests` running via Compose:

```bash
cd examples/functional_tests
docker compose up -d --build --wait
curl -fsS http://127.0.0.1:3737/docs
curl -fsS http://127.0.0.1:3737/openapi.json
docker compose down
```

Observed baseline:

```text
/docs status: 200 text/html
/openapi.json status: 200 application/json
OpenAPI version: 3.0.3
paths: 343
operations: 351
schemas: 20
tags: 17
x-marreta-consumers: 17
source routes: 351
missing operations: 0
extra operations: 0
```

Positive findings:

- `/docs` returns Swagger UI HTML.
- `/openapi.json` is valid JSON.
- Every source route appears in OpenAPI.
- Path params convert from `:id` to `{id}`.
- Auth providers appear in `components.securitySchemes`.
- `decimal` maps to `type: string, format: decimal`.
- `enum [...]` maps to `type: string, enum: [...]`.
- `reply html` and `reply text` map to `text/html` and `text/plain`.

Critical findings:

```text
missing_schema_refs: 1
routes_with_payload_or_raw: 96
schema_bound_payload_routes: 22
missing_requestBody_for_payload_or_raw: 74
generic_json_object_response_entries: 327
operations_without_operationId: 351
operations_without_summary: 351
operations_without_description: 351
```

---

## 3. Design Principles

1. **Never emit invalid OpenAPI.**
   A broken `$ref` is worse than a generic schema. If Marreta cannot produce a
   precise schema, it should emit a valid fallback.

2. **Do not over-promise.**
   If the runtime status or body shape is dynamic, the OpenAPI must not claim a
   precise static contract that Marreta cannot prove.

3. **Prefer source-level inference over comments.**
   The first refinement should extract what the language already knows:
   route bindings, schemas, auth requirements, reply content types, status
   literals, and error guards.

4. **Swagger UI is a viewer, `/openapi.json` is the contract.**
   Offline Swagger UI support is not part of this spec. `/openapi.json` must
   remain useful even when CDN assets are unavailable.

5. **No runtime execution.**
   OpenAPI generation must not execute route code, tasks, DB queries, cache
   calls, or queue handlers.

---

## 4. Problems And Corrections

### 4.1 Broken `$ref` For Private Schemas

Observed:

```text
/paths//iteration/private/schema/post/requestBody/content/application/json/schema
  #/components/schemas/PrivateIterationPayload
```

But `components.schemas.PrivateIterationPayload` is missing.

Cause:

- The route uses a schema declared outside the exported/global schema set.
- The OpenAPI generator emits `$ref` to the route schema name.
- The components builder only includes schemas visible in `registry.schemas`.

Correction:

- Build a `ReferencedSchemas` set while generating operations.
- Include every schema referenced by public HTTP routes in
  `components.schemas`, even when that schema is file-private.
- This applies to:
  - route request schema: `route POST ... take payload as Schema`;
  - route response schema: `reply CODE as Schema, ...`;
  - nested schema references inside any included schema.

Rationale:

- `export` controls code visibility inside a Marreta project.
- A public HTTP route controls an external I/O contract.
- If a public route uses a private schema, that shape has already become part
  of the API contract.
- OpenAPI requires the reference graph to be closed and resolvable; broken
  `$ref` entries break code generators and validators.

Implementation sketch:

```rust
let mut referenced_schemas = BTreeSet::new();

for route in &registry.routes {
    collect_route_schema_refs(route, &mut referenced_schemas);
}

for schema_name in referenced_schemas {
    let schema = resolve_schema_for_docs(&schema_name, registry)?;
    emit_component_schema(schema_name, schema);
    collect_nested_schema_refs(schema, &mut referenced_schemas);
}
```

Resolution model:

- Prefer exact schema used by the route's source module when available.
- Fall back to exported/global schema registry.
- If a schema cannot be resolved, do not emit a broken `$ref`; emit a valid
  fallback and attach a Marreta extension:

```json
{
  "type": "object",
  "x-marreta-unresolved-schema": "PrivateIterationPayload"
}
```

Success criteria:

- `missing_schema_refs == 0` for `examples/functional_tests`.
- `/iteration/private/schema` has a resolvable request body schema.

---

### 4.2 Missing Request Body For `take payload` And `take raw`

Observed:

```text
routes_with_payload_or_raw: 96
schema_bound_payload_routes: 22
missing_requestBody_for_payload_or_raw: 74
```

Cause:

- The generator only emits `requestBody` when the route has
  `take payload as Schema`.
- Plain `take payload` and `take raw` are runtime body bindings but are absent
  from OpenAPI.

Correction:

Emit `requestBody` for every route with body binding:

| Marreta binding | OpenAPI requestBody |
| --- | --- |
| `take payload as Schema` | `$ref` to schema |
| `take payload` | `application/json` with `{ "type": "object", "additionalProperties": true }` |
| `take raw` | `text/plain` with `{ "type": "string" }` by default |

Rationale:

- Plain `take payload` maps to a Marreta map/dictionary. A free-form JSON
  object is the most precise OpenAPI representation for that contract.
- Omitting `type` would be too broad because it would imply arrays, strings, or
  primitive JSON values are equally expected.
- `take raw` intentionally bypasses JSON parsing, so documenting it as
  `text/plain` is more honest than documenting it as JSON.

For `take raw`, if Marreta later supports declaring raw content type, use that
declared content type. Until then, `text/plain` is safer than pretending JSON.

Implementation sketch:

```rust
match route_body_binding(route) {
    BodyBinding::PayloadWithSchema(name) => request_body_ref(name),
    BodyBinding::PayloadAny => request_body_json_object(),
    BodyBinding::Raw => request_body_text(),
    BodyBinding::None => None,
}
```

Success criteria:

- No route with `take payload` or `take raw` is missing `requestBody`.
- Schema-bound payload routes keep current `$ref` behavior.
- Non-body routes still do not emit request bodies.

---

### 4.3 Auth Routes Missing `401` And `403`

Observed runtime:

```text
GET /auth/me without key -> 401
GET /auth/forbidden with valid key -> 403
```

Observed OpenAPI:

```json
{
  "responses": {
    "200": { "...": "..." }
  },
  "security": [{ "internal_auth": [] }]
}
```

Cause:

- Security schemes are generated.
- Operation-level `security` is generated.
- Failure responses from auth runtime and `allow` rules are not generated.

Correction:

- If a route has `require auth provider`, always add response `401`.
- If a route has at least one `allow` expression, add response `403`.
- These responses should use the runtime error envelope shape if Marreta has a
  stable one. If not, start with description-only responses.

Recommended first cut:

```json
"401": { "description": "Unauthorized" },
"403": { "description": "Forbidden" }
```

Optional refinement:

```json
"401": {
  "description": "Unauthorized",
  "content": {
    "application/json": {
      "schema": { "$ref": "#/components/schemas/MarretaError" }
    }
  }
}
```

Success criteria:

- All routes with `require auth` document `401`.
- All routes with `allow` document `403`.
- Existing security schemes remain unchanged.

---

### 4.4 Dynamic Status Codes Are Misdocumented As `200`

Observed:

```text
GET /response/dynamic_status -> runtime 202
OpenAPI documents only 200
```

Cause:

- `collect_responses` only extracts literal integer status codes from
  `reply`.
- If status code is an expression, it falls back to `200`.

Correction:

- If `reply` status code is a literal integer, keep current behavior.
- If status code is not a literal integer, do not emit fake `200`.
- Emit `default` response with a Marreta extension:

```json
"default": {
  "description": "Dynamic response status",
  "content": {
    "application/json": {
      "schema": { "type": "object", "additionalProperties": true }
    }
  },
  "x-marreta-dynamic-status": true
}
```

If a route has both literal and dynamic replies, emit both:

- literal responses for known statuses;
- `default` for dynamic status paths.

Rationale:

- `default` is the OpenAPI 3.x mechanism for responses whose HTTP status cannot
  be statically enumerated.
- `x-marreta-dynamic-status` preserves language-specific metadata without
  violating the OpenAPI schema.

Success criteria:

- `/response/dynamic_status` no longer claims only `200`.
- Routes with literal statuses keep precise status codes.

---

### 4.5 Error Paths From `raise`, `rescue fail`, And Nested Guards

Observed runtime:

```text
GET /errors/raise_uncaught -> 500
GET /errors/rescue_pipeline -> 503
POST /errors/guard with invalid body -> 400
```

Observed OpenAPI:

- `raise_uncaught` documents only generic `200`.
- `rescue_pipeline` documents only generic `200`.
- `errors/guard` documents `400` and `422`, which is correct for direct
  `require ... else fail`.

Cause:

- `collect_error_codes` only scans top-level `Fail`, `Require`, `Reject`, and
  `Transaction`.
- It does not inspect:
  - `raise`;
  - `rescue fail` inside expressions or pipelines;
  - nested task bodies declared inside routes;
  - `if` branches that contain `reply` or failure paths.

Correction:

Introduce response collection over statement expressions, not only top-level
statements.

First-cut rules:

- Direct `raise` in route body adds `500`.
- `require ... else raise` adds `500`.
- `pipeline rescue fail CODE, ...` adds `CODE`.
- `fail CODE, ...` inside reachable route block adds `CODE`.
- `reply CODE, ...` inside `if` branches adds `CODE`.

Do not attempt full control-flow proof. This is documentation inference, not
static verification. If a status can happen, it may be documented.

Implementation sketch:

```rust
fn collect_route_outcomes(body: &[Statement]) -> RouteOutcomes {
    let mut outcomes = RouteOutcomes::default();
    walk_statements_and_expressions(body, &mut |node| {
        match node {
            Reply(literal_code, schema, content_type) => outcomes.responses.push(...),
            Reply(dynamic_code, schema, content_type) => outcomes.dynamic_response = true,
            Fail(code) => outcomes.errors.insert(code),
            Raise(_) => outcomes.errors.insert(500),
            RescueFail(code) => outcomes.errors.insert(code),
            RequireElseRaise => outcomes.errors.insert(500),
            _ => {}
        }
    });
    outcomes
}
```

Success criteria:

- `/errors/raise_uncaught` documents `500`.
- `/errors/rescue_pipeline` documents `503`.
- Existing direct guard errors remain documented.

---

### 4.6 Generic Response Shapes

Observed:

```text
generic_json_object_response_entries: 327
```

Example:

- `/response/variable/list` returns a list at runtime.
- OpenAPI says response schema is `{ "type": "object" }`.

Cause:

- For `reply` without `as Schema`, OpenAPI always falls back to object.
- The generator does not infer obvious literal response types.

Correction:

Add shallow expression-to-OpenAPI inference for reply bodies:

| Marreta expression | OpenAPI schema |
| --- | --- |
| string literal | `{ "type": "string" }` |
| integer literal | `{ "type": "integer", "format": "int64" }` |
| float literal | `{ "type": "number", "format": "float" }` |
| decimal literal/value | `{ "type": "string", "format": "decimal" }` |
| boolean literal | `{ "type": "boolean" }` |
| null | `{ "nullable": true }` or `{ "type": "object", "nullable": true }` |
| list literal | `{ "type": "array", "items": inferred-or-empty }` |
| map literal | `{ "type": "object", "properties": ... }` when keys are static |
| identifier/expression/call | generic fallback |

Generic fallback should be:

```json
{
  "type": "object",
  "additionalProperties": true
}
```

instead of bare `{ "type": "object" }`.

This keeps contracts honest: precise when obvious, permissive when dynamic.

Rationale:

- Literal AST nodes are deterministic and require no runtime evaluation.
- Inferring a string, number, boolean, null, list literal, or static map literal
  is safe because it does not require data-flow analysis.
- The generator must stop at dynamic expressions, identifiers, calls, pipelines,
  DB/cache/doc/queue operations, and any construct that would require execution
  or flow-sensitive inference.

Success criteria:

- Literal scalar/list/map routes produce better schemas.
- Dynamic expressions still produce valid generic schemas.
- No route execution is required.

---

### 4.7 Query And Header Bindings

Observed:

- `take query` becomes one query parameter named `query` of type object.
- `take headers` is not represented.

Correction:

Keep query/header support conservative in this spec:

- `take query` should remain a single generic query object unless Marreta gains
  typed query schemas.
- Add clearer OpenAPI metadata:

```json
{
  "name": "query",
  "in": "query",
  "required": false,
  "style": "deepObject",
  "explode": true,
  "schema": {
    "type": "object",
    "additionalProperties": { "type": "string" }
  },
  "description": "All query string parameters bound as a Marreta map"
}
```

- `take headers` should not enumerate arbitrary headers.
- Auth headers are already covered by `securitySchemes`.
- For generic header binding, add an operation extension instead of fake
  parameters:

```json
"x-marreta-bindings": {
  "headers": true
}
```

Success criteria:

- Query binding is clearer in Swagger/OpenAPI.
- Header binding is discoverable without inventing fake required headers.

---

### 4.8 Operation Metadata

Observed:

```text
operations_without_operationId: 351
operations_without_summary: 351
operations_without_description: 351
```

Correction:

Add deterministic `operationId` for every operation.

Recommended format:

```text
<method>_<normalized_path>
```

Examples:

```text
get_auth_me
post_contracts_api_types
put_db_items_by_id
```

Rules:

- Lowercase.
- Replace path separators with `_`.
- Replace `{id}` with `by_id`.
- Strip duplicate underscores.
- If collision occurs, suffix `_2`, `_3`, etc.

Do not add summary/description inference in this spec. Comment-driven docs
should be a separate decision because it creates a documentation syntax
contract.

Success criteria:

- `operations_without_operationId == 0`.
- No operationId collisions.

---

### 4.9 Swagger UI CDN Dependency

Observed:

`/docs` loads Swagger UI assets from:

```text
https://unpkg.com/swagger-ui-dist/swagger-ui.css
https://unpkg.com/swagger-ui-dist/swagger-ui-bundle.js
```

Correction:

No runtime change in this spec.

Rationale:

- `/openapi.json` is the contract.
- `/docs` is a convenience viewer.
- Bundling Swagger UI would increase binary size and introduce asset lifecycle
  concerns.

Document the limitation in runtime docs or README:

```text
/docs requires internet access for Swagger UI CDN assets.
/openapi.json is always served locally.
```

Future option:

- `MARRETA_DOCS_MODE=cdn|embedded|off`, but not in this spec.

---

## 5. Proposed Implementation Plan

### Phase 1 — Safety First

- Add OpenAPI validation tests for broken `$ref`.
- Include route-referenced private schemas in components.
- Add requestBody for plain `take payload` and `take raw`.

This phase fixes the most user-visible correctness problems.

### Phase 2 — Runtime Failure Responses

- Add `401` for auth routes.
- Add `403` for routes with `allow`.
- Add `500` for direct `raise` and `require ... else raise`.
- Add error statuses from expression-level `rescue fail`.

### Phase 3 — Honest Dynamic Responses

- Replace fake `200` fallback for dynamic reply status with `default`.
- Add `x-marreta-dynamic-status`.
- Add shallow reply body schema inference.

### Phase 4 — Tooling Ergonomics

- Add deterministic `operationId`.
- Improve query binding metadata.
- Add `x-marreta-bindings.headers` for `take headers`.

---

## 6. Tests

Unit tests in `src/openapi.rs`:

- Private schema referenced by route appears in `components.schemas`.
- All `$ref` targets resolve.
- `take payload` without schema emits generic JSON requestBody.
- `take raw` emits text requestBody.
- `require auth` emits `401`.
- `allow` emits `403`.
- Direct `raise` emits `500`.
- `rescue fail 503` emits `503`.
- Dynamic reply status emits `default`, not fake `200`.
- Literal response body inference maps list/map/scalars correctly.
- `operationId` is deterministic and collision-safe.
- Query binding includes `style: deepObject`.
- Header binding emits `x-marreta-bindings.headers`.

Functional validation against `examples/functional_tests`:

```bash
cd examples/functional_tests
docker compose up -d --build --wait
curl -fsS http://127.0.0.1:3737/openapi.json -o /tmp/openapi.json
docker compose down
```

Assertions:

- `/openapi.json` parses as JSON.
- `openapi == "3.0.3"`.
- source route count equals OpenAPI operation count.
- all `$ref` targets resolve.
- every route with `take payload` or `take raw` has requestBody.
- routes with auth have `401`.
- routes with allow have `403`.
- known dynamic/error routes match expected documented statuses:
  - `/response/dynamic_status` has `default`;
  - `/errors/raise_uncaught` has `500`;
  - `/errors/rescue_pipeline` has `503`;
  - `/auth/me` has `401`;
  - `/auth/forbidden` has `403`.

---

## 7. Non-Goals

- Generating prose summaries from comments.
- Adding a formal doc-comment syntax.
- Bundling Swagger UI assets into the binary.
- Full control-flow analysis.
- Executing routes or tasks to infer response bodies.
- Typed query parameter schemas.
- Full JSON Schema generation for arbitrary dynamic expressions.

---

## 8. Success Criteria

The spec is complete when:

- `/docs` still serves Swagger UI.
- `/openapi.json` remains valid OpenAPI 3.0.3 JSON.
- `examples/functional_tests` has zero broken schema references.
- all body-taking routes are represented with requestBody.
- auth/allow routes document expected runtime failures.
- dynamic status routes no longer lie as static `200`.
- OpenAPI generation remains startup-only and does not execute application code.
