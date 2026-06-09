# 033 — Request Logging

> Status: Delivered
> Type: Runtime observability
> Scope: Automatic HTTP request/access logging for `marreta serve`

---

## 1. Purpose

This spec introduces native request/access logging for MarretaLang runtime
servers.

The purpose is not to expand the application-level `log` namespace or to create
an in-language observability framework.

The purpose is narrower:

- emit one structured runtime log event per handled HTTP request
- provide consistent request/response visibility without user code boilerplate
- expose common operational fields such as method, path, status, and latency

This is a runtime/server concern, not business logic.

---

## 2. Why Request Logging Matters

MarretaLang is centered on API and endpoint execution.

That means operators almost always need a baseline access log stream showing:

- which route was hit
- with which HTTP method
- whether it succeeded or failed
- how long it took

Without native request logging, every application must reinvent access logging
manually with `log.*`, which is the wrong abstraction boundary and creates
inconsistent output across projects.

Application logs and request logs solve different problems:

- `log.*` is for domain/application events
- request logging is for runtime HTTP observability

---

## 3. Design Principles

Native request logging must follow these rules:

1. It should be runtime-controlled, not application-controlled.
2. It should emit structured logs only.
3. It should be consistent across every route automatically.
4. It should stay small in scope in the first cut.
5. It should avoid logging sensitive request data by default.
6. It should coexist with `log.*`, not merge into it.

---

## 4. Delivered Runtime Contract

The first cut should introduce automatic access logging in `marreta serve`.

Logging should be controlled through environment configuration:

```env
MARRETA_REQUEST_LOG=true
```

The first cut should support:

- `MARRETA_REQUEST_LOG=true`
- `MARRETA_REQUEST_LOG=false`

The first cut should treat `MARRETA_REQUEST_LOG` as the start of a runtime
configuration family, not as an isolated one-off flag.

That means future extensions may reasonably introduce names such as:

- `MARRETA_REQUEST_LOG_FORMAT`
- `MARRETA_REQUEST_LOG_INCLUDE_QUERY`

without invalidating the boolean enable/disable contract above.

## 4.1 Command defaults

Default behavior should be scoped by command:

- `marreta serve` -> default `true`
- `marreta test` -> default `false`
- commands without an HTTP request lifecycle should not emit request logs

Examples of commands that should not emit request logs:

- `marreta doctor`
- `marreta migrate *`
- REPL-like or non-server tooling flows

This is more precise than saying "default true when unset", because the runtime
must avoid polluting test output or non-server command flows.

---

## 5. Output Destination

Request logs should be emitted by the runtime to:

- `stdout`

The first cut should use:

- JSON Lines

This keeps request logging aligned with the delivered `log` namespace and with
modern runtime/container expectations.

No file sinks, alternate formats, or in-code transport configuration should be
added in the first cut.

---

## 6. Event Shape

Each handled request should emit one JSON object on a single line.

The minimum first-cut shape should be:

- `timestamp`
- `kind`
- `method`
- `path`
- `route` when available
- `status`
- `duration_ms`

The fixed `kind` value should be:

- `"request"`

### Example

```json
{"timestamp":"2026-05-02T14:10:22Z","kind":"request","method":"GET","path":"/orders/42","route":"/orders/:id","status":200,"duration_ms":7}
```

### Another example

```json
{"timestamp":"2026-05-02T14:10:22Z","kind":"request","method":"POST","path":"/orders","status":201,"duration_ms":18}
```

---

## 7. Field Semantics

## 7.1 `timestamp`

- UTC timestamp
- same canonical timestamp policy already used by runtime log output

## 7.2 `kind`

- always `"request"` for first-cut access log events

## 7.3 `method`

- request HTTP method as an uppercase string
- examples: `GET`, `POST`, `PUT`, `PATCH`, `DELETE`

## 7.4 `path`

- concrete request path received by the server
- example: `/orders/42`

The first cut should log the normalized path only, without embedding a raw URL
string that mixes path and query.

## 7.5 `route`

- logical route pattern when the runtime can resolve it
- example: `/orders/:id`

This is useful because it separates:

- the incoming path
- the declared route shape

## 7.6 `status`

- final HTTP response status code

## 7.7 `duration_ms`

- total request handling duration in milliseconds
- measured by the runtime around the request lifecycle
- encoded as a float to preserve sub-millisecond resolution for very fast
  routes

Example:

```json
{"timestamp":"2026-05-02T14:10:22Z","kind":"request","method":"GET","path":"/health","route":"/health","status":200,"duration_ms":0.42}
```

---

## 8. Route Resolution Semantics

The `route` field is essential in the first cut because it keeps request
latency and status aggregation tied to declared route patterns instead of
exploding cardinality across raw paths.

When the runtime resolves a declared route pattern, it should emit that pattern
in `route`.

Examples:

- incoming path `/orders/42`
- declared route `/orders/:id`
- emitted field:
  - `route: "/orders/:id"`

For `404` behavior, the first cut should distinguish between:

- `404` because no declared route matched the incoming path
  - `route` absent or `null`
- `404` because a declared route handled the request and emitted a `404`
  response
  - `route` present with the resolved declared pattern

---

## 9. Separation From `log.*`

This spec deliberately does **not** extend the manual `log` namespace.

These outputs must remain conceptually separate:

### Application log

```json
{"timestamp":"2026-05-02T14:10:22Z","level":"info","data":{"event":"order.created","order_id":42}}
```

### Runtime request log

```json
{"timestamp":"2026-05-02T14:10:22Z","kind":"request","method":"POST","path":"/orders","status":201,"duration_ms":18}
```

This separation is important because:

- request logging is universal and automatic
- application logging is selective and user-authored

---

## 10. Sensitive Data Policy

The first cut should be conservative.

It should **not** automatically log:

- request body
- response body
- cookies
- authorization headers
- full header maps
- stack traces
- arbitrary environment values

This keeps the feature useful without making the runtime casually leak secrets.

---

## 11. Query String Policy

The first cut should not emit raw query strings by default.

This is a deliberate conservative cut because query parameters often contain:

- tokens
- emails
- internal search terms
- pagination cursors

If query observability is ever added later, it should be introduced explicitly
with a well-defined redaction policy, not casually concatenated into `path`.

---

## 12. Error And Failure Semantics

Request logging should still emit a request event when:

- the route returns `4xx`
- the route returns `5xx`
- a runtime error is converted into an HTTP response

The event should reflect the final emitted HTTP status and measured duration.

The first cut does not need an extra `outcome` field because:

- `status` already carries the important operational signal

---

## 13. Non-Goals

The first cut does not include:

- tracing spans
- distributed trace propagation
- request body logging
- response body logging
- full query logging
- custom sink configuration
- alternate output formats
- in-language toggles
- per-route request logging configuration
- automatic user identity resolution
- IP / user agent logging by default
- request/correlation ID in the first cut

Request/correlation ID is intentionally deferred to a future dedicated design
front, tentatively:

- `034 — Request Context & Correlation`

---

## 14. Implementation Plan

### Phase 1 — Runtime toggle

- add `MARRETA_REQUEST_LOG` runtime configuration
- define command-scoped default behavior

### Phase 2 — Emit request events

- instrument the HTTP request lifecycle in `serve`
- emit one JSON Lines event per handled request

### Phase 3 — Field contract

- include `timestamp`, `kind`, `method`, `path`, `route`, `status`,
  `duration_ms`
- guarantee stable field names
- encode `duration_ms` as float

### Phase 4 — Sensitive-data guardrails

- ensure bodies/headers/cookies are not emitted in the first cut

### Phase 5 — Examples and docs

- document the runtime env toggle
- add examples of request log output
- review `docs/vscode-marreta` if any new reserved runtime-facing examples or
  patterns deserve syntax updates

---

## 15. Test Plan

### Phase 1 tests

- env parsing for `MARRETA_REQUEST_LOG`
- command-scoped default behavior validation

### Phase 2 tests

- `GET` emits one request event
- `POST` emits one request event
- `PUT`, `PATCH`, and `DELETE` emit one request event

### Phase 3 tests

- event includes required fields
- route pattern is present when available
- duration is emitted as float milliseconds
- unresolved-path `404` emits no `route`
- declared-route `404` preserves `route`

### Phase 4 tests

- request body is not logged
- authorization headers are not logged
- query string is not emitted by default
- failure responses still emit request events

### Phase 5 tests

- functional harness validates request log emission from `stdout`
- examples cover success and failure status codes
- review `docs/vscode-marreta` for any needed highlighting/version bump tied to
  delivered examples or new reserved runtime terms

---

## 16. Recommendation

This spec should be treated as:

- automatic runtime request logging
- environment-controlled
- JSON Lines to `stdout`
- minimal and conservative by default

That keeps MarretaLang aligned with modern API/server expectations without
turning the language into an observability platform.
