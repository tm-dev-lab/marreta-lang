# MarretaLang — HTTP Runtime Implementation Plan (v0.2)

> Status: Delivered.

> **Goal:** Add a fully functional HTTP server to MarretaLang, enabling `.marreta` files to declare REST routes that are registered and served by an `axum`-based web server powered by `tokio`.
>
> **At the end of this phase**, a developer should be able to write a `.marreta` file with `route` declarations and run `marreta serve app.marreta` to get a working REST API with no boilerplate.
>
> **Not included in this phase:** Database, Queue, Cache (v0.3–v0.5). Routes that reference `db`, `queue`, or `cache` will parse and register correctly but produce a `NotImplemented` error at runtime when those operations are reached.

---

## Table of Contents

1. [Architecture Overview](#1-architecture-overview)
2. [Phase 1 — Dependencies & Cargo.toml](#2-phase-1--dependencies--cargotoml)
3. [Phase 2 — New AST Nodes](#3-phase-2--new-ast-nodes)
4. [Phase 3 — Lexer & Parser Extensions](#4-phase-3--lexer--parser-extensions)
5. [Phase 4 — HTTP Value Types](#5-phase-4--http-value-types)
6. [Phase 5 — Route Registry](#6-phase-5--route-registry)
7. [Phase 6 — HTTP Server (axum)](#7-phase-6--http-server-axum)
8. [Phase 7 — Request Binding (`take`)](#8-phase-7--request-binding-take)
9. [Phase 8 — Response (`reply` / `fail`)](#9-phase-8--response-reply--fail)
10. [Phase 9 — `marreta serve` CLI Command](#10-phase-9--marreta-serve-cli-command)
11. [Phase 10 — `marreta.env` Configuration](#11-phase-10--marretaenv-configuration)
12. [Phase 11 — Tests](#12-phase-11--tests)
13. [Acceptance Criteria](#13-acceptance-criteria)

---

## 1. Architecture Overview

### 1.1 New Execution Flow

```
Source Code (.marreta)
        │
        ▼
   ┌─────────┐
   │  LEXER  │
   └────┬────┘
        │ Vec<Token>
        ▼
   ┌─────────┐
   │ PARSER  │  Now also parses `route`, `take`, `reply`, `fail`, `listen`
   └────┬────┘
        │ AST (Vec<Statement>)
        ▼
   ┌──────────────┐
   │ ROUTE LOADER │  Separates route declarations from top-level statements
   └──────┬───────┘
          │ RouteRegistry
          ▼
   ┌──────────────┐
   │  AXUM SERVER │  Registers each route as an async handler
   └──────┬───────┘
          │
          ▼
   On each HTTP request:
   ┌─────────────┐
   │ INTERPRETER │  Fresh scope per request; injects payload/query/headers/params
   └─────────────┘
          │
          ▼
     reply / fail / HttpError → axum Response
```

### 1.2 Key Design Decisions

| Decision | Choice | Reason |
|---|---|---|
| HTTP framework | `axum` | Ergonomic, built on `hyper`, native `tokio` integration |
| Async runtime | `tokio` | Spec requirement; enables concurrent `*>>` in future |
| Interpreter per request | Yes (cloned shared state) | Route bodies are stateless; tasks/consts are read-only shared |
| `reply`/`fail` mechanism | Rust `Err(MarretaError::HttpResponse)` | Interrupt execution cleanly, bubble up to handler |
| URL params | Extracted by axum, injected as scope variables | `:id` → `id` variable available in route body |
| `take` binding | `payload` = JSON body, `query` = query string, `headers` = header map | Matches SPEC.md sections 3.3–3.5 |

### 1.3 Rust Module Changes

```
src/
├── main.rs              # + `serve` command
├── server.rs            # NEW: axum server setup and route registration
├── route_loader.rs      # NEW: extracts route declarations from AST
├── request.rs           # NEW: request context (payload, query, headers, params)
├── interpreter.rs       # + reply/fail handling, request context injection
├── ast.rs               # + Route, Reply, Fail statement nodes
├── token.rs             # + Route, Take, Reply, Fail, Listen, GET/POST/PUT/PATCH/DELETE tokens (already reserved)
├── parser.rs            # + parse_route, parse_reply, parse_fail
├── error.rs             # + HttpResponse variant (used for reply/fail control flow)
└── value.rs             # (no changes needed)
```

---

## 2. Phase 1 — Dependencies & Cargo.toml

Add `axum`, `tokio`, and `serde_json` to `Cargo.toml`:

```toml
[dependencies]
axum = "0.7"
tokio = { version = "1", features = ["full"] }
serde_json = "1"
tower = "0.4"

[dev-dependencies]
pretty_assertions = "1"
reqwest = { version = "0.12", features = ["json", "blocking"] }
```

**Note:** `serde_json` is used to serialize/deserialize request bodies and responses. `reqwest` (blocking) is used in integration tests to make real HTTP requests to the running server.

### Deliverables

- [ ] `Cargo.toml` updated with all dependencies
- [ ] `cargo build` compiles cleanly

---

## 3. Phase 2 — New AST Nodes

### 3.1 New Statement variants (`ast.rs`)

```rust
/// route GET "/users/:id"
///     body...
Route {
    verb: HttpVerb,
    path: String,
    take: Option<TakeBinding>,   // take payload / take query / take headers
    body: Vec<Statement>,
    line: usize,
    column: usize,
},

/// reply 200, data
Reply {
    status_code: i64,
    body: Expression,
    line: usize,
    column: usize,
},

/// fail 404, "Not found"
Fail {
    status_code: i64,
    message: Expression,   // String expression (may contain interpolation)
    line: usize,
    column: usize,
},
```

### 3.2 New types

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum HttpVerb {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TakeBinding {
    Payload(String),   // take payload  → variable name is "payload"
    Query(String),     // take query    → variable name is "query"
    Headers(String),   // take headers  → variable name is "headers"
}
```

### Deliverables

- [ ] `HttpVerb` enum
- [ ] `TakeBinding` enum
- [ ] `Statement::Route`, `Statement::Reply`, `Statement::Fail` added
- [ ] Unit tests for manual AST construction of each new node

---

## 4. Phase 3 — Lexer & Parser Extensions

### 4.1 Lexer

The tokens `Route`, `Take`, `Reply`, `Fail`, `Listen`, `Get`, `Post`, `Put`, `Patch`, `Delete` are already in `TokenKind` and recognized by `keyword_lookup()`. **No lexer changes needed.**

### 4.2 Parser — `parse_statement()` extension

Add three new arms to `parse_statement()`:

```rust
TokenKind::Route => self.parse_route(),
TokenKind::Reply => self.parse_reply(),
TokenKind::Fail  => self.parse_fail(),
```

### 4.3 `parse_route()`

```
route VERB "PATH" [take IDENTIFIER] NEWLINE INDENT
    statement*
DEDENT
```

```rust
fn parse_route(&mut self) -> Result<Statement, MarretaError> {
    let (line, column) = self.current_position();
    self.expect(TokenKind::Route)?;

    let verb = match self.current_kind() {
        TokenKind::Get    => { self.advance(); HttpVerb::Get }
        TokenKind::Post   => { self.advance(); HttpVerb::Post }
        TokenKind::Put    => { self.advance(); HttpVerb::Put }
        TokenKind::Patch  => { self.advance(); HttpVerb::Patch }
        TokenKind::Delete => { self.advance(); HttpVerb::Delete }
        other => return Err(MarretaError::UnexpectedToken { expected: "HTTP verb".into(), got: ... }),
    };

    let path = self.expect_string()?;  // "/users/:id"

    let take = if self.check(&TokenKind::Take) {
        self.advance();
        let name = self.expect_identifier()?;
        Some(match name.as_str() {
            "payload" => TakeBinding::Payload(name),
            "query"   => TakeBinding::Query(name),
            "headers" => TakeBinding::Headers(name),
            _         => TakeBinding::Payload(name),  // custom name treated as payload
        })
    } else {
        None
    };

    self.expect(TokenKind::Newline)?;
    let body = self.parse_block()?;

    Ok(Statement::Route { verb, path, take, body, line, column })
}
```

### 4.4 `parse_reply()`

```
reply INTEGER, expression
```

```rust
fn parse_reply(&mut self) -> Result<Statement, MarretaError> {
    let (line, column) = self.current_position();
    self.expect(TokenKind::Reply)?;
    let status_code = self.expect_integer()?;
    self.expect(TokenKind::Comma)?;
    let body = self.parse_expression(PREC_NONE)?;
    Ok(Statement::Reply { status_code, body, line, column })
}
```

### 4.5 `parse_fail()`

```
fail INTEGER, expression
```

```rust
fn parse_fail(&mut self) -> Result<Statement, MarretaError> {
    let (line, column) = self.current_position();
    self.expect(TokenKind::Fail)?;
    let status_code = self.expect_integer()?;
    self.expect(TokenKind::Comma)?;
    let message = self.parse_expression(PREC_NONE)?;
    Ok(Statement::Fail { status_code, message, line, column })
}
```

### Deliverables

- [ ] `parse_route()` with verb, path, optional `take`, indented body
- [ ] `parse_reply()` with status code and body expression
- [ ] `parse_fail()` with status code and message expression
- [ ] Unit tests for each new parser construct

---

## 5. Phase 4 — HTTP Value Types

### 5.1 `error.rs` — New variant for control flow

`reply` and `fail` need to interrupt execution and bubble up to the axum handler. We use a dedicated error variant (not a real error — it's control flow):

```rust
/// Emitted by `reply` and `fail` to terminate route execution.
/// Caught by the route handler and converted to an HTTP response.
HttpResponse {
    status_code: u16,
    body: serde_json::Value,
    is_error: bool,   // false = reply, true = fail
},
```

### 5.2 Interpreter — `execute_statement()` extension

```rust
Statement::Reply { status_code, body, .. } => {
    let val = self.evaluate(body)?;
    let json = value_to_json(&val);
    Err(MarretaError::HttpResponse {
        status_code: *status_code as u16,
        body: json,
        is_error: false,
    })
}

Statement::Fail { status_code, message, .. } => {
    let msg = self.evaluate(message)?;
    let json = serde_json::json!({ "error": msg.to_string() });
    Err(MarretaError::HttpResponse {
        status_code: *status_code as u16,
        body: json,
        is_error: true,
    })
}
```

The existing `MarretaError::HttpError` (from `require`/`reject`) also needs to be caught and converted:

```rust
// In the route handler:
MarretaError::HttpError { status_code, message } => {
    // Convert to HttpResponse automatically
    (status_code as u16, json!({ "error": message }))
}
```

### 5.3 `value_to_json()` helper

```rust
pub fn value_to_json(value: &Value) -> serde_json::Value {
    match value {
        Value::Integer(n)  => json!(n),
        Value::Float(n)    => json!(n),
        Value::String(s)   => json!(s),
        Value::Boolean(b)  => json!(b),
        Value::Null        => serde_json::Value::Null,
        Value::List(items) => json!(items.iter().map(value_to_json).collect::<Vec<_>>()),
        Value::Map(m)      => {
            let obj: serde_json::Map<_, _> = m.borrow()
                .iter()
                .map(|(k, v)| (k.clone(), value_to_json(v)))
                .collect();
            serde_json::Value::Object(obj)
        }
        Value::Task { name, .. } => json!({ "task": name }),
    }
}
```

### 5.4 `json_to_value()` helper (for request body deserialization)

```rust
pub fn json_to_value(json: &serde_json::Value) -> Value {
    match json {
        serde_json::Value::Null       => Value::Null,
        serde_json::Value::Bool(b)    => Value::Boolean(*b),
        serde_json::Value::Number(n)  => {
            if let Some(i) = n.as_i64() { Value::Integer(i) }
            else { Value::Float(n.as_f64().unwrap_or(0.0)) }
        }
        serde_json::Value::String(s)  => Value::String(s.clone()),
        serde_json::Value::Array(arr) => Value::List(arr.iter().map(json_to_value).collect()),
        serde_json::Value::Object(obj) => {
            let map: HashMap<String, Value> = obj.iter()
                .map(|(k, v)| (k.clone(), json_to_value(v)))
                .collect();
            Value::Map(Rc::new(RefCell::new(map)))
        }
    }
}
```

### Deliverables

- [ ] `MarretaError::HttpResponse` variant
- [ ] `Statement::Reply` and `Statement::Fail` execute correctly in interpreter
- [ ] `require`/`reject` `HttpError` also converted to response in handler
- [ ] `value_to_json()` for all Value variants
- [ ] `json_to_value()` for all JSON types
- [ ] Unit tests for both conversion helpers

---

## 6. Phase 5 — Route Registry

### 6.1 `route_loader.rs`

Separates a parsed AST into:
- **Route declarations** → registered on the HTTP server
- **Top-level statements** → executed once at startup (task definitions, constants)

```rust
pub struct RouteRegistry {
    pub routes: Vec<RouteDefinition>,
    pub startup_stmts: Vec<Statement>,
}

pub struct RouteDefinition {
    pub verb: HttpVerb,
    pub path: String,
    pub take: Option<TakeBinding>,
    pub body: Vec<Statement>,
}

pub fn load(program: Vec<Statement>) -> RouteRegistry {
    let mut routes = Vec::new();
    let mut startup_stmts = Vec::new();

    for stmt in program {
        match stmt {
            Statement::Route { verb, path, take, body, .. } => {
                routes.push(RouteDefinition { verb, path, take, body });
            }
            other => startup_stmts.push(other),
        }
    }

    RouteRegistry { routes, startup_stmts }
}
```

### 6.2 Route Conflict Validation

Before registering routes with axum, `RouteRegistry` validates for conflicts that would cause a panic or silent misbehavior at runtime.

**Conflict rules (axum/matchit semantics):**

| Case | Example | Result |
|---|---|---|
| Exact duplicate | `GET /users` × 2 | **Error** — identical route registered twice |
| Same pattern, different param names | `GET /users/:id` and `GET /users/:name` | **Error** — `matchit` treats these as the same pattern |
| Same structure, all params | `GET /:a/:b` and `GET /:x/:y` | **Error** — structurally identical wildcard paths |
| Literal vs param (same segment) | `GET /users/active` and `GET /users/:id` | **OK** — axum resolves literal first |
| Different verbs | `GET /users/:id` and `POST /users/:id` | **OK** — verb disambiguates |

**Algorithm — normalize path to structural pattern:**

```rust
fn path_pattern(path: &str) -> String {
    // Replace all :param segments with the placeholder ":*"
    // "/users/:id/orders/:order_id" → "/users/:*/orders/:*"
    path.split('/')
        .map(|seg| if seg.starts_with(':') { ":*" } else { seg })
        .collect::<Vec<_>>()
        .join("/")
}
```

Two routes conflict when they share the same verb AND the same normalized pattern.

**Implementation in `load()`:**

```rust
pub fn load(program: Vec<Statement>) -> Result<RouteRegistry, MarretaError> {
    let mut routes: Vec<RouteDefinition> = Vec::new();
    let mut startup_stmts = Vec::new();

    for stmt in program {
        match stmt {
            Statement::Route { verb, path, take, body, line, column } => {
                // Check for conflicts against already-registered routes
                let new_pattern = path_pattern(&path);
                for existing in &routes {
                    if existing.verb == verb && path_pattern(&existing.path) == new_pattern {
                        return Err(MarretaError::RouteConflict {
                            verb: format!("{:?}", verb),
                            path_a: existing.path.clone(),
                            path_b: path.clone(),
                            line,
                            column,
                        });
                    }
                }
                routes.push(RouteDefinition { verb, path, take, body });
            }
            other => startup_stmts.push(other),
        }
    }

    Ok(RouteRegistry { routes, startup_stmts })
}
```

**New error variant in `error.rs`:**

```rust
RouteConflict {
    verb: String,
    path_a: String,
    path_b: String,
    line: usize,
    column: usize,
},
```

**Error message:**

```
Route conflict at line 5:1
  GET "/users/:name" conflicts with GET "/users/:id"
  Both routes match the same URL pattern. Use distinct path segments or different HTTP verbs.
```

**Note on false positives:** The literal-vs-param case (`/users/active` vs `/users/:id`) is intentionally **not flagged** — axum handles it correctly, literal segments always win. Only cases that would cause axum to panic or silently shadow a route are flagged.

### 6.3 Startup execution

Top-level statements (task definitions, constants) run once before the server starts. Their results are stored in a shared, read-only `Environment` that each request clones:

```rust
// In server startup:
let mut base_interp = Interpreter::new();
base_interp.execute(&registry.startup_stmts)?;
let shared_env = Arc::new(base_interp.into_environment());
```

Each request handler clones `shared_env` into a fresh `Interpreter` for isolation.

### Deliverables

- [ ] `route_loader.rs` with `RouteRegistry` and `load()`
- [ ] `path_pattern()` normalization for conflict detection
- [ ] `MarretaError::RouteConflict` variant with friendly message
- [ ] Conflict detection: exact duplicates, same-pattern different param names, same wildcard structure
- [ ] Literal-vs-param (`/users/active` vs `/users/:id`) correctly allowed
- [ ] Top-level statements executed once at startup
- [ ] Route definitions stored for server registration
- [ ] Unit tests for loader with mixed route/non-route programs
- [ ] Unit tests for each conflict scenario (duplicate, param rename, wildcard clash, allowed literal-vs-param)

---

## 7. Phase 6 — HTTP Server (axum)

### 7.1 `server.rs`

```rust
use axum::{Router, extract::*, response::*, routing::*};
use std::sync::Arc;
use tokio::net::TcpListener;

pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

pub async fn serve(
    registry: RouteRegistry,
    shared_env: Arc<Environment>,
    config: ServerConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut router = Router::new();

    for route_def in registry.routes {
        router = register_route(router, route_def, Arc::clone(&shared_env));
    }

    let addr = format!("{}:{}", config.host, config.port);
    let listener = TcpListener::bind(&addr).await?;
    println!("MarretaLang serving on http://{}", addr);
    axum::serve(listener, router).await?;
    Ok(())
}
```

### 7.2 Route registration

Each `RouteDefinition` becomes an axum handler. URL parameters (`:id`) are extracted by axum and injected into the interpreter scope.

```rust
fn register_route(
    router: Router,
    route: RouteDefinition,
    shared_env: Arc<Environment>,
) -> Router {
    let axum_path = to_axum_path(&route.path);  // "/users/:id" stays the same

    let handler = move |
        Path(params): Path<HashMap<String, String>>,
        Query(query_params): Query<HashMap<String, String>>,
        headers: HeaderMap,
        body: Option<Json<serde_json::Value>>,
    | {
        let env = shared_env.clone();
        let route = route.clone();
        async move {
            execute_route(route, params, query_params, headers, body, env).await
        }
    };

    match route.verb {
        HttpVerb::Get    => router.route(&axum_path, get(handler)),
        HttpVerb::Post   => router.route(&axum_path, post(handler)),
        HttpVerb::Put    => router.route(&axum_path, put(handler)),
        HttpVerb::Patch  => router.route(&axum_path, patch(handler)),
        HttpVerb::Delete => router.route(&axum_path, delete(handler)),
    }
}
```

### 7.3 `execute_route()`

```rust
async fn execute_route(
    route: RouteDefinition,
    url_params: HashMap<String, String>,
    query_params: HashMap<String, String>,
    headers: HeaderMap,
    body: Option<Json<serde_json::Value>>,
    shared_env: Arc<Environment>,
) -> impl IntoResponse {
    // Clone shared env into a fresh interpreter
    let mut interp = Interpreter::from_environment((*shared_env).clone());

    // Inject URL params as variables
    for (key, val) in &url_params {
        interp.env_set(key.clone(), Value::String(val.clone()));
    }

    // Inject `take` binding
    if let Some(take) = &route.take {
        match take {
            TakeBinding::Payload(name) => {
                let val = body.map(|b| json_to_value(&b.0)).unwrap_or(Value::Null);
                interp.env_set(name.clone(), val);
            }
            TakeBinding::Query(name) => {
                let map = query_params.into_iter()
                    .map(|(k, v)| (k, Value::String(v)))
                    .collect();
                interp.env_set(name.clone(), Value::Map(Rc::new(RefCell::new(map))));
            }
            TakeBinding::Headers(name) => {
                let map = headers.iter()
                    .filter_map(|(k, v)| Some((k.as_str().to_string(), Value::String(v.to_str().ok()?.to_string()))))
                    .collect();
                interp.env_set(name.clone(), Value::Map(Rc::new(RefCell::new(map))));
            }
        }
    }

    // Execute route body
    match interp.execute(&route.body) {
        Err(MarretaError::HttpResponse { status_code, body, .. }) => {
            (StatusCode::from_u16(status_code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
             Json(body)).into_response()
        }
        Err(MarretaError::HttpError { status_code, message }) => {
            (StatusCode::from_u16(status_code as u16).unwrap_or(StatusCode::BAD_REQUEST),
             Json(json!({ "error": message }))).into_response()
        }
        Err(e) => {
            (StatusCode::INTERNAL_SERVER_ERROR,
             Json(json!({ "error": e.to_string() }))).into_response()
        }
        Ok(_) => {
            // Route completed without reply — return 204 No Content
            StatusCode::NO_CONTENT.into_response()
        }
    }
}
```

### Deliverables

- [ ] `server.rs` with `serve()` function
- [ ] Route registration for all 5 HTTP verbs
- [ ] URL params injected as scope variables
- [ ] `execute_route()` handles all error/response cases
- [ ] 204 No Content for routes with no `reply`

---

## 8. Phase 7 — Request Binding (`take`)

### 8.1 `take payload` (POST/PUT/PATCH)

JSON body deserialized to `Value::Map` and bound to variable name:

```marreta
route POST "/users" take payload
    require payload.name else fail 400, "Name is required"
    reply 201, payload
```

→ `payload` is a `Value::Map` with all JSON keys as properties.

### 8.2 `take query` (GET)

Query string parameters deserialized to `Value::Map`:

```marreta
route GET "/products" take query
    limit = query.limit or 10
    reply 200, { limit: limit }
```

→ `query` is a `Value::Map` where all values are `Value::String` (query params are always strings).

### 8.3 `take headers`

Request headers as `Value::Map`:

```marreta
route GET "/protected" take headers
    token = headers.authorization
    require token else fail 401, "Unauthorized"
    reply 200, { ok: true }
```

### 8.4 URL parameters (automatic, no `take` needed)

```marreta
route GET "/users/:id"
    reply 200, { id: id }
```

→ `:id` is automatically available as `id` (string) in the route scope.

### 8.5 Multiple bindings (future consideration)

The spec shows only one `take` per route. For v0.2, a single `take` is sufficient. Multiple sources (body + headers) can be accessed by using `take headers` and reading `payload` from the body manually if needed — or we can add `take payload, headers` syntax in a future version.

### Deliverables

- [ ] `take payload` binds JSON body as Map
- [ ] `take query` binds query string as Map
- [ ] `take headers` binds headers as Map
- [ ] URL params (`:name`) auto-injected as String variables
- [ ] Tests for each binding type

---

## 9. Phase 8 — Response (`reply` / `fail`)

### 9.1 `reply`

```marreta
reply 200, user          # Serializes user (Map/List/any Value) to JSON
reply 201, { id: new_id }  # Inline map literal
reply 204, null          # No content
```

**Behavior:**
- Immediately terminates route execution (via `Err(HttpResponse)`)
- Serializes the Value to JSON via `value_to_json()`
- Sets `Content-Type: application/json`

### 9.2 `fail`

```marreta
fail 400, "Name is required"
fail 404, "User not found"
fail 500, "Internal error"
```

**Behavior:**
- Immediately terminates route execution
- Response body: `{ "error": "message" }`

### 9.3 `require`/`reject` integration

These already produce `MarretaError::HttpError` — the route handler catches them and returns the correct HTTP response. No changes to interpreter logic needed.

```marreta
require payload.name else fail 400, "Name is required"
# If payload.name is falsy → 400 { "error": "Name is required" }
```

### Deliverables

- [ ] `reply` produces correct JSON response with status code
- [ ] `fail` produces `{ "error": "..." }` JSON with status code
- [ ] `require`/`reject` HttpError converted to response correctly
- [ ] Tests for each response type

---

## 10. Phase 9 — `marreta serve` CLI Command

### 10.1 New CLI command

```bash
marreta serve app.marreta          # Default port 8080
marreta serve app.marreta --port 3000
marreta serve routes/ --port 8080  # Directory (loads all .marreta files)
```

### 10.2 `main.rs` extension

```rust
Some("serve") => {
    let path = match args.get(2) {
        Some(p) => p,
        None => {
            eprintln!("Usage: marreta serve <file.marreta> [--port PORT]");
            process::exit(1);
        }
    };
    let port = parse_port_arg(&args).unwrap_or(8080);
    run_serve(path, port);
}
```

```rust
fn run_serve(path: &str, port: u16) {
    let source = read_file(path);
    let tokens = tokenize(&source);
    let program = parse(tokens);
    let registry = route_loader::load(program);

    // Execute startup statements (task defs, constants)
    let mut base_interp = Interpreter::new();
    if let Err(e) = base_interp.execute(&registry.startup_stmts) {
        eprintln!("Startup error: {}", e);
        process::exit(1);
    }

    let shared_env = Arc::new(base_interp.into_environment());
    let config = ServerConfig { host: "0.0.0.0".into(), port };

    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(server::serve(registry, shared_env, config))
        .unwrap_or_else(|e| {
            eprintln!("Server error: {}", e);
            process::exit(1);
        });
}
```

### 10.3 Updated help text

```
marreta serve <file>       Start HTTP server from .marreta file
marreta serve <dir>        Load all .marreta files from directory
  --port PORT              Port to listen on (default: 8080)
  --host HOST              Host to bind (default: 0.0.0.0)
```

### Deliverables

- [ ] `marreta serve <file>` starts the HTTP server
- [ ] `--port` flag supported
- [ ] Startup task/constant definitions executed before server starts
- [ ] Informational output: "Serving on http://0.0.0.0:8080"
- [ ] Graceful handling of startup errors

---

## 11. Phase 10 — `marreta.env` Configuration

### 11.1 `config.rs` — New module

Reads `marreta.env` from the current directory (or path specified):

```rust
pub struct MarretaConfig {
    pub host: String,
    pub port: u16,
    // Future: db_provider, db_url, queue_provider, etc.
}

impl MarretaConfig {
    pub fn load() -> Self {
        // Read marreta.env if it exists, fall back to defaults
        let host = env_or("MARRETA_HOST", "0.0.0.0");
        let port = env_or("MARRETA_PORT", "8080").parse().unwrap_or(8080);
        Self { host, port }
    }
}
```

### 11.2 Example `marreta.env`

```env
MARRETA_HOST=0.0.0.0
MARRETA_PORT=8080
```

### Deliverables

- [ ] `config.rs` reads `marreta.env` / environment variables
- [ ] `--port` CLI flag overrides config file
- [ ] Sensible defaults if no config present

---

## 12. Phase 11 — Tests

### 12.1 Unit Tests

| Module | Tests |
|---|---|
| `ast.rs` | New Route, Reply, Fail node construction |
| `parser.rs` | Parse all route verb types, with/without `take`, parse reply/fail |
| `interpreter.rs` | reply/fail emit HttpResponse, require/reject emit HttpError |
| `route_loader.rs` | Mixed AST correctly split into routes vs startup stmts |
| `value_to_json()` | All Value variants → JSON |
| `json_to_value()` | All JSON types → Value |

### 12.2 Integration Tests (HTTP)

Integration tests start an actual server on a random free port and make real HTTP requests using `reqwest`:

```rust
fn start_test_server(source: &str) -> u16 {
    // Parse source, load routes, start server on random port
    // Return port number
}

#[test]
fn test_get_route_returns_200() {
    let port = start_test_server(r#"
route GET "/hello"
    reply 200, { message: "hello" }
"#);
    let resp = reqwest::blocking::get(format!("http://127.0.0.1:{}/hello", port)).unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().unwrap();
    assert_eq!(json["message"], "hello");
}
```

**Test scenarios:**

| # | Scenario |
|---|---|
| 1 | `GET /hello` → 200 with JSON body |
| 2 | `POST /echo take payload` → 201 echoing body |
| 3 | URL param `:id` injected and accessible |
| 4 | `take query` binds query string params |
| 5 | `take headers` binds request headers |
| 6 | `require` failing → 400 error JSON |
| 7 | `reject` firing → 402 error JSON |
| 8 | `fail 404, "Not found"` → 404 error JSON |
| 9 | Route with task call (defined at top level) |
| 10 | Route with `match` expression |
| 11 | Route with pipeline |
| 12 | Route with `or` default value from query |
| 13 | Unknown route → 404 (axum default) |
| 14 | Route with no `reply` → 204 No Content |
| 15 | `reply 200, null` → 200 with null body |
| 16 | Startup task visible inside route body |
| 17 | Duplicate exact route → `RouteConflict` error at boot (no server starts) |
| 18 | Same path, different param names → `RouteConflict` error at boot |
| 19 | Literal + param same segment (`/users/active` + `/users/:id`) → allowed, both work correctly |

### 12.3 Example file: `examples/http_hello.marreta`

```marreta
route GET "/hello"
    reply 200, { message: "Hello from MarretaLang!" }

route GET "/hello/:name"
    reply 200, { message: "Hello #{name}!" }

route POST "/echo" take payload
    reply 200, payload
```

### Deliverables

- [ ] Unit tests for all new AST/parser/interpreter nodes (minimum 20 new tests)
- [ ] Integration test server helper
- [ ] All 16 HTTP scenarios tested
- [ ] `examples/http_hello.marreta` works end-to-end with `marreta serve`

---

## 13. Acceptance Criteria

The HTTP Runtime v0.2 is **complete** when all items below are met:

### Functional

- [x] `route GET/POST/PUT/PATCH/DELETE "path"` declares a route
- [x] `take payload` binds JSON request body as Map
- [x] `take query` binds query string params as Map
- [x] `take headers` binds request headers as Map
- [x] URL params (`:name`) auto-injected as variables in route scope (as String)
- [x] `reply CODE, data` returns JSON response and terminates route
- [x] `fail CODE, "message"` returns `{ "error": "..." }` and terminates route
- [x] `require`/`reject` guards produce HTTP error responses
- [x] Tasks defined at top level are accessible inside route bodies
- [x] `marreta serve <file>` starts the HTTP server
- [x] Server reads host/port from `marreta.env` or `--port` flag
- [x] Conflicting routes detected at boot with a clear error message (before server starts)

### Quality

- [x] `cargo test` passes with 0 failures (443 tests)
- [x] `cargo clippy` with no warnings
- [x] `cargo fmt` applied
- [x] No `unwrap()` in production code (only `RwLock::read().unwrap()` — idiomatic)
- [x] All errors typed via `MarretaError`
- [x] Server handles panics/errors per-request (one bad request doesn't kill the server)

### Documentation

- [x] `SPEC.md` updated with v0.2 divergences (10 notes)
- [x] `examples/http_hello.marreta` demonstrates all HTTP features (9 routes)
- [x] CHANGELOG updated

---

## Recommended Implementation Order

```
1. Cargo.toml — add axum, tokio, serde_json, reqwest (dev)
2. ast.rs — add Route, Reply, Fail, HttpVerb, TakeBinding
3. parser.rs — parse_route, parse_reply, parse_fail
4. error.rs — add HttpResponse variant
5. value.rs — add value_to_json, json_to_value
6. interpreter.rs — handle Reply, Fail statements; expose from_environment constructor
7. route_loader.rs — split AST into routes + startup stmts
8. server.rs — axum server, route registration, execute_route
9. config.rs — marreta.env reader
10. main.rs — marreta serve command
11. Unit tests for each new component
12. Integration tests with real HTTP requests
```

Each step should compile and have tests before moving to the next.
