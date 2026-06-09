# Implementation Plan — v0.8 Queue Module

**Status:** ✅ Complete — all phases shipped in v0.8.0.

## Overview

v0.8 introduces a message queue integration layer to MarretaLang. The goal is to
allow routes to **produce** messages and dedicated **`on`** handlers to **consume**
them — using the same ergonomics the language already established for HTTP routes
and DB/Doc operations.

The initial provider target is **RabbitMQ**. The architecture mirrors the
`db.*`/`doc.*` pattern: the provider is declared via environment variables, keeping
application code broker-agnostic.

---

## Syntax Reference

### Consumer — `on queue` (point-to-point)

```marreta
on queue "orders.processing" take message
    # message is a free-form map
    order_id = message.order_id
    # implicit ack on clean exit

on queue "orders.processing" take message as order_payload
    # message validated against order_payload schema
    # schema mismatch → automatic nack (no requeue)
```

### Consumer — `on topic` (pub/sub)

```marreta
on topic "payments.approved" take message
    # subscribes to exactly this topic
    print("payment received:", message.order_id)

on topic "orders.created" take message as order_event
    # validated against order_event schema
    print("order created:", message.order_id)
```

Topics are exact strings — no wildcards. To consume multiple topics, declare
multiple `on topic` handlers. The driver maps each topic to the broker's native
pub/sub primitive (RabbitMQ: exact routing key match on the shared exchange).

### Explicit nack

```marreta
on queue "orders.processing" take message as order_payload
    require message.order_id else nack          # discard, no requeue
    require message.amount > 0 else nack requeue  # requeue for retry
```

### Producer — `queue.push` (point-to-point)

```marreta
queue.push "orders.processing", { order_id: 123, amount: 49.90 }
queue.push "orders.processing" as order_payload, { order_id: 123, amount: 49.90 }
```

### Producer — `topic.publish` (pub/sub)

```marreta
topic.publish "payments.approved", { order_id: 123 }
topic.publish "payments.approved" as payment_event, { order_id: 123 }
```

The topic is a dot-separated string. The driver translates it to the broker's
native pub/sub primitive — on RabbitMQ this becomes a routing key on a shared
durable topic exchange (`marreta.topics` by default, configurable via
`MARRETA_TOPIC_EXCHANGE`).

### Pipeline support

Both `queue.push` and `topic.publish` **return the published value** (after
schema filtering, if any), making them pipeline-friendly. When the payload is
omitted, it is received from the left side of `>>`:

```marreta
db.orders.find(id) >> queue.push "invoices" >> reply 200
payload >> topic.publish "order.created" >> reply 201
```

### Schema behavior summary

| Context | Without `as` | With `as` |
|---|---|---|
| `on queue/topic` consumer | accepts any JSON map | validates on arrival; mismatch → nack (no requeue) |
| `queue.push` producer | sends payload as-is | strips fields not in schema before sending |
| `topic.publish` producer | sends payload as-is | strips fields not in schema before sending |

---

## Environment Variables

```
MARRETA_QUEUE_PROVIDER=rabbitmq                    # required if queue.* / on queue / on topic is used
MARRETA_QUEUE_HOST=queue.internal                  # required
MARRETA_QUEUE_PORT=5672                            # optional, defaults to 5672
MARRETA_QUEUE_USER=app_user                        # required
MARRETA_QUEUE_PASSWORD=secret                      # required when broker auth is enabled
MARRETA_QUEUE_VHOST=/                              # optional, advanced; default /
MARRETA_QUEUE_PREFETCH=10                          # optional, default 10
MARRETA_QUEUE_RECONNECT_MAX_RETRIES=10             # optional, default 10
MARRETA_TOPIC_EXCHANGE=marreta.topics              # optional, default marreta.topics (RabbitMQ only)
```

Same pattern as `MARRETA_DB_*` and `MARRETA_DOC_*`. If `MARRETA_QUEUE_PROVIDER`
is not set, queue operations at startup emit a warning and any `on`/`queue.*`
call at runtime returns a clear error:

```
Queue provider not configured — set MARRETA_QUEUE_PROVIDER, MARRETA_QUEUE_HOST,
MARRETA_QUEUE_PORT, MARRETA_QUEUE_USER, and MARRETA_QUEUE_PASSWORD
```

---

## Ack / Nack Semantics

| Outcome | Behavior |
|---|---|
| Handler completes without error | Implicit `ack` |
| Handler calls `nack` | Nack, no requeue |
| Handler calls `nack requeue` | Nack, requeue message |
| Schema validation fails (`as`) | Nack, no requeue (malformed message — retry won't help) |
| Runtime error (`fail`, `raise`, panic) | Nack, requeue (transient failure — retry may succeed) |

This mirrors how Spring AMQP and Celery handle ack by default: success = ack,
exception = nack+requeue, making it transparent for the common case.

---

## AST Changes

### New statement variants

```rust
// on queue "name" take binding [as schema]
Statement::OnQueue {
    queue_name: Expression,      // allows dynamic names via variables
    binding: String,             // e.g. "message"
    schema: Option<String>,      // optional schema name for validation
    body: Vec<Statement>,
    line: usize,
    column: usize,
}

// on topic "pattern" take binding [as schema]
Statement::OnTopic {
    pattern: Expression,         // exact topic string, e.g. "orders.created"
    binding: String,
    schema: Option<String>,
    body: Vec<Statement>,
    line: usize,
    column: usize,
}

// nack / nack requeue
Statement::Nack {
    requeue: bool,
    line: usize,
    column: usize,
}
```

### New expression variants (producer)

```rust
// queue.push "name" [as schema], payload
Expression::QueuePush {
    queue_name: Box<Expression>,
    schema: Option<String>,
    payload: Box<Expression>,
}

// topic.publish "topic" [as schema], payload
Expression::TopicPublish {
    topic: Box<Expression>,
    schema: Option<String>,
    payload: Box<Expression>,
}
```

---

## Lexer / Parser Changes

### New keywords

| Token | Keyword |
|---|---|
| `Token::On` | `on` |
| `Token::Queue` | `queue` |
| `Token::Topic` | `topic` |
| `Token::Nack` | `nack` |
| `Token::Requeue` | `requeue` |

`queue` and `topic` are context keywords (only meaningful after `on` or as
`queue.push`/`topic.publish`). They must not shadow existing identifiers.

### Parser: `on` statement

```
on_stmt     = "on" ("queue" | "topic") string_expr "take" ident ["as" ident] NEWLINE INDENT body DEDENT
```

### Parser: `nack` statement

```
nack_stmt   = "nack" ["requeue"]
```

### Parser: `queue.*` expressions

```
queue_push    = "queue" "." "push" string_expr ["as" ident] "," expr
queue_publish = "queue" "." "publish" string_expr ["as" ident] "," expr
```

---

## Queue Module (`src/queue/`)

### File structure

```
src/queue/
  mod.rs          — public API, QueueConfig, trait QueueDriver
  driver.rs       — trait definition (same pattern as src/db/driver.rs)
  rabbitmq.rs     — RabbitMQ implementation via lapin crate
  pool.rs         — connection pool (QueuePool, mirrors DbPool/DocPool)
```

### `QueueDriver` trait

```rust
#[async_trait]
pub trait QueueDriver: Send + Sync {
    /// Publish a message to a named queue (point-to-point).
    async fn push(&self, queue: &str, payload: &Value) -> Result<(), QueueError>;

    /// Publish a message to a topic (pub/sub, provider-agnostic).
    /// The topic is a dot-separated string (e.g. "payments.approved").
    /// The driver translates it to the broker's native pub/sub primitive.
    async fn publish(&self, topic: &str, payload: &Value) -> Result<(), QueueError>;

    /// Prepare to consume an exact topic. Returns an opaque handle for consume_topic.
    /// Topics are exact dot-separated strings — no wildcards.
    async fn bind_topic(&self, topic: &str) -> Result<String, QueueError>;

    /// Start consuming from a named queue.
    async fn consume_queue(&self, queue: &str) -> Result<BoxStream<'static, QueueDelivery>, QueueError>;

    /// Start consuming from a topic binding (handle from bind_topic).
    async fn consume_topic(&self, handle: &str) -> Result<BoxStream<'static, QueueDelivery>, QueueError>;

    /// Ack and nack are delivery-owned — call delivery.ack() / delivery.nack(requeue).
    /// These trait methods exist for API completeness only.
    async fn ack(&self, tag: u64) -> Result<(), QueueError>;
    async fn nack(&self, tag: u64, requeue: bool) -> Result<(), QueueError>;
}
```

### `QueueDelivery`

```rust
pub struct QueueDelivery {
    pub tag: u64,
    pub payload: Value,          // deserialized JSON
    pub routing_key: String,     // for topic consumers: the exact topic the message was sent to
    pub exchange: String,        // broker-internal; not exposed to handler code
    // ack_tx is internal — call delivery.ack() or delivery.nack(requeue)
}
```

Inside a topic handler, `message_topic` is injected as a string variable holding
the exact topic the message was published with (e.g. `"payments.approved"`).

### `QueueError`

```rust
pub enum QueueError {
    ConnectionFailed(String),
    PublishFailed(String),
    ConsumeFailed(String),
    AckFailed(String),
    SerializationError(String),
}
```

---

## RabbitMQ Implementation (`src/queue/rabbitmq.rs`)

Uses the `lapin` crate (async, tokio-native).

### Connection strategy

- Single connection, multiple channels (one channel per consumer, one shared
  channel for producers) — same pattern as connection pooling in `src/db/`.
- Reconnection: exponential backoff on connection loss, configurable via
  `MARRETA_QUEUE_RECONNECT_MAX_RETRIES` (default: 10).

### Auto-declare at startup

RabbitMQ supports idempotent declaration — declaring a queue or exchange that
already exists with the same parameters is a no-op. MarretaLang uses this to
auto-create all required infrastructure at startup, requiring zero manual broker
configuration for the common case.

**`on queue` handlers** — at startup, before spawning the consumer task:

```
queue_declare(name, durable=true, exclusive=false, auto_delete=false)
```

Safe to call on every restart. If the queue already exists with the same
parameters, RabbitMQ confirms without error. If it exists with different
parameters (e.g. someone manually created it as non-durable), RabbitMQ returns
a channel error — the server logs a clear message and exits.

**`on topic` handlers** — at startup:

```
# At connect(): shared exchange declared once (idempotent)
exchange_declare("marreta.topics", kind=topic, durable=true, auto_delete=false)

# Per consumer: server-named exclusive queue bound to shared exchange
queue_declare(server_generated_name, exclusive=true, auto_delete=true)
queue_bind(queue, "marreta.topics", topic)  # exact topic string as routing key
```

All pub/sub goes through a single shared topic exchange (`marreta.topics`).
The exclusive queue is per-process and auto-deleted when the consumer disconnects
— standard fan-out behavior for pub/sub. On reconnect, the binding is
automatically recreated.

The exchange name is configurable via `MARRETA_TOPIC_EXCHANGE`.

**Durability defaults:**

| Resource | Default | Rationale |
|---|---|---|
| Named queues (`on queue`) | `durable=true` | Survives broker restart; messages not lost |
| Topic exchanges (`on topic`) | `durable=true` | Exchange survives restart |
| Topic subscriber queues | `exclusive=true, auto_delete=true` | Ephemeral per-process, no leak on crash |

### QoS / Prefetch

By default MarretaLang sets `prefetch_count=10` per consumer channel. This
prevents a slow consumer from buffering unlimited unacked messages in memory.

Configurable via environment variable:

```
MARRETA_QUEUE_PREFETCH=10   # default
MARRETA_QUEUE_PREFETCH=1    # strict one-at-a-time (safest for ordered processing)
MARRETA_QUEUE_PREFETCH=0    # unlimited (not recommended)
```

### Exchange and binding for `on topic`

- Declares a `topic` exchange (durable).
- Creates a server-named exclusive queue bound to the exchange with the routing
  pattern.
- On disconnect the binding is recreated automatically.

### Message format

All messages are JSON. Content-type header set to `application/json`.

---

## Interpreter Changes (`src/interpreter.rs`)

### Startup: registering consumers

After loading the route registry, the interpreter walks all top-level statements
looking for `OnQueue` and `OnTopic`. For each one it spawns a Tokio task that:

1. Calls `driver.consume_queue(name)` or `driver.consume_topic(exchange, pattern)`
2. Iterates the delivery stream
3. For each delivery: validates schema if present, runs the handler body, acks or
   nacks based on outcome

```rust
async fn start_consumers(
    registry: &RouteRegistry,
    queue_pool: Arc<dyn QueueDriver>,
    interpreter_state: Arc<InterpreterState>,
) {
    for stmt in &registry.startup_stmts {
        match stmt {
            Statement::OnQueue { queue_name, binding, schema, body, .. } => {
                // spawn consumer task
            }
            Statement::OnTopic { pattern, binding, schema, body, .. } => {
                // spawn consumer task
            }
            _ => {}
        }
    }
}
```

### Schema validation on consume

Reuses the existing schema validation logic from the HTTP payload path
(`validate_payload`). On mismatch: `nack(requeue=false)` + log warning.

### `nack` statement execution

When `Statement::Nack { requeue }` is reached during handler execution, the
interpreter sets a sentinel on the execution context that causes the consumer
loop to call `driver.nack(tag, requeue)` instead of `driver.ack(tag)`.

### `queue.push` / `topic.publish` execution

Evaluated as expressions. If `as schema` is present, the payload is filtered
through the schema (same as `reply as` response filtering) before sending.
Returns `Value::Null` on success, raises a runtime error on failure.

---

## `RouteRegistry` Changes

`OnQueue` and `OnTopic` handlers need to be collected at load time alongside
routes. Two options:

1. Add `consumers: Vec<ConsumerDefinition>` to `RouteRegistry`
2. Store them in `startup_stmts` (already done for tasks/schemas) and walk at
   startup

**Decision: option 1** — explicit field makes the intent clear, mirrors
`routes: Vec<RouteDefinition>`, and allows OpenAPI-style documentation of
consumers in the future.

```rust
pub struct RouteRegistry {
    pub routes: Vec<RouteDefinition>,
    pub consumers: Vec<ConsumerDefinition>,   // NEW
    pub schemas: HashMap<String, SchemaDefinition>,
    pub startup_stmts: Vec<Statement>,
}

pub struct ConsumerDefinition {
    pub kind: ConsumerKind,          // Queue | Topic
    pub target: String,              // queue name or exact topic string
    pub binding: String,             // variable name inside handler
    pub schema: Option<String>,
    pub body: Vec<Statement>,
    pub source_file: Option<String>,
    pub line: usize,
    pub column: usize,
}

pub enum ConsumerKind {
    Queue,
    Topic,
}
```

---

## OpenAPI / Swagger Impact

`on queue` and `on topic` are not HTTP endpoints — they do not appear in the
OpenAPI paths. However, the `/openapi.json` spec should include an
`x-marreta-consumers` extension field documenting the registered consumers:

```json
{
  "x-marreta-consumers": [
    {
      "kind": "queue",
      "target": "orders.processing",
      "schema": "order_payload"
    },
    {
      "kind": "topic",
      "pattern": "payments.approved",
      "schema": "payment_event"
    }
  ]
}
```

This is informational only — Swagger UI ignores unknown extensions.

---

## `/health` Endpoint Impact

The built-in `/health` response should include queue connectivity status:

```json
{
  "ok": true,
  "api": "my-api",
  "version": "1.0.0",
  "db": "connected",
  "doc": "connected",
  "queue": "connected"
}
```

If the queue provider is not configured, `"queue": "not_configured"`.
If configured but disconnected, `"queue": "disconnected"` and `"ok": false`.

---

## `Cargo.toml` Dependencies

```toml
# Queue — RabbitMQ
lapin        = { version = "4", optional = true }
tokio-stream = { version = "0.1", optional = true }

[features]
queue = ["lapin", "tokio-stream"]
```

Feature-gated to keep the binary lean when queue is not needed. When
`MARRETA_QUEUE_PROVIDER` is set at runtime and the feature is not compiled in,
the server fails fast with a clear message.

---

## Example: `examples/queue/`

A self-contained example demonstrating the full producer/consumer cycle:

```
examples/queue/
  app.marreta        — entrypoint
  routes/api.marreta — HTTP routes that push to queues
  handlers/orders.marreta — on queue/topic handlers
  schemas/core.marreta    — shared schemas
  docker-compose.yml — rabbitmq + app
  seed.sh            — optional: seed test messages
  test.sh            — end-to-end test
```

### `routes/api.marreta`

```marreta
route POST "/orders" take payload as order_payload
    queue.push "orders.processing" as order_payload, payload
    reply 202, { queued: true, order_id: payload.order_id }

route POST "/payments/approve" take payload as payment_event
    topic.publish "payments.approved" as payment_event, payload
    reply 202, { queued: true }
```

### `handlers/orders.marreta`

```marreta
on queue "orders.processing" take message as order_payload
    # process order...
    result = db.table("orders").insert(message)

on topic "payments.approved" take message as payment_event
    # exact topic — message_topic holds the topic this message was published to
    order = db.table("orders").find(message.order_id)
    require order else nack
```

---

## Phases

### Phase 1 — AST & Parser ✅
- Add `Token::On`, `Token::Queue`, `Token::Topic`, `Token::Nack`, `Token::Requeue`
- Parse `on queue/topic` statements
- Parse `nack` / `nack requeue` statements
- Parse `queue.push` and `topic.publish` expressions
- Unit tests for all new syntax

### Phase 2 — Queue Module ✅
- `src/queue/mod.rs` — `QueueDriver` trait, `QueueConfig`, `QueueError`
- `src/queue/pool.rs` — `QueuePool` with reconnection logic
- `src/queue/rabbitmq.rs` — lapin-based implementation
- Unit tests with a mock driver

### Phase 3 — Route Loader & Registry ✅
- Add `ConsumerDefinition`, `ConsumerKind` to `route_loader.rs`
- Collect `OnQueue`/`OnTopic` into `registry.consumers` during load
- Update `file_loader.rs` to merge consumers across files
- Unit tests for multi-file consumer collection

### Phase 4 — Interpreter Integration ✅
- `start_consumers()` — spawn Tokio tasks per consumer at startup
- Schema validation on delivery
- Ack/nack logic tied to handler outcome
- `queue.push` / `topic.publish` expression evaluation
- `as schema` filtering on producer side
- Integration tests with mock driver

### Phase 5 — OpenAPI & Health ✅
- `x-marreta-consumers` extension in `/openapi.json`
- `"queue"` field in `/health` response

### Phase 6 — Example & Docs ✅
- `examples/queue/` standalone example with docker-compose (rabbitmq + app), routes, handlers, schemas, test.sh
- `examples/functional_tests/routes/queue.marreta` — new Section 28 covering:
  - `on queue` without schema (free-form message)
  - `on queue` with schema validation (valid and invalid messages)
  - `on topic` pub/sub handler (exact topic, with and without schema)
  - `queue.push` from a route (point-to-point)
  - `topic.publish` from a route (pub/sub)
  - `nack` and `nack requeue` explicit control
  - Producer `as schema` field filtering
- `examples/functional_tests/test.sh` — Section 28 tests (requires queue provider)
- `examples/functional_tests/docker-compose.yml` — add RabbitMQ service
- Update `SPEC.md` section 7 with final implemented syntax
- Update `CHANGELOG.md`

### Test Coverage Requirement

All phases must maintain a minimum of **80% unit test coverage** across `src/`.
This applies per-module:

- `src/queue/mod.rs`, `src/queue/pool.rs` — trait, config, error types: ≥ 80%
- `src/queue/rabbitmq.rs` — use a `MockQueueDriver` for unit tests; integration
  tests against a real broker are separate and do not count toward the 80% floor
- `src/lexer.rs`, `src/parser.rs` — new tokens and grammar rules must have unit
  tests covering happy path, error cases, and edge cases
- `src/interpreter.rs` — ack/nack logic, schema validation on delivery, producer
  expression evaluation: all branches covered
- `src/route_loader.rs`, `src/file_loader.rs` — `ConsumerDefinition` collection,
  multi-file merge: ≥ 80%

Coverage is measured via `cargo llvm-cov` (or equivalent). Each phase PR must
not reduce overall coverage below 80%. If a phase introduces new code that would
pull coverage below the threshold, additional unit tests are required before
merging.

---

## Design Watch Points

### 1. `on` vs existing identifier `on`

`on` is a common word. Verify it does not appear as a variable name in any
existing `.marreta` files or test fixtures. If conflicts arise, consider `consume`
as an alternative keyword.

### 2. Consumer lifecycle and graceful shutdown

When the server receives SIGTERM, in-flight consumer handlers must complete before
the process exits. The consumer tasks should be tracked and awaited during
shutdown — same pattern as graceful HTTP server shutdown.

### 3. `*>>` inside `on` handlers

Parallel broadcast inside a consumer handler is allowed (unlike inside
`transaction`). No special restriction needed.

### 4. Dynamic queue names

`on queue "orders.processing"` uses a string literal. Should dynamic names be
allowed (e.g. `on queue env.QUEUE_NAME`)? Decision: **yes, allow any expression**
— consistent with how route paths work. Evaluated at startup, not per-message.

### 5. Dead letter queues (DLQ)

Not in scope for v0.8. RabbitMQ DLX requires setting `x-dead-letter-exchange`
as a queue argument at declare time — this cannot be added to an existing queue
without deleting and recreating it.

For v0.8, `nack` without requeue simply discards the message. If the operator
configures a DLX externally on the queue, RabbitMQ will route nacked messages
there transparently — no MarretaLang changes needed.

A future version may add explicit DLQ support in the language:

```marreta
# hypothetical v0.9+ syntax
on queue "orders.processing" dlq "orders.failed" take message as order_payload
    # nack without requeue → routed to orders.failed automatically
```

This would require MarretaLang to pass `x-dead-letter-exchange` during
`queue_declare`, which means the queue name becomes part of the contract.
Design deferred to a future version.

### 6. Ordering guarantee

`on queue` consumers process one message at a time per handler declaration
(sequential within a consumer). Multiple `on queue` handlers for the same queue
name would create competing consumers — document this as the intended way to
scale throughput.
