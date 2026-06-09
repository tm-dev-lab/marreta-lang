# 035 — W3C Trace Context

> Status: Delivered
> Type: Runtime observability
> Scope: Runtime-only W3C trace context propagation and log correlation

Delivery notes:

- `f784e58` — added the W3C Trace Context spec.
- `c3e4eb5` — clarified functional-test expectations for trace context.
- `f7b70c6` — delivered runtime W3C Trace Context support for HTTP inbound,
  logs, and outbound `http_client.*` propagation.
- `53e0ff2` — aligned `tracestate` sanitization with W3C lenient member
  handling.

---

## 1. Purpose

This spec introduces runtime-native W3C Trace Context support for MarretaLang
servers.

The purpose is not to expose a tracing API in MarretaLang code, create custom
spans, or turn the runtime into an OpenTelemetry exporter.

The purpose is narrower:

- accept and validate inbound W3C `traceparent`
- create a valid trace context when inbound context is absent or invalid
- include `trace_id` and `span_id` in runtime request logs and `log.*` output
- propagate W3C `traceparent` through outbound `http_client.*` calls

This is a runtime/server concern only. The first cut intentionally adds no
application-facing namespace or expression surface.

---

## 2. Why Trace Context Matters

MarretaLang now has two complementary log streams:

- application logs from `log.*`
- runtime request logs from request logging

Without shared trace fields, operators can see that a request happened and that
application events happened, but cannot reliably join them across process,
service, and tool boundaries.

W3C Trace Context solves this through the market-standard HTTP propagation
format:

```http
traceparent: 00-<trace-id>-<span-id>-<trace-flags>
tracestate: ...
```

This keeps MarretaLang aligned with OpenTelemetry-compatible tooling instead
of introducing a parallel request ID or correlation ID mechanism.

For a single inbound request, the runtime should make this possible:

```json
{"timestamp":"2026-05-05T14:10:22.137Z","kind":"request","trace_id":"0af7651916cd43dd8448eb211c80319c","span_id":"b7ad6b7169203331","method":"POST","path":"/orders","route":"/orders","status":201,"duration_ms":1.37}
```

```json
{"timestamp":"2026-05-05T14:10:22.139Z","level":"info","trace_id":"0af7651916cd43dd8448eb211c80319c","span_id":"b7ad6b7169203331","data":{"event":"order.created","order_id":42}}
```

The `trace_id` is the cross-service join key. The `span_id` identifies the
current server-side operation.

---

## 3. Design Principles

Trace context must follow these rules:

1. It should be automatic in `marreta serve`.
2. It should stay small in the first cut.
3. It should follow W3C Trace Context instead of inventing a local ID scheme.
4. It should not expose a tracing namespace in application code.
5. It should treat externally supplied trace headers as untrusted input.
6. It should work without user code boilerplate.
7. It should remain compatible with future OpenTelemetry support.

---

## 4. Standard Contract

The first cut should use W3C Trace Context as the only runtime correlation
mechanism.

Inbound and outbound HTTP propagation should use:

- `traceparent`
- `tracestate`

Runtime and application logs should use:

- `trace_id`
- `span_id`

The first cut should not introduce:

- `X-Request-Id`
- `X-Correlation-Id`
- `correlation.id`
- `request.id`
- `context.*`

This avoids creating a second lightweight correlation mechanism that would
later need to coexist with tracing.

---

## 5. Runtime Contract

For every handled HTTP request in `marreta serve`, the runtime should establish
a trace context containing:

- `trace_id`
- `span_id`
- `trace_flags`
- optional `tracestate`

The trace context should exist even when request logging is disabled.

Rationale:

- request logging controls access-log emission
- trace context controls cross-service correlation

Those are related but not the same feature.

Trace context is runtime metadata. MarretaLang code does not generate, read, or
modify trace IDs directly in the first cut. If application-level trace surface
is ever needed, it should be designed against a concrete use case, not added
preemptively.

The runtime owns the trace context lifecycle end to end in the first cut.
Application code should not generate, read, modify, override, or depend on
trace IDs for business logic.

---

## 6. Inbound Trace Context

The runtime should read W3C Trace Context headers:

- `traceparent`
- `tracestate`

The first cut should accept inbound `traceparent` only when it is valid per W3C
Trace Context:

- version `00`
- lowercase hexadecimal trace ID
- lowercase hexadecimal parent ID
- valid trace flags
- non-zero trace ID
- non-zero parent ID

If `traceparent` is missing or invalid, the runtime should create a new root
trace context instead of failing the request.

Inbound `tracestate` should be preserved and propagated only when inbound
`traceparent` is valid.

The runtime does not need to parse, rank, or mutate `tracestate` in the first
cut. It should treat `tracestate` as an opaque W3C header value, subject to
basic header safety validation.

For first-cut pass-through, `tracestate` should be accepted only when it
satisfies W3C safety limits:

- list of key/value entries
- at most 32 entries
- at most 512 characters total
- no control characters
- no unsafe header bytes

Malformed or unsafe `tracestate` should be dropped silently.

When inbound `tracestate` exceeds 512 characters or 32 entries, the runtime
should drop trailing entries until both W3C limits are satisfied. If no entries
remain after truncation, the header should be dropped entirely.

When inbound `traceparent` is invalid or absent, any inbound `tracestate` should
also be discarded. Vendor-specific state is meaningful only relative to a valid
trace context; preserving it without context risks polluting downstream traces.

The runtime should propagate `tracestate` exactly as received. It should not
modify entries, reorder entries, or add a Marreta-specific vendor entry in the
first cut.

---

## 7. Generated Trace Context

When the runtime creates a new root trace context, it should generate:

- a new 16-byte lowercase hex `trace_id`
- a new 8-byte lowercase hex `span_id`
- default `trace_flags` of `00`

Generated IDs should use cryptographically secure randomness from the operating
system.

Generated values must follow W3C Trace Context constraints:

- `trace_id`: 32 lowercase hex chars, not all zero
- `span_id`: 16 lowercase hex chars, not all zero
- `trace_flags`: 2 lowercase hex chars

The log format for generated or accepted trace fields is part of the public
contract:

- `trace_id` is always 32 lowercase hexadecimal characters
- `span_id` is always 16 lowercase hexadecimal characters

Both formats are stable across runtime versions and may be relied upon by log
consumers.

---

## 8. Server Span Semantics

For a valid inbound `traceparent`, the runtime should:

- preserve the inbound `trace_id`
- create a new server-side `span_id`
- preserve inbound `trace_flags`
- preserve and propagate inbound `tracestate`

The inbound parent ID should not be emitted as the current `span_id` in logs.
The current server operation gets its own span ID.

The first cut does not need to expose parent span ID in logs.

---

## 9. No Application-Facing Surface

The first cut should not expose trace context to MarretaLang code.

It should not introduce:

- `trace.id`
- `trace.span_id`
- `correlation.id`
- `context.trace_id`
- `request.id`

Rationale:

- the runtime can provide useful correlation without new language surface
- exposing trace context invites API questions about custom spans and mutation
- W3C propagation should remain an operational runtime concern in the first cut

Application code can still log domain data normally:

```marreta
log.info({ event: "order.created", order_id: order.id })
```

The runtime adds `trace_id` and `span_id` to the emitted log event when an
active trace context exists.

---

## 10. Response Headers

The first cut should not emit `traceparent` in HTTP responses.

W3C Trace Context is a request-side propagation format. Response-side trace
context is handled by separate mechanisms such as `traceresponse`, which is not
part of this spec.

Unlike `X-Request-Id`, `traceparent` exists for system-to-system correlation,
not for human-facing support flows. Echoing it on responses creates
expectations that the W3C protocol does not support.

Per W3C Trace Context, propagation is request-side and one-directional for this
first cut. Clients that initiated the trace already know their `traceparent`;
servers should not echo it as a response contract.

---

## 11. Request Log Integration

When request logging is enabled, runtime request log events should include:

- `trace_id`
- `span_id`

Example:

```json
{"timestamp":"2026-05-05T14:10:22.137Z","kind":"request","trace_id":"0af7651916cd43dd8448eb211c80319c","span_id":"b7ad6b7169203331","method":"GET","path":"/orders/42","route":"/orders/:id","status":200,"duration_ms":0.42}
```

For ordinary `marreta serve` request handling, these fields should be present.
If inbound trace context is missing or invalid, the runtime should generate
valid trace fields and still emit them.

This means request logs emitted by `marreta serve` should always have
`trace_id` and `span_id` when trace context is enabled.

The first cut should not add `traceparent` or `tracestate` as raw log fields.
Logs should use parsed stable fields.

---

## 12. Application Log Integration

`log.*` calls executed inside an active trace context should include:

- `trace_id`
- `span_id`

Example:

```marreta
log.info({
    event: "order.created",
    order_id: order.id
})
```

Should emit:

```json
{"timestamp":"2026-05-05T14:10:22.139Z","level":"info","trace_id":"0af7651916cd43dd8448eb211c80319c","span_id":"b7ad6b7169203331","data":{"event":"order.created","order_id":42}}
```

`log.*` calls outside a trace context should continue to work and should omit
`trace_id` and `span_id`.

Examples outside trace context include:

- startup logs
- queue consumers
- any execution context outside an HTTP request

This asymmetry is intentional:

- request logs for handled HTTP requests always include trace fields when trace
  context is enabled
- application logs include trace fields only when emitted inside an active
  trace context

---

## 13. Outbound HTTP Propagation

When `http_client.*` runs inside an active trace context, the runtime should
propagate W3C Trace Context headers:

```http
traceparent: 00-<trace-id>-<span-id>-<trace-flags>
tracestate: ...
```

Outbound `traceparent` should use:

- the active request `trace_id`
- a new outbound `span_id`
- the active `trace_flags`
- version `00`

If user code explicitly supplies `traceparent` or `tracestate` in outbound
headers, the explicit user-provided header should win.

Propagation should be silent:

- it should not emit additional logs
- it should not fail when context is absent
- it should simply omit trace headers outside an active trace context

Generating a new outbound `span_id` is required so downstream services can
observe a distinct child operation instead of seeing the inbound server span ID
reused across the trace.

Outbound `tracestate` should be propagated as received when present and valid.
The runtime should not mutate it or add Marreta-specific entries in the first
cut.

---

## 14. Security And Trust Boundaries

Trace context is observability metadata only.

It must not be used for:

- authentication
- authorization
- tenant selection
- user identity
- rate-limit identity
- cache trust boundaries
- idempotency

The runtime must treat inbound `traceparent` and `tracestate` as untrusted
header data.

Invalid inbound `traceparent` should be replaced by a generated root context,
not normalized silently.

Invalid or unsafe inbound `tracestate` should be dropped, not echoed.

This prevents trace headers from becoming a header injection, log forging, or
trust-boundary confusion vector.

Inbound trace parsing must never reject the HTTP request. Trace context is
metadata, not request correctness.

---

## 15. Environment Configuration

The first cut should not require an environment toggle to enable trace context.

Trace context should be on by default in `marreta serve`.

The first cut should support:

- `MARRETA_TRACE_CONTEXT=true`
- `MARRETA_TRACE_CONTEXT=false`

Default behavior:

- `marreta serve` -> default `true`
- commands without an HTTP request lifecycle should not create trace context

When `MARRETA_TRACE_CONTEXT=false`:

- inbound trace headers are ignored
- no runtime trace context is created
- request logs do not receive `trace_id` or `span_id`
- `log.*` output does not receive trace fields
- `http_client.*` does not propagate `traceparent` or `tracestate`

Disabling trace context should be treated as an operational escape hatch, not
as the expected default.

The trace context toggle is independent from request logging. When
`MARRETA_TRACE_CONTEXT=false` and `MARRETA_REQUEST_LOG=true`, request logs should
continue to be emitted but should omit `trace_id` and `span_id`.

The existing request logging toggle remains separate:

- `MARRETA_REQUEST_LOG=true|false`

Future runtime configuration may introduce a family such as:

- `MARRETA_TRACE_*`

Potential future options:

- disable inbound trace header acceptance
- configure sampling
- configure OpenTelemetry export

Those are intentionally out of scope for the first cut.

---

## 16. Non-Goals

The first cut does not include:

- application-facing trace namespace
- custom spans in MarretaLang code
- span nesting APIs
- OpenTelemetry export
- sampling configuration
- baggage propagation
- `traceresponse`
- metrics export
- trace visualization
- queue trace propagation
- async task trace propagation
- automatic user identity correlation
- raw request header exposure
- raw request object exposure
- per-route trace configuration
- `X-Request-Id`
- `X-Correlation-Id`

---

## 17. Operational Tradeoffs

This spec intentionally adopts W3C Trace Context as the only first-cut
correlation mechanism.

That means MarretaLang does not emit or propagate `X-Request-Id` in this spec,
even though that header is widely used by CDNs, proxies, gateways, and
human-facing support workflows.

This has real operational costs:

- clients that do not send `traceparent` do not receive a simple support ID in
  the response
- manual `curl`/Postman debugging may require timestamp-based log lookup
- proxy access logs that rely on `X-Request-Id` do not automatically share a
  Marreta-generated ID

The tradeoff is deliberate:

- parallel correlation mechanisms diverge over time
- W3C Trace Context is the modern cross-service propagation standard
- MarretaLang should avoid introducing a local ID system before adopting the
  standard one

If this gap proves unacceptable in real deployments, a future spec may add an
operator-facing compatibility projection derived from the canonical W3C trace
context. That would need to preserve a single source of truth rather than
introducing an independent second ID.

Signals that could justify revisiting this decision include repeated support
workflow issues, recurring debugging friction in real deployments, or concrete
operator requirements from environments that depend on `X-Request-Id`.

---

## 18. Watch Points

Known tensions to revisit after real usage:

- whether lack of `X-Request-Id` creates unacceptable human support friction
- whether `traceresponse` becomes stable and widely adopted enough to support
- how sampling should interact with future OpenTelemetry export
- whether queue/event trace propagation needs a dedicated transport contract
- whether application-facing trace surface is justified by concrete use cases

---

## 19. Implementation Plan

### Phase 1 — Runtime trace context

- parse `MARRETA_TRACE_CONTEXT`
- parse inbound `traceparent`
- preserve safe inbound `tracestate`
- generate root trace context when inbound context is absent or invalid
- do not emit `traceparent` in HTTP responses

### Phase 2 — Log integration

- add `trace_id` and `span_id` to runtime request logs
- add `trace_id` and `span_id` to `log.*` events when context exists
- preserve existing `log.*` behavior outside trace context

### Phase 3 — HTTP client propagation

- propagate `traceparent` for outbound `http_client.*` calls inside active
  trace context
- generate a new outbound `span_id` while preserving `trace_id`
- propagate `tracestate` when present
- preserve explicit user-provided outbound trace headers

### Phase 4 — Examples and docs

- document trace context behavior
- add examples showing request log and application log trace correlation
- review `docs/vscode-marreta` if examples need syntax updates

---

## 20. Test Plan

### Phase 1 tests

- `MARRETA_TRACE_CONTEXT=false` disables trace context
- unset `MARRETA_TRACE_CONTEXT` defaults to enabled in `serve`
- missing inbound `traceparent` generates valid trace context
- valid inbound `traceparent` preserves `trace_id`
- valid inbound `traceparent` gets a new server `span_id`
- invalid inbound `traceparent` is replaced by generated context
- unsafe inbound `tracestate` is dropped
- inbound `tracestate` is dropped when `traceparent` is invalid
- invalid inbound `traceparent` never fails the HTTP request
- response does not include `traceparent`

### Phase 2 tests

- request log includes `trace_id` and `span_id`
- request log always includes generated trace fields for handled HTTP requests
- app `log.info(...)` inside route includes `trace_id` and `span_id`
- app `log.info(...)` outside route omits trace fields
- request logging disabled does not disable trace context

### Phase 3 tests

- outbound `http_client.get(...)` propagates `traceparent`
- outbound `traceparent` preserves `trace_id` and creates a new `span_id`
- outbound `traceparent` uses version `00` and propagates `trace_flags`
- outbound propagation preserves `tracestate` when present
- explicit outbound `traceparent` overrides automatic propagation
- propagation does not occur outside active trace context

### Phase 4 tests

- `examples/functional_tests` validates shared `trace_id` across inbound
  `traceparent`, request log, app log, and outbound propagation
- `examples/functional_tests` scenario runner validates inbound `traceparent`
- examples avoid new language syntax

---

## 21. Resolved Decisions

Implementation closed the original review questions as follows:

1. 035 is runtime-only with no application-facing surface.
2. Invalid or absent inbound `traceparent` creates a new root context without
   failing the request.
3. Valid `tracestate` is preserved as opaque pass-through; invalid members are
   ignored where possible.
4. Outbound `http_client.*` propagation is included in the first cut.
5. `trace_id` and `span_id` are the first-cut log fields.
6. `MARRETA_TRACE_CONTEXT=true|false` is the operational escape hatch.
7. `X-Request-Id` is intentionally not introduced; operational tradeoffs are
   documented in this spec.

---

## 22. Recommendation

This spec should be treated as minimal W3C Trace Context support:

- no MarretaLang syntax
- no local request ID mechanism
- no `X-Request-Id`
- no custom tracing API
- W3C `traceparent` propagation
- no `traceparent` response echo
- `trace_id` and `span_id` in logs

That gives MarretaLang standards-based observability without creating a
parallel correlation system.
