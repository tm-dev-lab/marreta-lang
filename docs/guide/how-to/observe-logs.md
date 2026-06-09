---
title: "Observe logs and traces"
category: how-to
slug: "how-to/observe-logs"
summary: "Read Marreta's structured logs, emit your own with the log namespace, set the level, and follow a request across log lines with its trace id."
---

# Observe logs and traces

Marreta emits structured JSON logs. There are two kinds: a `request` record for every HTTP
call the server handles, and an `app_log` record for each message your code emits. Both
carry a trace id, so you can follow a single request across all of its log lines.

## Emit your own logs

Use the [`log`](../reference/namespaces/log.md) namespace, which has `info`, `warn`, and
`error`:

```ruby
route GET "/work"
    log.info("starting work")
    result = 21 * 2
    log.warn("about to finish")
    reply 200, { result: result }
```

Each call produces an `app_log` record with the level and your message in `data`:

```json
{"timestamp":"2026-06-06T18:00:42Z","kind":"app_log","level":"info","trace_id":"0a85c287...","span_id":"4978d297...","data":"starting work"}
{"timestamp":"2026-06-06T18:00:42Z","kind":"app_log","level":"warn","trace_id":"0a85c287...","span_id":"4978d297...","data":"about to finish"}
```

## Every request is logged

The server logs one `request` record per HTTP call, with the method, path, status, and
duration:

```json
{"timestamp":"2026-06-06T18:00:42.809Z","kind":"request","trace_id":"0a85c287...","span_id":"4978d297...","method":"GET","path":"/work","route":"/work","status":200,"duration_ms":1.04}
```

Notice the `trace_id` is the same as the two `app_log` lines above. The request and the logs
your code emitted while handling it share one trace, so you can group them. Turn the
per-request record off with `MARRETA_REQUEST_LOG=false` if you only want your own logs.

## Tie a request's logs together

Because the `request` line and every `app_log` it produced share one `trace_id`, you can pull a
single request's whole story by that id. While developing, that is one filter on the JSON lines:

```bash
# every line for one request, in the order it happened
marreta serve | grep '"trace_id":"0a85c287'
```

A log collector (Loki, CloudWatch, Datadog, and so on) does the same with a `trace_id` filter,
which is how you read one request end to end in production. The `duration_ms` on the `request`
line is that request's total server time, so sorting requests by it is the first step when a
route feels slow.

## Set the level

`MARRETA_LOG_LEVEL` filters by severity (`debug`, `info`, `warn`, `error`, default `info`).
Raising it to `warn` drops the `info` line and keeps the `warn`:

```bash
# Only warn and error app logs are emitted
MARRETA_LOG_LEVEL=warn marreta serve
```

## Follow a trace across services

Each request is assigned a trace, and the `trace_id` ties its logs together. With
`MARRETA_TRACE_CONTEXT` on (the default), an outbound [`http_client`](../reference/namespaces/http_client.md)
request carries the W3C `traceparent` header, so a downstream service that also speaks trace
context continues the same trace instead of starting a new one. That is what lets you follow
one logical request across several services.

## Notes

- Logs go to the process output as JSON lines, ready for any log collector to parse.
- The relevant variables (`MARRETA_LOG_LEVEL`, `MARRETA_REQUEST_LOG`, `MARRETA_TRACE_CONTEXT`)
  are in the [Configuration reference](../reference/configuration.md).
