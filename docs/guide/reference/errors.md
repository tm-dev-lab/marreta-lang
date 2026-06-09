---
title: "Error codes"
category: reference
slug: "reference/errors"
summary: "The error codes the runtime emits, when they happen, and which ones you can recover from."
---

# Error codes

When the runtime returns an error, the response body carries a `code` field with one
of these values. The codes describe what went wrong. Your own `fail` and `raise`
choose the HTTP status, while these codes do not.

## Load-time

This code happens when the project is loaded (`serve`, `test`, `doctor`) and stops
the server from starting. Fix it before the app can run. `marreta doctor` reports it.

| Code | Meaning |
|---|---|
| `config_error` | A route or export conflict, a schema cycle, an incompatible runtime, or an invalid persistent schema. |

## Runtime: your code

These point to a bug in the project and surface as a 500 unless you handle them.

| Code | Meaning |
|---|---|
| `reference_error` | An undefined variable or task, a missing property, or a value that is not callable. |
| `type_error` | A value had the wrong type for the operation. |
| `arity_error` | A task was called with the wrong number of arguments. |
| `arithmetic_error` | Division by zero or another arithmetic fault. |
| `raise_error` | An explicit `raise` from project code. |

## Runtime: infrastructure

These come from a provider or a fallible operation. They are the ones you can recover
from with `rescue` (see [Handle errors](../how-to/handle-errors.md)).

| Code | Meaning |
|---|---|
| `db_error` | A relational database operation failed. |
| `cache_error` | A cache operation failed. |
| `queue_error` | A message queue operation failed. |
| `http_client_error` | An outbound `http_client` request failed. |
| `io_error` | A filesystem or project-load I/O failure. |
| `infrastructure_error` | An infrastructure dependency was unavailable. |
| `runtime_error` | A generic runtime fault not covered by a more specific code. |
