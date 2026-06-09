# 037 — Runtime Event Log Contract

> Status: Delivered
> Type: Runtime observability
> Scope: Canonical JSON event shapes for application logs, request events, consumer events, and runtime error summaries

Delivery notes:

- `5a1c406` — approved the runtime event log contract spec.
- `be87acf` — delivered canonical `kind` fields, consumer lifecycle events,
  and compact runtime error JSON events while preserving stderr traces.
- `76b856c` — aligned edge cases: schema rejection emits only a consumer event,
  `app_log` always uses `data`, and request runtime-error status is explicit.

---

## 1. Purpose

This spec defines the stable JSON event contract emitted by the MarretaLang
runtime in production-oriented execution.

The purpose is not to introduce a logging framework, log sinks, metrics,
OpenTelemetry exporters, custom spans, or a new MarretaLang namespace.

The purpose is narrower:

- make runtime-emitted JSON events distinguishable by `kind`
- formalize the shape of `log.*` output as `kind: "app_log"`
- preserve the delivered request log shape as `kind: "request"`
- add automatic async consumer runtime events as `kind: "consumer"`
- add compact runtime error summary events as `kind: "runtime_error"`
- keep these JSON events separate from Marreta-native stack trace output

Logs are part of the public v1 operational contract. Operators should be able
to ingest Marreta runtime output into grep, jq, Datadog, Elastic, Loki, Splunk,
or another log pipeline without reverse-engineering which component emitted a
line.

---

## 2. Relationship To Existing Specs

This spec consolidates and extends observability behavior from:

- `032_LOG_NAMESPACE.md` — application-authored `log.*`
- `033_REQUEST_LOGGING.md` — automatic HTTP request logs
- `035_W3C_TRACE_CONTEXT.md` — `trace_id` / `span_id` fields in logs
- `036_ASYNC_TRACE_PROPAGATION.md` — trace context in queue consumers

This spec deliberately does **not** replace:

- `007_ERROR_HANDLING.md`
- `022_RUNTIME_ERROR_HARDENING.md`
- `022b_TRACE_PERF_AND_ERGONOMICS.md`

Those specs own Marreta-native error identity and uncaught stack trace
diagnostics.

In particular:

- Marreta-native stack traces remain stderr diagnostics.
- Stack trace frames are not serialized into JSON event logs in this spec.
- `runtime_error` events are compact summaries, not full traces.
- HTTP response error envelopes are not changed by this spec.
- `raise`, `rescue`, `fail`, and `reply` semantics are not changed by this spec.

The boundary is:

```text
stdout JSON events  -> machine-ingestable operational events
stderr traces       -> Marreta-native developer diagnostics for uncaught failures
HTTP error bodies   -> client-facing response envelopes
```

This prevents collision with the delivered stack-trace work while giving
operators a consistent log contract.

When the same failure produces output in multiple channels, each channel owns a
different audience:

- stdout JSON is for log aggregators and production operations.
- stderr text is for human debugging and source-level diagnostics.
- HTTP bodies are for clients.

The runtime may emit to multiple channels for the same failure when
appropriate. None of these channels is the canonical representation of the
others.

---

## 3. Design Principles

1. Every JSON event emitted by the runtime should have a stable `kind`.
2. Runtime events and application-authored logs should be distinguishable
   without inspecting arbitrary payload fields.
3. The runtime should not log sensitive request bodies, headers, queue payloads,
   or auth material by default.
4. Trace fields should be attached when an active W3C trace context exists.
5. Error summary events should point to the same failure as stderr traces, but
   should not duplicate stack trace frames.
6. Existing toggles should remain meaningful; new toggles should only be added
   when they solve a proven operational need.
7. The first cut should preserve JSON Lines on stdout.
8. The first cut should not add in-language log configuration.

---

## 4. Canonical Event Envelope

Every runtime JSON event emitted to stdout should be a single JSON object on
one line.

Common fields:

- `timestamp`
- `kind`
- optional `trace_id`
- optional `span_id`

Field semantics:

- `timestamp`: UTC timestamp in the same canonical runtime format already used
  by request logs and `log.*`.
- `kind`: stable event discriminator.
- `trace_id`: 32 lowercase hex characters when an active trace context exists.
- `span_id`: 16 lowercase hex characters when an active trace context exists.

JSON object field order is not part of the public contract. Consumers should
parse events by field name, not by serialized order.

The first-cut `kind` values are:

- `app_log`
- `request`
- `consumer`
- `runtime_error`

No other runtime JSON event kinds should be emitted without a dedicated spec or
an explicit amendment to this one.

---

## 5. `kind: "app_log"`

Application-authored `log.*` output should use:

```json
{
  "timestamp": "2026-05-07T01:06:00Z",
  "kind": "app_log",
  "level": "info",
  "trace_id": "0af7651916cd43dd8448eb211c80319c",
  "span_id": "b7ad6b7169203331",
  "data": {
    "event": "order.created",
    "order_id": 42
  }
}
```

Required fields:

- `timestamp`
- `kind`
- `level`
- `data`

Optional fields:

- `trace_id`
- `span_id`

Field semantics:

- `kind`: always `"app_log"`.
- `level`: one of `"debug"`, `"info"`, `"warn"`, `"error"`.
- `data`: the value passed by user code to `log.*`, represented using the
  existing stable logging serialization.

This is a formalization of the delivered `032_LOG_NAMESPACE.md` behavior. It
does not add a new user-facing logging API.

## 5.1 Compatibility Note

Before this spec, `log.*` events did not include `kind`.

For v1, `kind: "app_log"` should be treated as the stable public contract.
During pre-v1 development, tests should be updated to assert fields rather than
compare full JSON strings or rely on object key order.

Existing fields keep their names, types, and semantics:

- `timestamp`
- `level`
- `data`
- `trace_id`
- `span_id`

Only the addition of `kind: "app_log"` is new.

---

## 6. `kind: "request"`

HTTP request runtime events remain the automatic access log introduced by
`033_REQUEST_LOGGING.md`.

Example:

```json
{
  "timestamp": "2026-05-07T01:06:00.604Z",
  "kind": "request",
  "trace_id": "11111111111111111111111111111111",
  "span_id": "a56ee3dcb356e05a",
  "method": "GET",
  "path": "/trace-context/stub/headers",
  "route": "/trace-context/stub/headers",
  "status": 200,
  "duration_ms": 0.197885
}
```

Required fields:

- `timestamp`
- `kind`
- `method`
- `path`
- `status`
- `duration_ms`

Optional fields:

- `trace_id`
- `span_id`
- `route`

Field semantics remain as defined in `033_REQUEST_LOGGING.md` and
`035_W3C_TRACE_CONTEXT.md`.

This spec extends `MARRETA_REQUEST_LOG` to also gate `kind: "consumer"` events
as described in Section 7.3. It does not change request-event behavior.

When `MARRETA_REQUEST_LOG=false`, `request` events are not emitted.

When `MARRETA_TRACE_CONTEXT=false`, `request` events may still be emitted, but
should omit `trace_id` and `span_id`.

---

## 7. `kind: "consumer"`

The runtime should emit one automatic consumer event per processed queue/topic
delivery.

This is the async equivalent of request logging. It gives operators visibility
into consumer execution even when user code does not call `log.*`.

Example:

```json
{
  "timestamp": "2026-05-07T01:06:00.700Z",
  "kind": "consumer",
  "trace_id": "11111111111111111111111111111111",
  "span_id": "985bca1ab89e1d30",
  "consumer_kind": "queue",
  "target": "ft.async.trace",
  "routing_key": "",
  "delivery_attempt": 1,
  "status": "ack",
  "duration_ms": 12.4
}
```

Topic example:

```json
{
  "timestamp": "2026-05-07T01:06:00.715Z",
  "kind": "consumer",
  "trace_id": "33333333333333333333333333333333",
  "span_id": "4a7a274849b981a1",
  "consumer_kind": "topic",
  "target": "ft.async.trace.created",
  "routing_key": "ft.async.trace.created",
  "status": "ack",
  "duration_ms": 8.1
}
```

Required fields:

- `timestamp`
- `kind`
- `consumer_kind`
- `target`
- `status`
- `duration_ms`

Optional fields:

- `trace_id`
- `span_id`
- `routing_key`
- `exchange`
- `delivery_attempt`

Field semantics:

- `kind`: always `"consumer"`.
- `consumer_kind`: `"queue"` or `"topic"`.
- `target`: declared consumer target from Marreta source.
  - For `on queue "orders"`, target is `"orders"`.
  - For `on topic "orders.created"`, target is `"orders.created"`.
- `routing_key`: broker routing key when available.
  - For topic consumers this is usually the actual published topic.
  - For point-to-point queues it may be empty or absent depending on driver.
- `exchange`: broker exchange when available.
- `delivery_attempt`: broker receive attempt count when available.
  - Drivers that cannot provide this reliably should omit it.
  - Examples: RabbitMQ quorum `x-delivery-count`, SQS
    `ApproximateReceiveCount`, or a future retry driver counter.
- `status`: processing outcome.
- `duration_ms`: total handler execution duration measured by the runtime.

For topic consumers, `target` and `routing_key` usually match because Marreta
topics are exact strings:

```text
on topic "orders.created" receives "orders.created"

target:      "orders.created" # declared exact topic
routing_key: "orders.created" # actual delivered routing key
```

## 7.1 Consumer Status Values

First-cut status values:

- `"ack"`
- `"nack"`
- `"nack_requeue"`
- `"error"`
- `"schema_rejected"`

Semantics:

- `ack`: handler completed successfully and delivery was acknowledged.
- `nack`: handler explicitly rejected without requeue.
- `nack_requeue`: handler explicitly rejected with requeue.
- `error`: handler raised an uncaught runtime error and runtime rejected the
  delivery without requeue.
- `schema_rejected`: consumer schema validation failed before handler
  execution and runtime rejected the delivery without requeue.

The consumer event should be emitted after the final ack/nack decision returns.
For drivers where acknowledgement is asynchronous, the event should describe
the final decision made by the runtime, not an earlier handler return.

## 7.2 Consumer Payload Policy

Consumer runtime events must not include queue payloads by default.

Payload data belongs in application-authored `log.*` calls when the developer
chooses to emit it.

This avoids leaking sensitive message contents and prevents high-cardinality
payloads from becoming part of the runtime log contract.

## 7.3 Consumer Toggle

The first cut should not introduce a separate consumer log toggle.

Consumer runtime events are automatic runtime lifecycle events, like request
events.

`MARRETA_REQUEST_LOG` should control both first-cut lifecycle event kinds:

- `kind: "request"`
- `kind: "consumer"`

When `MARRETA_REQUEST_LOG=true`:

- handled HTTP requests emit `request` events
- processed queue/topic deliveries emit `consumer` events

When `MARRETA_REQUEST_LOG=false`:

- handled HTTP requests do not emit `request` events
- processed queue/topic deliveries do not emit `consumer` events

Rationale:

- the historical env var name is request-oriented, but its practical role is
  controlling automatic runtime lifecycle logs
- operators need one simple switch to disable automatic runtime noise
- adding `MARRETA_CONSUMER_LOG` now would add ceremony without enough benefit
- a future `MARRETA_RUNTIME_EVENTS` alias can be introduced if the runtime grows
  more automatic lifecycle event kinds after v1

This toggle does not affect:

- `kind: "app_log"` emitted by user code through `log.*`
- `kind: "runtime_error"` summaries for uncaught runtime failures
- stderr Marreta-native diagnostics and stack traces

---

## 8. `kind: "runtime_error"`

The runtime should emit compact JSON error summary events when an uncaught
runtime failure crosses a runtime boundary.

Runtime boundaries include:

- HTTP route execution
- queue/topic consumer execution
- startup/runtime bootstrap when representable as a JSON event

Example for HTTP route failure:

```json
{
  "timestamp": "2026-05-07T01:06:00.900Z",
  "kind": "runtime_error",
  "trace_id": "0af7651916cd43dd8448eb211c80319c",
  "span_id": "b7ad6b7169203331",
  "scope": "request",
  "error_code": "db_error",
  "operation": "db.query",
  "message": "Database operation failed",
  "http_status": 500
}
```

Example for consumer failure:

```json
{
  "timestamp": "2026-05-07T01:06:01.100Z",
  "kind": "runtime_error",
  "trace_id": "11111111111111111111111111111111",
  "span_id": "985bca1ab89e1d30",
  "scope": "consumer",
  "consumer_kind": "queue",
  "target": "ft.orders",
  "error_code": "type_error",
  "operation": "interpreter",
  "message": "Property 'id' not found",
  "consumer_status": "error"
}
```

Required fields:

- `timestamp`
- `kind`
- `scope`
- `error_code`
- `message`

Optional fields:

- `trace_id`
- `span_id`
- `operation`
- `http_status`
- `consumer_kind`
- `target`
- `consumer_status`

Field semantics:

- `kind`: always `"runtime_error"`.
- `scope`: runtime boundary where the failure escaped.
  - `"request"`
  - `"consumer"`
  - `"startup"`
- `error_code`: stable Marreta error code from the existing error identity
  system.
- `operation`: existing trace-oriented operation label when available.
- `message`: Marreta-facing error message, with no Rust internals.
- `http_status`: final HTTP status when scope is `"request"`.
- `consumer_status`: final consumer status when scope is `"consumer"`.

Typical `error_code` scopes:

| error_code | Typical scope | Notes |
| --- | --- | --- |
| `schema_error` | `request`, `consumer` | Input validation failed before user handler logic completed. |
| `type_error` | `request`, `consumer`, `startup` | Interpreter type mismatch or missing property. |
| `db_error` | `request`, `consumer`, `startup` | Database operation failed. |
| `http_client_error` | `request`, `consumer`, `startup` | Outbound HTTP operation failed. |
| `queue_error` | `request`, `consumer`, `startup` | Queue publish, consume, ack, or nack operation failed. |
| `runtime_error` | `request`, `consumer`, `startup` | Fallback for uncaught runtime failures without a more specific stable code. |

## 8.1 Runtime Error Events Are Not Stack Traces

`runtime_error` events must not include:

- stack frames
- Rust panic locations
- Rust crate paths
- raw driver error types
- source code snippets
- full Marreta trace text

The existing stderr trace output remains the owner of human-readable stack
diagnostics.

For the same failure, the runtime may emit:

```text
stdout: {"kind":"runtime_error", ...}
stderr: [marreta] uncaught runtime error ...
stderr: [marreta] trace:
stderr:   at route GET /orders ...
stderr:   at task validate ...
```

This is not duplication. The JSON event is for machines and aggregation; the
stderr trace is for humans and local/source-level debugging.

## 8.2 Handled Errors

Handled `rescue` flows should not emit `runtime_error` events.

If user code catches an error and decides to log it, that is an `app_log`
event emitted by `log.*`.

This preserves the `022_RUNTIME_ERROR_HARDENING.md` rule that handled errors
do not emit top-level uncaught runtime traces.

## 8.3 Authored Responses

Authored `reply` and `fail` responses should not automatically emit
`runtime_error` events.

Rationale:

- `reply` is normal control flow.
- `fail` is an authored HTTP response contract.
- Treating authored `fail 4xx` as runtime errors would inflate error streams
  and blur application decisions with runtime failures.

Request logs still capture the final HTTP status.

---

## 9. Output Streams

First-cut output streams:

- stdout: JSON event logs
- stderr: Marreta-native uncaught error diagnostics and traces

This matches existing container/runtime expectations:

- stdout can be ingested as JSON Lines
- stderr remains suitable for diagnostics, process failures, startup failures,
  and local debugging

The first cut should not add:

- file log sinks
- log rotation
- syslog
- remote exporters
- configurable JSON layouts
- text-mode access logs

---

## 10. Trace Field Policy

Trace fields follow `035_W3C_TRACE_CONTEXT.md` and
`036_ASYNC_TRACE_PROPAGATION.md`.

When active trace context exists:

- `trace_id` should be included
- `span_id` should be included

When no active trace context exists:

- `trace_id` should be absent
- `span_id` should be absent

The runtime should not emit raw `traceparent` or `tracestate` fields in event
logs.

For consumer events, if `MARRETA_TRACE_CONTEXT=true`, the consumer path should
have an active trace context because 036 creates a continued or orphan root
context. Therefore consumer events should include trace fields when trace
context is enabled.

When `MARRETA_TRACE_CONTEXT=false`, consumer events should still be emitted but
should omit trace fields.

---

## 11. Sensitive Data Policy

Runtime event logs must not include sensitive data by default.

Specifically:

- request events must not include request body
- request events must not include raw headers
- request events must not include query strings by default
- consumer events must not include message payload
- consumer events must not include raw transport metadata
- runtime error events must not include raw secrets, auth tokens, or driver
  internals

Application code may explicitly log sensitive data via `log.*`; that remains a
developer responsibility. The runtime should not add sensitive data
automatically.

---

## 12. Backward Compatibility

This spec intentionally changes the pre-v1 `log.*` JSON shape by adding
`kind: "app_log"`.

Because MarretaLang is still pre-v1, this is acceptable and should happen
before the public v1 contract freezes.

Expected test migration:

- stop comparing full JSON log strings
- parse JSON or assert field presence
- treat object key order as non-contractual

Existing request log consumers should continue to work because
`kind: "request"` already exists.

---

## 13. Functional Validation

Functional validation should live in `examples/functional_tests`.

Required checks:

### App log

- `log.info({ event: "x" })` emits `kind: "app_log"`
- `log.info("x")` emits `data: "x"`
- emitted event includes `level: "info"`
- emitted event preserves `data.event`
- emitted event includes trace fields inside HTTP request context

### Request event

- handled route emits `kind: "request"` when `MARRETA_REQUEST_LOG=true`
- request event includes method/path/status/duration
- request event includes trace fields when trace context is enabled
- request event omits trace fields when trace context is disabled

### Consumer event

- queue consumer success emits `kind: "consumer"` and `status: "ack"`
- explicit `nack` emits `status: "nack"`
- explicit `nack requeue` emits `status: "nack_requeue"`
- consumer schema rejection emits `status: "schema_rejected"`
- consumer runtime error emits `status: "error"`
- topic fan-out emits one consumer event per subscriber
- consumer events include trace fields when trace context is enabled

### Runtime error event

- uncaught route runtime error emits `kind: "runtime_error"`
- uncaught consumer runtime error emits `kind: "runtime_error"`
- handled `rescue` flow does not emit `runtime_error`
- authored `fail` does not emit `runtime_error`
- runtime error event does not include stack frames
- stderr still includes Marreta-native trace for uncaught failures

---

## 14. Non-Goals

This spec does not include:

- OpenTelemetry exporters
- metrics
- histograms
- custom spans
- sampling
- log sinks
- log routing
- configurable JSON layout
- log redaction framework
- raw request/response body logging
- queue payload logging
- serializing Marreta stack traces into JSON
- replacing stderr stack trace diagnostics
- changing HTTP error response bodies
- changing `raise` / `rescue` semantics

---

## 15. Watch Points

Known tensions to revisit only with concrete evidence:

- `MARRETA_REQUEST_LOG=false` disables both `request` and `consumer` events by
  design. If operators report needing independent toggles in practice, a future
  spec may introduce `MARRETA_CONSUMER_LOG` or rename/add a clearer master
  toggle such as `MARRETA_RUNTIME_EVENTS`.
- Whether `runtime_error` should include a stable `source` object with
  `.marreta` file/line without becoming a full stack trace.
- Whether startup failures should emit JSON `runtime_error` events or remain
  stderr-only until a broader process-event spec exists.
- Whether log event field order should be standardized for readability. The
  machine contract should remain field-based, not order-based.

---

## 16. Implementation Notes

Implementation should be staged to avoid breaking existing behavior
accidentally:

1. Add `kind: "app_log"` to `log.*` output.
2. Preserve existing request event shape.
3. Add consumer event emission around the queue handler lifecycle.
4. Add compact runtime error events at uncaught runtime boundaries.
5. Keep existing stderr trace output unchanged.
6. Update tests to assert JSON fields, not full string equality.

Consumer event timing should wrap:

- schema validation
- handler execution
- final ack/nack decision

The event should be emitted after the final ack/nack decision returns.

Runtime error event emission should happen at the same boundaries that already
call Marreta-native uncaught error logging, but should not replace those calls.
