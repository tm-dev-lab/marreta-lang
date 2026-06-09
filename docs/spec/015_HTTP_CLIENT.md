# Implementation Plan — v0.10 HTTP Client Module

**Status:** ✅ Complete — all 5 phases implemented for v0.10.0.

## Overview

v0.10 introduces outbound HTTP client capabilities to MarretaLang. Routes and tasks need to call external APIs — payment gateways, microservices, webhooks, third-party services. The roadmap entry in `SPEC.md` reads:

> v0.10 — HTTP Client | Outbound HTTP calls (`http_client.get`, `http_client.post`, `http_client.put`, `http_client.delete`, `http_client.patch`) — compose external APIs from within a route

This plan defines the complete syntax, architecture, and phased implementation — following the same format as `013_QUEUE.md` and `014_CACHE.md`.

---

## Syntax Reference

### Two Styles — Pipeline and Direct

`http_client.*` supports two composable styles — consistent with `db.*`. Both are always available; choose based on how much control you need over the response.

**Pipeline style** — data flows in via `>>`, the response body flows out via `.body` into the next operation. Tasks encapsulate the status guard, keeping the caller's pipeline clean:

```marreta
# Fetch user from upstream → save to local DB
task fetch_user(id)
    response = http_client.get("https://users.service/users/#{id}") as user_schema
    require response.status == 200 else fail 502, "user service failed"
    response.body

route POST "/users/sync/:id"
    fetch_user(params.id) >> db.users.save >> reply 201

# Pipe payload into an upstream POST → guard → pipe body to queue
route POST "/orders" take payload as order_payload
    response = payload >> http_client.post("https://orders.service/orders",
        headers: { "Authorization": "Bearer #{env.API_KEY}" }) as order_result
    require response.status == 201 else fail 502, "order service failed"
    response.body >> queue.push("invoices") >> db.orders.save
    reply 201, response.body

# Read-through: cache → external API → cache → pipeline
task get_product(id)
    cached = cache.get("product:#{id}")
    require not cached else cached
    response = http_client.get("https://catalog.service/products/#{id}") as product_schema
    require response.status == 200 else fail 502, "catalog unavailable"
    response.body >> cache.set("product:#{id}", ttl: 300)
```

**Direct style** — full access to the response envelope when you need to branch on status codes or inspect headers:

```marreta
response = http_client.get("https://api.example.com/users/#{id}")
label = match response.status
    200      -> "found"
    404      -> "not found"
    fallback -> fail 502, "upstream error: #{response.status}"

request_id = response.headers["x-request-id"]
```

### Response Model

Every `http_client.*` call returns a `Map` with three fields:

```marreta
response.status     # integer — 200, 201, 404, 500, etc.
response.body       # parsed JSON (map/list/string/integer) or raw string if not JSON
response.headers    # map of lowercase header names → string values
```

HTTP 4xx/5xx responses are **not errors** — they return normally. The developer decides what to do with the status via `require`, `match`, or conditional logic.

### Request Verbs

```marreta
response = http_client.get("https://api.example.com/users")
response = http_client.post("https://api.example.com/users", { name: "Ana", email: "ana@co.com" })
response = http_client.put("https://api.example.com/users/42", { name: "Ana Maria" })
response = http_client.patch("https://api.example.com/users/42", { active: false })
response = http_client.delete("https://api.example.com/users/42")
```

### Named Parameters

```marreta
# Custom headers
response = http_client.post("https://pay.stripe.com/charges", payload,
    headers: { "Authorization": "Bearer #{env.STRIPE_KEY}", "Idempotency-Key": "#{request_id}" })

# Query string — added to the URL
response = http_client.get("https://api.example.com/search",
    query: { q: term, page: 1, limit: 20 })

# Timeout override in milliseconds (default: MARRETA_HTTP_TIMEOUT_MS or 30000)
response = http_client.get("https://slow-api.com/report", timeout: 10000)
```

### Pipeline — Flowing Data In and Out

The `>>` operator works with `http_client.*` in both directions:

- **Input:** piped value becomes the request body (POST/PUT/PATCH) or query params (GET/DELETE)
- **Output:** `response.body` flows forward via `>>` into `db.*`, `cache.*`, `queue.*`, tasks, or `map` blocks

```marreta
# Input: pipe payload into POST
payload >> http_client.post("https://orders.service/orders")

# Input: pipe query map into GET
{ status: "active", page: 1 } >> http_client.get("https://api.example.com/users")

# Output: response.body flows into the language pipeline
response = http_client.get("https://catalog.service/products/#{id}") as product_schema
require response.status == 200 else fail 502, "catalog unavailable"
response.body >> db.products.save

# Output: response.body flows through map transformation → DB → queue
response = http_client.get("https://orders.service/pending") as order_list
require response.status == 200 else fail 502, "orders service failed"
response.body
    >> map order
        order.synced_at = now()
        keep order
    >> db.orders.save
    >> queue.push("order_sync_complete")

# Fire-and-forget — no status check, no body needed
task notify_webhook(event)
    http_client.post(webhook_url, event)
```

> Only `Map` values can be piped into `http_client.get` or `http_client.delete` (query params). Any other type raises `TypeError`.

### Schema Contracts — Outgoing and Incoming

Schemas integrate at both the request (outgoing) and response (incoming) boundary — consistent with how schemas work everywhere else in the language.

#### Outgoing — validate payload before sending

`as schema` on the payload validates and strips before sending — the same pattern as `take payload as schema` and `queue.push ... as schema`:

```marreta
# Strips fields not declared in charge_request; raises TypeError on missing required
response = http_client.post("https://api.stripe.com/v1/charges",
    payload as charge_request,
    headers: { "Authorization": "Bearer #{env.STRIPE_KEY}" })
```

#### Incoming — document and shape the response body

`as schema` on the call itself shapes `response.body` — the same semantics as `reply CODE as schema`: extra fields are stripped, missing fields become `null`. No error is raised on mismatch.

The goal is **code clarity** — whoever reads the route knows the exact shape of the data flowing through, without inspecting the upstream API:

```marreta
# as user_schema makes the contract visible and shapes .body
response = http_client.get("https://users.service/users/#{id}") as user_schema
require response.status == 200 else fail 502, "user service failed"
response.body >> db.users.save   # body has the shape of user_schema

# Both directions — full contract documentation in one line
response = http_client.post("https://orders.service/orders",
    payload as order_request) as order_response

# Pipeline with schemas
payload as order_request >> http_client.post("https://orders.service/orders") as order_response
```

#### Schema Behavior Summary

| Context | Behavior |
|---|---|
| `http_client.post(url, payload as S)` | Strips undeclared fields from payload; raises `TypeError` on missing required |
| `http_client.get(url) as S` | Strips extra fields from `response.body`; missing fields → `null`; no raise |

### Parallel Calls via `*>>`

`*>>` broadcasts the same value to multiple task branches concurrently — the idiomatic pattern for aggregating several upstream services:

```marreta
route GET "/dashboard/:user_id"
    orders, profile, notifications = params.user_id *>>
        -> get_orders
        -> get_profile
        -> get_notifications
    reply 200, { orders: orders, profile: profile, notifications: notifications }

task get_orders(user_id)
    response = http_client.get("https://orders.service/users/#{user_id}/orders") as order_list
    require response.status == 200 else fail 502, "orders service failed"
    response.body

task get_profile(user_id)
    response = http_client.get("https://users.service/users/#{user_id}",
        headers: { "Authorization": "Bearer #{env.INTERNAL_KEY}" }) as user_profile
    require response.status == 200 else fail 502, "user service failed"
    response.body

task get_notifications(user_id)
    response = http_client.get("https://notifications.service/users/#{user_id}/notifications")
    response.status == 200 and response.body or []
```

### Real-World Examples

```marreta
# Payment gateway — schema on both sides documents the contract
task charge_card(amount, token)
    response = http_client.post("https://api.stripe.com/v1/charges",
        { amount: amount, source: token } as stripe_charge_request,
        headers: { "Authorization": "Bearer #{env.STRIPE_KEY}" },
        timeout: 5000) as stripe_charge_response
    require response.status == 200 else fail 502, response.body
    response.body

# Pipeline: charge → save → notify (response.body flows through the chain)
route POST "/payments" take payload as payment_request
    charge = charge_card(payload.amount, payload.token)
    charge >> db.payments.save >> queue.push("receipts")
    reply 201, charge

# Microservice call with rescue
task get_user_profile(user_id)
    response = http_client.get("https://users-service/users/#{user_id}") as user_profile rescue fail 503, "user service unavailable"
    require response.status == 200 else fail response.status, response.body
    response.body

# Sync upstream data → transform → persist → report
route POST "/sync/products" take payload
    response = http_client.get("https://catalog.service/products",
        query: { since: payload.since },
        headers: { "Authorization": "Bearer #{env.CATALOG_KEY}" }
    ) as product_list
    require response.status == 200 else fail 502, "catalog sync failed"
    synced = response.body
        >> map product
            product.synced_at = now()
            product.source = "catalog"
            keep product
        >> db.products.save
    reply 200, { synced: synced.length() }

# Read-through cache with pipeline
task get_product(id)
    cached = cache.get("product:#{id}")
    require not cached else cached
    response = http_client.get("https://catalog.service/products/#{id}") as product_schema
    require response.status == 200 else fail 502, "catalog unavailable"
    response.body >> cache.set("product:#{id}", ttl: 300)
```

---

## Environment Variables

```
MARRETA_HTTP_TIMEOUT_MS=5000    # optional, default 30000 (30s)
```

A single env var — a global safety net so no request hangs forever. Per-request `timeout:` overrides for that specific call:

```marreta
# This call uses 5s regardless of the global
response = http_client.post("https://api.stripe.com/v1/charges", payload,
    timeout: 5000)

# This call uses the global (default 30s)
response = http_client.get("https://slow-reporting.service/report")
```

Notably absent: no `BASE_URL`, no `DEFAULT_HEADERS`, no `MAX_REDIRECTS`. Real applications call N different APIs — each with its own URL, auth headers, and timeout requirements. Per-request named parameters handle everything. Redirect behavior (max 10) is hardcoded in the driver — API-to-API communication doesn't redirect.

---

## Error Semantics

| Failure | Behavior |
|---|---|
| HTTP 4xx/5xx response | Not an error — returns the response Map normally |
| Connection refused / DNS failure | Raises `HttpClientError` (catchable via `rescue`) |
| Timeout exceeded | Raises `HttpClientError` |
| TLS/SSL failure | Raises `HttpClientError` |
| Invalid URL | Raises `HttpClientError` |
| Non-JSON response body | Not an error — `body` is the raw string |
| Redirect loop | Raises `HttpClientError` |
| Schema mismatch on outgoing (`as`) | Raises `TypeError` (programmer error) |
| Schema mismatch on incoming (`as` on call) | Not an error — extra fields stripped, missing → `null` |
| Piped non-Map into GET/DELETE | Raises `TypeError` |

All `http_client.*` errors use `error.code = "infrastructure_error"` and `error.op = "http_client.{verb}"`, consistent with Marreta Error Identity. No `reqwest::` internals ever surface in error messages.

---

## AST & Parser Changes

**Implementation note:** No new AST expression variants are needed. `http_client` becomes `TokenKind::HttpClient` → `Expression::Identifier("http_client")`. Method calls like `http_client.get(...)` are parsed as standard `Expression::MethodCall`, dispatched at interpreter level via `dispatch_http_client()` — the same pattern used by `db.*`, `doc.*`, `cache.*`.

Named parameters (`headers:`, `query:`, `timeout:`) reuse the existing `Argument::Named` AST node. Outgoing schema (`http_client.post(url, payload as schema)`) reuses the existing `Argument::Typed` node already used for `take payload as schema`.

Pipeline injection (payload for POST/PUT/PATCH, query map for GET/DELETE) is handled in `apply_pipeline_value()`, mirroring the `cache.set` pattern from v0.9.

### New Token

| Token | Keyword |
|---|---|
| `TokenKind::HttpClient` | `http_client` |

`http_client` is a context keyword — verify no existing variable named `http_client` appears in `.marreta` files or test fixtures before registering.

`as schema` on the call expression (`http_client.get(url) as schema`) reuses the existing `Argument::Typed` / typed-call pattern already parsed for `take payload as schema`. The parser recognizes `as IDENT` immediately after a `MethodCall` on `HttpClientNamespace` and attaches it as the response schema annotation.

---

## Architecture

### Module Structure

```
src/http_client/
├── mod.rs        # HttpClientEngine, HttpClientConfig::from_env()
├── driver.rs     # HttpClient trait, HttpRequest, HttpResponse, HttpMethod, HttpClientError, MockHttpClient
└── reqwest.rs    # ReqwestDriver via reqwest::Client
```

### Value & Token

Following the existing pattern exactly:

- `TokenKind::HttpClient` in `src/token.rs` (after `Cache`)
- `"http_client"` registered in `keyword_lookup()` (after `"cache"`)
- Parser: `TokenKind::HttpClient` → `Expression::Identifier("http_client")` (same as `Db` / `Cache` / `Queue`)
- `Value::HttpClientNamespace` in `src/value.rs` (after `CacheNamespace`)

### `HttpClient` Trait (`src/http_client/driver.rs`)

```rust
#[async_trait]
pub trait HttpClient: Send + Sync {
    async fn execute(&self, request: HttpRequest) -> HttpClientResult<HttpResponse>;
}

pub struct HttpRequest {
    pub method:  HttpMethod,
    pub url:     String,
    pub body:    Option<Value>,
    pub headers: HashMap<String, String>,
    pub query:   HashMap<String, String>,
    pub timeout: Option<Duration>,
}

pub struct HttpResponse {
    pub status:  u16,
    pub body:    Value,
    pub headers: HashMap<String, String>,
}

pub enum HttpMethod { Get, Post, Put, Patch, Delete }

pub enum HttpClientError {
    ConnectionFailed(String),
    Timeout(String),
    TlsError(String),
    InvalidUrl(String),
    RequestFailed(String),
}
```

Single `execute()` method on the trait. All five verbs share the same signature — one method to implement, one to mock. `MockHttpClient` is defined in the same file under `#[cfg(test)]`.

### `ReqwestDriver` (`src/http_client/reqwest.rs`)

```rust
pub struct ReqwestDriver {
    client:          reqwest::Client,
    default_timeout: Duration,
}
```

- `reqwest::Client` created once at startup (connection pool, TLS). Shared across all handler threads via `Arc` — `reqwest::Client` is internally `Arc`'d.
- Response body: try `serde_json::from_str` → `Value`, fallback → `Value::String`. Content-Type is **not** consulted — many APIs lie about it.
- Response headers: lowercase all keys; last value wins for duplicates.
- Move `reqwest` from `[dev-dependencies]` to `[dependencies]`. Blocking feature stays in dev-deps only.

### Error Variant (`src/error.rs`)

```rust
// New ErrorCode variant:
HttpClientError,

// New MarretaError variant:
HttpClientError { message: String, operation: String },
```

### Interpreter (`src/interpreter.rs`)

- `evaluate()`: `"http_client"` identifier → `Value::HttpClientNamespace`
- `MethodCall` on `Value::HttpClientNamespace` → `dispatch_http_client(method, arguments)`
- `dispatch_http_client()`: parse verb, extract URL / body / headers / query / timeout, apply outgoing schema if present, merge default headers, call driver, apply incoming schema to response body if present (strip extra fields, missing → `null`), convert `HttpResponse` → `Value::Map { status, body, headers }`
- `apply_pipeline_value()`: new arm — body injection for POST/PUT/PATCH, query injection for GET/DELETE

### Health Endpoint

No changes. HTTP client has no single connectivity endpoint to health-check. The `/_health` response does not gain a new field.

---

## Phases

### Phase 1 — Tokens, Value & Error Scaffolding ✅

- `TokenKind::HttpClient` + keyword registration in `src/token.rs`
- `Value::HttpClientNamespace` with all trait impls in `src/value.rs`
- `MarretaError::HttpClientError` + `ErrorCode::HttpClientError` in `src/error.rs`
- Unit tests for all new variants

**Files:** `src/token.rs`, `src/value.rs`, `src/error.rs`

### Phase 2 — HttpClient Driver ✅

- `src/http_client/driver.rs` — trait, all types, `MockHttpClient` (`#[cfg(test)]`)
- `src/http_client/reqwest.rs` — `ReqwestDriver` via `reqwest::Client`
- `src/http_client/mod.rs` — `HttpClientConfig::from_env()`, `HttpClientEngine`
- Move `reqwest` to `[dependencies]` in `Cargo.toml`
- Register `mod http_client` in `src/lib.rs`

**Files:** `src/http_client/*`, `Cargo.toml`, `src/lib.rs`

### Phase 3 — Interpreter Integration ✅

- `dispatch_http_client()` — all 5 verbs, URL resolution, named param extraction, outgoing `as schema` validation, response conversion to `Value::Map`
- Pipeline injection in `apply_pipeline_value()` (body for POST/PUT/PATCH; query for GET/DELETE)
- `resolve_named_map()` helper for `headers:` / `query:` extraction
- `resolve_named_timeout()` helper for `timeout:` (milliseconds → `Duration`)
- 20+ unit tests with `MockHttpClient`

**Files:** `src/interpreter.rs`

### Phase 4 — Server Wiring & Config ✅

- `HttpClientEngine::from_env()` initialization in `main.rs` (always runs — no conditional, unlike `db.*`)
- Thread `http_client: Arc<dyn HttpClient>` through `ServerConfig` → `Interpreter`
- `config.rs` — read `MARRETA_HTTP_*` env vars into `HttpClientConfig`
- Integration test: spawn local axum server, call it from a MarretaLang route via `http_client.*`

**Files:** `src/main.rs`, `src/server.rs`, `src/config.rs`

### Phase 5 — Functional Tests & Docs ✅

- `examples/functional_tests/routes/http_client.marreta` — Section 30 (30 routes: stubs + callers)
- Test strategy: one set of routes acts as the "external API" (stubs); another calls them via `http_client.*`. Fully self-referencing — no external dependencies.
- Covers: all 5 verbs, `headers:`, `query:`, `timeout:`, response envelope (`.status`, `.body`, `.headers`), `match response.status`, rescue on connection failure, fire-and-forget, pipeline input for POST/PUT/PATCH/GET, pipeline output to cache (read-through pattern), pipeline map, multi-step chain
- `examples/functional_tests/test.sh` — Section 30 (25 test cases); `patch()` helper added
- `docs/spec/SPEC.md` — §9 HTTP Client section added; §9+ renumbered; roadmap entry updated
- **334/334 functional tests passing**

**Files:** `examples/functional_tests/`, `docs/spec/`, `CHANGELOG.md`

### Test Coverage Requirement

All phases must maintain the **80% unit test coverage** floor across `src/`:

- `src/http_client/driver.rs`, `src/http_client/mod.rs` — trait, config, error types: ≥ 80%
- `src/http_client/reqwest.rs` — via `MockHttpClient` for unit tests; integration tests against a real HTTP server are separate and do not count toward the 80% floor
- `src/token.rs`, `src/value.rs`, `src/error.rs` — new variants: happy path + error cases
- `src/interpreter.rs` — all `Http*` dispatch branches, pipeline injection, outgoing schema, named param extraction, driver-absent error path: all branches covered

Coverage measured via `cargo llvm-cov` (or `cargo tarpaulin`). Each phase must not reduce overall coverage below 80%.

---

## Design Watch Points

### 1. HTTP 4xx/5xx are not errors — deliberate

This differs from Python `requests` (which raises by default). In MarretaLang, the developer guards explicitly with `require response.status == 200`. Silently raising on non-2xx would force `rescue` everywhere, making the common case noisy.

### 2. Auto-detect JSON — do not check Content-Type

Many APIs return `Content-Type: text/plain` with a valid JSON body, or `application/json` with a plain string error. Parse the body and fall back to a raw string — more robust than trusting the header.

### 3. `reqwest` moves to production dependencies

Adds ~2–3s to clean builds. Acceptable tradeoff — `reqwest` is idiomatic and battle-tested. The blocking feature stays in dev-deps only.

### 4. No `PROVIDER` pattern

HTTP has no competing backends. The `HttpClient` trait exists for testability only. `MockHttpClient` is the only non-`reqwest` implementation — no provider enum, no dispatch table.

### 5. Pipeline input direction

`>>` into `http_client.*` is unambiguous: `payload >> http_client.post(url)` sends payload as the request body; `{ q: q } >> http_client.get(url)` sends the map as query params. The result of the call is always the response Map — it flows out naturally and can be assigned or chained via property access (`.body`, `.status`).

### 6. Single `reqwest::Client` instance

Created at startup, shared across all handlers via `Arc`. Connection pool and TLS session cache are shared — correct and efficient.

### 7. Timeout layering

`MARRETA_HTTP_TIMEOUT_MS` (default 30s) is the global safety net. Per-request `timeout: N` overrides for that call only. Applied in `dispatch_http_client()`, before calling the driver.

### 8. No `BASE_URL`, `DEFAULT_HEADERS`, or `MAX_REDIRECTS`

Deliberate omission. Real applications call N different APIs — Stripe, internal microservices, notification providers — each with its own URL, auth headers, and timeout. Global defaults for these assume a single-API world that doesn't exist. Per-request named parameters handle everything. Redirect limit (10) is hardcoded in the driver — API-to-API communication doesn't redirect, and reqwest's default is sane.

### 9. Circular HTTP calls

A route calling itself via `http_client.get` is the developer's responsibility. The timeout prevents permanent hangs. No loop detection — the complexity of detecting arbitrary call cycles across distributed services is out of scope.

---

## Verification

1. `cargo test --lib` — all existing tests pass; all new unit tests pass (≥ 80% coverage maintained)
2. `cargo build` — compiles cleanly with `reqwest` in production dependencies
3. `bash examples/functional_tests/test.sh` — Section 30 passes (self-referencing calls within the test server)
4. Manual smoke test: write a route that calls `https://httpbin.org/get`, verify `response.status == 200` and `response.body` is a Map with expected fields
