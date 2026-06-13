---
title: "Error codes"
category: reference
slug: "reference/errors"
summary: "The error codes the runtime emits, when they happen, and which ones you can recover from."
---

# Error codes

When the runtime returns an error, the response body carries a `code` field with one
of these values. The codes describe what went wrong. The HTTP status is usually yours
to choose with `fail` and `raise`, though the runtime sets it on its own in some cases.
For example schema validation returns 422, a unique-index violation returns 409 with
the `unique_violation` code, and a failed `require auth` or `allow` returns 401 or 403.

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
| `unique_violation` | A write violated a unique index or constraint. Returned as HTTP 409. Fires for both the relational and the document provider, including an index you created by hand. |
| `invalid_identifier` | A `db` column identifier (a `select` column, an `order_by` clause, or a `where`/`like`/`in` column) was not a valid identifier (a plain name or `table.column`). Returned as HTTP 400. Guards against a runtime-derived identifier becoming a SQL injection vector. |
| `unknown_column` | A `db` column identifier was a valid name but is not a column of the table, when a `db:` schema declares it. Returned as HTTP 400. |
| `runtime_error` | A generic runtime fault not covered by a more specific code. |

## Validation

A route that binds a body with `take payload as Schema` validates it against the
schema before your code runs. When the body does not match, the runtime returns
HTTP 422 with a message naming the offending field, and your route body never
executes. You do not write this check, and you do not choose its status.
