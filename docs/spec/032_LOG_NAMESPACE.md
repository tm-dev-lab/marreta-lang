# 032 — Log Namespace

> Status: Delivered
> Type: Native namespace
> Scope: Small native logging surface for backend/API observability

---

## 1. Purpose

This spec introduces a small native `log` namespace for MarretaLang.

The purpose of `log` is not to expose a full logging framework with appenders,
formatters, transports, or log routing policy inside application code.

The purpose is narrower:

- emit structured or textual runtime logs from routes, tasks, and consumers
- support ordinary backend/API observability flows
- keep logging explicit and low-ceremony in business logic

Examples of legitimate use cases:

- request debugging during development
- operational audit breadcrumbs
- queue/consumer execution tracing
- external integration diagnostics
- temporary business-event observability without ad hoc `print(...)`

---

## 2. Why `log` Matters

In real API/backend systems, logging is one of the most common operational
needs.

Without a native logging surface, developers tend to fall back to:

- `print(...)` for production-like diagnostics
- incidental filesystem writes
- ad hoc queue/doc persistence only to get observability

That creates the wrong abstraction boundary.

`log` should exist as an explicit observability primitive, just like `json`,
`fs`, and `base64` exist as explicit data-boundary primitives.

---

## 3. Design Principles

The `log` namespace must follow these rules:

1. It should stay very small.
2. It should be explicit, not magical.
3. It should prefer structured logging when values are already native.
4. It should work naturally in normal calls, pipelines, and broadcasts.
5. It should not expose logger transport configuration inside language code.
6. It should not turn logging into a second persistence mechanism.

---

## 4. Delivered Surface

The delivered surface is:

```marreta
log.info(value)
log.warn(value)
log.error(value)
log.debug(value)
```

This is the complete first-cut surface.

---

## 5. Function Purposes

## 5.1 `log.info(value)`

Purpose:

- emit informational logs for expected application flow

## 5.2 `log.warn(value)`

Purpose:

- emit warning logs for degraded or suspicious flow that does not abort

## 5.3 `log.error(value)`

Purpose:

- emit error logs for failures or conditions close to failure

## 5.4 `log.debug(value)`

Purpose:

- emit lower-priority diagnostics for development and troubleshooting

---

## 6. Semantics

## 6.1 Input types

All first-cut functions accept any runtime value that can be represented
stably for logging.

The runtime should accept:

- `string`
- `integer`
- `float`
- `boolean`
- `null`
- `list`
- `map`
- temporal values
- relational/document payload values already visible to user code

The runtime should reject values that do not have a stable logging
representation, such as namespace marker values.

## 6.2 Return value

All first-cut logging calls should return the original input value unchanged.

That means:

```marreta
payload = log.info(payload)
```

is valid, and:

```marreta
payload >> log.info() >> queue.push("events")
```

should preserve pipeline flow.

This makes `log.*` behave as a tap/pass-through stage.

## 6.3 Structured-first behavior

If the input is already a native structured value, logging should preserve that
structure at the runtime boundary instead of forcing premature stringification
in user code.

This means the developer should be able to write:

```marreta
log.info({
    order_id: order.id,
    customer_id: order.customer_id,
    status: "created"
})
```

without converting the payload manually to text first.

## 6.4 String logging

Plain string logging remains valid and important:

```marreta
log.warn("retrying upstream request")
```

String interpolation should continue to use the language's normal interpolation
syntax:

```marreta
log.info("loading order #{id}")
```

The `log` namespace should not introduce printf-style or placeholder-style
message formatting APIs in the first cut.

Logged strings must be treated as inert data. The runtime must not interpret
log payloads as templates, must not expand placeholders beyond Marreta's normal
string interpolation that already happened before the call, and must not
perform remote lookups or dynamic resolution based on log content.

## 6.5 Structured context

Structured logging context should be passed as ordinary native values,
especially `map` values:

```marreta
log.info({
    message: "order created",
    order_id: order.id,
    customer_id: order.customer_id
})
```

The first cut should not introduce alternate signatures such as
`log.info(message, fields)` or variadic named logging fields.

Formatting should remain the responsibility of existing type-oriented
surfaces, not the `log` namespace itself. For example:

```marreta
log.info("billing date #{time.format(billing_date, \"dd/MM/yyyy\")}")
```

```marreta
payload >> json.pretty() >> log.debug()
```

## 6.6 Pipeline behavior

The namespace should work naturally in pipelines:

```marreta
payload >> log.info() >> queue.push("audit")
```

```marreta
response.body >> log.debug() >> cache.set("last_response")
```

## 6.7 Broadcast behavior

The namespace should also work naturally in broadcasts:

```marreta
payload *>>
    -> log.info()
    -> log.debug()
```

```marreta
orders *>>
    -> log.info()
    -> queue.push("audit")
```

Each broadcast branch should receive the same input value, emit its own log
event if the branch contains a logging call, and return the original input
unchanged.

Multiple log events for the same source input are expected behavior inside a
broadcast, not a runtime error.

## 6.8 Runtime destination

The first cut should treat log emission as runtime-managed output.

Application code must not configure sinks, appenders, file targets, or
transport routing through the `log` namespace.

Those concerns belong to runtime configuration or host process behavior, not to
route/task/business code.

## 6.9 Log level configuration

The effective application log level should be configured by runtime
environment, not inside MarretaLang source code.

The first cut should use an environment variable:

- `MARRETA_LOG_LEVEL`

Expected values:

- `debug`
- `info`
- `warn`
- `error`

If absent, the runtime may choose a sensible default, but that default must be
documented by the implementation.

## 6.10 Output stream

The first cut should emit all log events to `stdout`.

This keeps the runtime aligned with modern container/backend deployment models,
where log collection is normally handled by the host platform.

The language must not expose file-target or sink-selection APIs in the first
cut.

## 6.11 Output shape

The first cut should emit logs as **JSON Lines** on `stdout`.

That means:

- one log event per line
- each line is a valid JSON object

The minimum output contract should include:

- `timestamp`
- `level`
- `message` for string input
- `data` for structured non-string input

Examples:

```marreta
log.info("starting sync")
```

should emit a line shaped like:

```json
{"timestamp":"2026-05-01T12:34:56Z","level":"info","message":"starting sync"}
```

```marreta
log.error({
    event: "payment.failed",
    order_id: 99,
    reason: "gateway_timeout"
})
```

should emit a line shaped like:

```json
{"timestamp":"2026-05-01T12:34:56Z","level":"error","data":{"event":"payment.failed","order_id":99,"reason":"gateway_timeout"}}
```

The first cut should not introduce alternate output formats.

---

## 7. Error Behavior

The namespace should fail clearly for unsupported values.

Expected outcomes:

- unsupported runtime value -> `type_error`

The logging surface must not silently swallow obviously invalid logging inputs
if they cannot be represented safely.

## 7.1 Error table

| Operation | Invalid case | Expected failure |
|---|---|---|
| `log.info(value)` | unsupported runtime value | `type_error` |
| `log.warn(value)` | unsupported runtime value | `type_error` |
| `log.error(value)` | unsupported runtime value | `type_error` |
| `log.debug(value)` | unsupported runtime value | `type_error` |

---

## 8. What Does Not Belong

The following should stay out of the initial `log` surface:

- named logger instances
- custom formatting DSLs
- per-call sink selection
- file-path logging APIs
- rotation settings
- structured field builders separate from normal maps
- tracing/span APIs
- correlation-id propagation policy
- sampling controls
- file sink selection
- in-code log-level configuration
- alternate runtime log output formats
- printf-style formatting helpers
- alternate `message + fields` logging signatures

These would expand the namespace too far for the first cut.

---

## 9. Examples

## 9.1 Simple route logging

```marreta
route GET "/orders/:id"
    log.info("loading order #{id}")
    order = db.orders.find(id)
    reply 200, order
```

## 9.2 Structured payload logging

```marreta
log.info({
    event: "customer.created",
    customer_id: customer.id,
    created_at: time.now()
})
```

## 9.3 Pipeline tap behavior

```marreta
payload
    >> log.debug()
    >> json.stringify()
    >> fs.write("debug.json")
```

## 9.4 Broadcast diagnostics

```marreta
payload *>>
    -> log.info()
    -> log.warn()
```

## 9.5 Queue consumer diagnostics

```marreta
on queue "orders" take msg
    msg >> log.info()
    process_order(msg)
```

---

## 10. Recommendation

Current recommendation:

1. keep `log` extremely small
2. make all log calls pass-through
3. support structured values directly
4. keep runtime sink/level policy out of language code
5. use JSON Lines on `stdout` as the default runtime contract
6. avoid inventing tracing/span/config APIs too early

The best first implementation target is:

- `log.info`
- `log.warn`
- `log.error`
- `log.debug`

---

## 11. Implementation Plan

Suggested implementation order:

### Phase 1

- reserve the `log` namespace
- close supported value, runtime level policy, and stdout emission policy

### Phase 2

- add parser/runtime namespace support
- add unit tests for pass-through semantics, broadcast semantics, and failure
  cases

### Phase 3

- add functional coverage in `examples/functional_tests`
- validate interaction with:
  - routes
  - tasks
  - queue consumers
  - pipelines
  - broadcasts
  - `json`
  - `fs`
- review `docs/vscode-marreta` for namespace highlighting, reserved-word
  treatment, and any syntax updates needed by the delivered `log` surface

---

## 12. Test Plan

Implementation of this spec must include:

1. unit tests for `log.info`
2. unit tests for `log.warn`
3. unit tests for `log.error`
4. unit tests for `log.debug`
5. tests proving pass-through return semantics
6. functional tests in `examples/functional_tests`
7. integration checks with route handlers, tasks, queue consumers, and
   pipelines
8. integration checks with broadcasts (`*>>`)
9. a delivery-time review of `docs/vscode-marreta` to determine whether
   highlighting or indentation updates are needed for the `log` namespace
