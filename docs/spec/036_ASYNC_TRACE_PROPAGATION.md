# 036 — Async Trace Propagation

> Status: Delivered
> Type: Runtime observability
> Scope: W3C Trace Context propagation across queue producers and consumers

Delivery notes:

- `1d2f49d` — approved the async trace propagation spec.
- `c8791b3` — delivered trace propagation through `queue.push`,
  `queue.publish`, and queue/topic consumers.
- `f807501` — accepted UTF-8 AMQP byte-array trace headers for interop with
  non-Marreta producers.

---

## 1. Purpose

This spec extends the runtime-native W3C Trace Context support introduced in
035 to asynchronous queue flows.

The purpose is not to expose a queue tracing API in MarretaLang code, create
custom spans, or turn the queue runtime into an OpenTelemetry exporter.

The purpose is narrower:

- propagate active W3C trace context through `queue.push`
- propagate active W3C trace context through `queue.publish`
- restore trace context inside `on queue` and `on topic` consumers
- include `trace_id` and `span_id` in `log.*` output emitted by consumers
- propagate restored trace context through `http_client.*` calls made by
  consumers

This is a runtime/transport concern only. The first cut intentionally adds no
application-facing namespace, expression surface, or payload field.

---

## 2. Why Async Trace Propagation Matters

Spec 035 closes the synchronous correlation path:

```text
HTTP request -> request log -> log.* -> http_client.*
```

Real backend systems often continue work asynchronously:

```text
HTTP request -> queue.push / queue.publish -> on queue / on topic consumer
```

Without async propagation, the trace dies at the queue boundary. Operators can
see that a request accepted work and can see that a consumer later failed, but
cannot reliably join those events by trace.

The runtime should make this possible without application code boilerplate:

```json
{"timestamp":"2026-05-06T14:10:22.137Z","level":"info","trace_id":"0af7651916cd43dd8448eb211c80319c","span_id":"3d4b0f4f8a7c9912","data":{"event":"request.accepted","order_id":42}}
```

```json
{"timestamp":"2026-05-06T14:10:23.401Z","level":"info","trace_id":"0af7651916cd43dd8448eb211c80319c","span_id":"a4e17b2e8d7c1120","data":{"event":"consumer.started","order_id":42}}
```

The `trace_id` remains the cross-boundary join key. The `span_id` changes at
each logical runtime boundary.

---

## 3. Design Principles

Async trace propagation must follow these rules:

1. It should use W3C Trace Context as the only correlation mechanism.
2. It should be automatic when `MARRETA_TRACE_CONTEXT=true`.
3. It should not modify user payloads.
4. It should use transport metadata, not application data.
5. It should add no MarretaLang code surface in the first cut.
6. It should preserve schema contracts for queue messages.
7. It should keep queue driver abstractions provider-neutral.
8. It should remain compatible with future OpenTelemetry support.

---

## 4. Standard Contract

The first cut should propagate the same W3C fields used by 035:

- `traceparent`
- `tracestate`

In queue transports, these values are runtime metadata attached to the message.
They are not part of the MarretaLang payload.

Runtime and application logs should continue to use:

- `trace_id`
- `span_id`

The first cut should not introduce:

- `correlation.id`
- `request.id`
- `context.*`
- payload fields such as `_traceparent`, `traceparent`, or `trace_id`
- user-facing queue metadata accessors

This preserves the single-mechanism decision from 035 and avoids creating a
parallel queue-specific correlation contract.

---

## 5. Metadata-Only Rule

Trace context must be carried as transport metadata only.

The runtime must not inject trace data into user payloads.

Bad:

```json
{
  "order_id": 42,
  "_traceparent": "00-..."
}
```

Good:

```text
payload:  { "order_id": 42 }
metadata: { "traceparent": "00-...", "tracestate": "..." }
```

Rationale:

- message schemas should remain the source of truth for application payloads
- strict consumers should not reject messages because runtime fields appeared
- trace context is transport metadata, not domain data
- payload injection would turn observability into an application contract
- providers with no metadata support should not force MarretaLang to pollute
  payloads

Drivers that cannot carry transport metadata are simply unable to support async
trace propagation in the first cut.

---

## 6. Queue Driver Contract

Queue drivers should support provider-neutral message metadata by introducing
an internal `QueueMessage` shape instead of adding loose metadata parameters to
every driver method:

```rust
QueueMessage {
    payload: Vec<u8>,
    metadata: Map<String, String>,
}
```

The metadata map is internal to the runtime and driver layer. It is not exposed
to MarretaLang code in the first cut.

Rationale for a message struct:

- queue transports naturally model payload plus metadata as one delivery object
- future transport fields can be added without growing trait parameter lists
- provider-specific drivers can map their native delivery records into one
  runtime shape
- Rust traits with wrapped request/response structs are easier to evolve than
  traits with many positional parameters

Provider mapping examples:

- RabbitMQ: AMQP message headers
- Kafka, future: record headers
- SQS, future: message attributes
- Pub/Sub, future: message attributes
- in-memory/testing driver: message metadata field

Trace metadata keys should use canonical lowercase names:

- `traceparent`
- `tracestate`

Drivers may map those keys to provider-specific transport structures, but the
runtime-level contract should remain lowercase and provider-neutral.

Trace metadata remains subject to broker limits. The W3C payload is small
enough for normal broker metadata limits:

- `traceparent` is 55 characters for version `00`
- `tracestate` is capped by 035 at 512 characters

---

## 7. Producer Semantics

When `queue.push` or `queue.publish` runs with an active trace context, the
runtime should attach trace metadata to the outgoing message.

For each produced message, the runtime should:

1. preserve the current `trace_id`
2. generate a new non-zero 8-byte lowercase hex `span_id`
3. preserve current `trace_flags`
4. preserve current `tracestate`
5. write `traceparent` metadata with version `00`
6. write `tracestate` metadata only when present

The produced `span_id` represents a logical producer/send span. MarretaLang does
not persist or export that span in the first cut, but the span ID is still
needed so downstream tools can reconstruct a correct trace tree if spans are
exported in the future.

If `queue.push` or `queue.publish` runs without an active trace context, the
runtime should not attach trace metadata.

Examples:

- startup code calling `queue.push` has no trace context
- future scheduled jobs may have no trace context
- `queue.push` inside a consumer uses the consumer's active trace context

---

## 8. Consumer Semantics

When `on queue` or `on topic` receives a message, the runtime should inspect
transport metadata for:

- `traceparent`
- `tracestate`

If valid trace metadata exists, the consumer should continue the trace:

1. read `trace_id` from metadata `traceparent`
2. treat metadata `span_id` as the parent producer span
3. generate a new non-zero 8-byte lowercase hex `span_id`
4. preserve metadata `trace_flags`
5. preserve valid metadata `tracestate`

The new consumer `span_id` represents the logical receive/process span for that
consumer execution.

In the first cut, MarretaLang log output emits only:

- `trace_id`
- `span_id`

It does not emit `parent_span_id`.

The producer's `span_id` from metadata is still treated as the logical parent
for trace tree reconstruction by external tooling that consumes propagated
`traceparent`. It is not surfaced in Marreta's own log shape in the first cut.

If trace metadata is missing or invalid, the runtime should create a new root
trace context for the consumer execution.

This creates an intentionally orphaned trace. It represents work that began at
the consumer boundary, not a continuation of an upstream request.

Rationale:

- messages may come from legacy or non-Marreta producers
- messages may have been produced before trace propagation existed
- broker bridges or tools may lose metadata
- manually inserted messages should still have correlated consumer logs
- an orphan trace is operationally more useful than no trace at all

Trace context is a hint, not a correctness contract. Invalid trace metadata
must never fail message processing.

---

## 9. Consumer Runtime Effects

Once the runtime establishes a consumer trace context, it should behave like the
HTTP request context from 035:

- `log.*` inside the consumer includes `trace_id` and `span_id`
- `http_client.*` inside the consumer propagates `traceparent` and `tracestate`
- `queue.push` and `queue.publish` inside the consumer propagate trace metadata
- interpreter execution outside any runtime context remains unchanged

MarretaLang code still does not read, write, or modify trace IDs directly.

---

## 10. Topic Fan-Out

`queue.publish` and `on topic` should follow the same propagation rules as
`queue.push` and `on queue`.

For fan-out delivery:

- the publisher attaches one trace context to the published message
- each subscriber receives the same `trace_id`
- each subscriber creates its own consumer `span_id`
- each subscriber's logs use that subscriber-specific `span_id`

This models each subscriber as a separate logical consumer span under the same
producer span.

---

## 11. Requeue, Retry, and DLQ

Trace metadata should be preserved across broker-level message movement when
the broker preserves message metadata.

Expected behavior:

- ack: no propagation concern
- nack without requeue: processing ends
- nack with requeue: original trace metadata remains with the message
- broker-managed retry queues: metadata remains with the message
- broker-managed DLQ: metadata remains with the dead-lettered message

If a broker or driver fails to preserve metadata across requeue, retry, or DLQ
moves, the redelivered message arrives without valid trace metadata. The
consumer should then follow Section 8 and create a new orphan root trace.

Application-level DLQ is different:

```marreta
on queue "orders" take msg
    queue.push "orders.dlq", msg
```

In this case, `queue.push` runs inside the consumer trace context and should
attach a new producer span under the consumer span. It does not start a new root
trace.

The first cut does not need to measure queue delay, retry count, or DLQ timing
as span metrics.

---

## 12. Toggle Behavior

Async trace propagation is controlled by the existing trace context toggle:

```bash
MARRETA_TRACE_CONTEXT=true|false
```

No second async-specific toggle should be introduced in the first cut.

When `MARRETA_TRACE_CONTEXT=false`:

- queue producers do not attach trace metadata
- queue consumers ignore inbound trace metadata
- `log.*` inside consumers does not include `trace_id` or `span_id`
- `http_client.*` inside consumers does not auto-propagate trace headers

This keeps 035 and 036 under one operational switch.

---

## 13. Validation and Sanitization

The queue metadata parser should reuse the same W3C validation rules from 035.

For metadata `traceparent`:

- accept version `00`
- require lowercase hexadecimal trace ID
- require lowercase hexadecimal parent/span ID
- require valid trace flags
- reject all-zero trace ID
- reject all-zero parent/span ID
- reject unsupported versions in the first cut

For metadata `tracestate`:

- only preserve when `traceparent` is valid
- validate W3C entry shape and safety
- cap to 32 entries
- cap to 512 characters total
- ignore invalid members when possible
- drop entirely if no valid members remain
- never mutate or add Marreta vendor entries

Invalid metadata should never fail message processing.

---

## 14. Example Flow

```marreta
route POST "/async-trace/start" take payload
    log.info({
        event: "request.received",
        id: payload.id
    })

    queue.push "ft.async_trace", {
        id: payload.id
    }

    reply 202, {
        accepted: true,
        id: payload.id
    }

on queue "ft.async_trace" take msg
    log.info({
        event: "consumer.received",
        id: msg.id
    })

    response = http_client.get("http://127.0.0.1:3737/async-trace/echo")

    log.info({
        event: "consumer.completed",
        id: msg.id,
        traceparent_seen: response.body.traceparent != "missing"
    })

route GET "/async-trace/echo" take headers
    reply 200, {
        traceparent: headers["traceparent"] or "missing"
    }
```

Expected behavior:

- the initial request log has the inbound or generated `trace_id`
- `request.received` has the same `trace_id`
- the queued message carries metadata `traceparent`
- `consumer.received` has the same `trace_id` and a different `span_id`
- `http_client.get` inside the consumer propagates a child `traceparent`
- `/async-trace/echo` sees `traceparent`

---

## 15. Functional Validation

Functional tests should cover the runtime behavior without exposing trace
metadata in payloads.

Recommended functional test:

1. send `POST /async-trace/start` with a known inbound `traceparent`
2. assert HTTP response is `202`
3. wait for request log and app log containing the known `trace_id`
4. wait for consumer log containing the same `trace_id`
5. assert consumer `span_id` differs from the HTTP request `span_id`
6. assert downstream `/async-trace/echo` receives a `traceparent` with the same
   `trace_id`

Recommended fan-out functional test:

1. send `POST /async-trace/broadcast` with a known inbound `traceparent`
2. publish a message to a topic with at least two matching subscribers
3. wait for both subscriber logs
4. assert both subscriber logs contain the same `trace_id`
5. assert each subscriber has a different `span_id`
6. assert subscriber `span_id` values differ from the original request span

Example shape:

```marreta
route POST "/async-trace/broadcast" take payload
    queue.publish "ft.async_topic", "events.created", {
        id: payload.id
    }

    reply 202, { ok: true }

on topic "events.created" take msg
    log.info({
        event: "subscriber_a.received",
        id: msg.id
    })

on topic "events.created" take msg
    log.info({
        event: "subscriber_b.received",
        id: msg.id
    })
```

Recommended scenario tests:

- queue push from a traced route does not change payload shape
- consumer with missing trace metadata still processes successfully
- consumer with invalid trace metadata still processes successfully
- topic fan-out gives each subscriber a separate consumer span

Validation should be layered:

- unit tests should cover trace metadata parsing and sanitization
- scenario tests should cover producer/consumer semantics with in-memory
  drivers
- functional tests should include at least one RabbitMQ-backed path proving that
  metadata crosses the real broker and reaches the consumer

Because consumers are asynchronous, shell functional tests should use polling
helpers such as `wait_for_log_pattern`, not fixed sleeps.

---

## 16. Non-Goals

This spec does not include:

- OpenTelemetry SDK integration
- OpenTelemetry exporters
- custom spans in MarretaLang code
- span timing or duration metrics for queue boundaries
- measuring queue delay
- baggage propagation
- user-facing queue metadata APIs
- custom message metadata fields in MarretaLang code
- payload-injected trace fields
- trace data in message envelope wrappers
- queue correlation IDs separate from W3C Trace Context
- batch publish span modeling
- heuristics to reattach orphan messages to old traces
- provider-specific tracing behavior
- changes to message schema validation

---

## 17. Watch Points

Known tradeoffs to revisit only with concrete evidence:

- Orphan root traces in consumers may create traces that do not connect to an
  upstream request. This is intentional in the first cut because correlated
  consumer logs are more useful than no trace. If it proves confusing in real
  tooling, a future spec may add an explicit orphan marker or revise this
  behavior.
- Some queue providers may not support metadata. Those providers will not carry
  async trace context in the first cut unless their driver gains a safe metadata
  mapping.
- Broker-managed DLQ and retry behavior depends on provider metadata
  preservation. Drivers should document provider-specific gaps if they exist.
- Future OpenTelemetry integration may export real producer and consumer spans.
  This spec only prepares propagation semantics; it does not export spans.

---

## 18. Resolved Decisions

1. `QueueMessage` should be introduced before implementation, carrying payload
   plus metadata as one internal driver shape.
2. Validation should be layered: unit tests for metadata, scenario tests for
   runtime semantics, and at least one RabbitMQ-backed functional test for real
   metadata transport.
3. Orphan roots should not be distinguished in logs in the first cut. If orphan
   traces prove confusing in real observability tooling, a future spec may add
   an explicit marker.
