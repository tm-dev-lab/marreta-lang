---
title: "Namespaces"
category: namespaces
slug: "reference/namespaces"
summary: "The built-in namespaces Marreta exposes for data, messaging, integration, and utilities, one page each."
---

# Namespaces

A namespace groups related operations under a name you call with dot syntax, like
`db.users.find(id)` or `time.now()`. These are the native namespaces: they are built into
the language and the runtime, with no import. The provider-backed ones (data, cache,
messaging) read their connection details from `marreta.env`.

For the idea behind namespaces, including the file namespaces you create yourself by
exporting tasks, see [Namespaces](../concepts/namespaces.md).

## Data and persistence

| Namespace | What it does | Page |
|---|---|---|
| `db` | Relational database access and query pipelines. | [db](namespaces/db.md) |
| `doc` | Document database access and aggregation pipelines. | [doc](namespaces/doc.md) |
| `cache` | Short-lived key-value storage with TTLs. | [cache](namespaces/cache.md) |

## Messaging

| Namespace | What it does | Page |
|---|---|---|
| `queue` | Point-to-point work queues. | [queue](namespaces/queue.md) |
| `topic` | Publish-subscribe topics. | [topic](namespaces/topic.md) |

## Integration

| Namespace | What it does | Page |
|---|---|---|
| `http_client` | Outbound HTTP requests to other services. | [http_client](namespaces/http_client.md) |

## Utilities

| Namespace | What it does | Page |
|---|---|---|
| `time` | Instants, dates, durations, and the clock. | [time](namespaces/time.md) |
| `math` | Numeric helpers (rounding, clamping, min and max). | [math](namespaces/math.md) |
| `json` | Parse and serialize JSON. | [json](namespaces/json.md) |
| `base64` | Encode and decode base64. | [base64](namespaces/base64.md) |
| `uuid` | Generate UUIDs. | [uuid](namespaces/uuid.md) |
| `log` | Structured logging. | [log](namespaces/log.md) |
| `fs` | Read and write files. | [fs](namespaces/fs.md) |
| `feature` | Read feature flags. | [feature](namespaces/feature.md) |

Request context (`params`, `query`, `payload`, `message`) and configuration (`env`) are
also addressed as namespaces, and are covered where they are used, in
[Routes](../tutorials/quickstart.md) and the how-to guides.
