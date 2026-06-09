---
title: "log"
category: namespaces
slug: "reference/namespaces/log"
summary: "Emit structured log events at info, warn, and error levels."
---

# log

The `log` namespace emits structured log events from your routes and consumers. Each
call produces a structured record with the level and message, alongside the request's
trace context.

## When to use

Use `log` to record what happened for observability: an info for normal flow, a warn
for a recoverable problem, an error for a failure worth alerting on.

## Operations

```ruby
log.info("processing order #{order.id}")
```

| Name | Signature | Summary |
|---|---|---|
| `log.info` | `log.info(message)` | Emits an info event. |
| `log.warn` | `log.warn(message)` | Emits a warning event. |
| `log.error` | `log.error(message)` | Emits an error event. |

## Notes

- An uncaught `raise` or `fail` is already logged by the runtime with its trace
  context, so you do not need to log before raising.
