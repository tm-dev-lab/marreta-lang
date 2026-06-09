---
title: "Keywords"
category: language
slug: "reference/keywords"
summary: "The language keywords and constructs, grouped by what they do."
---

# Keywords

These are the language constructs, grouped by purpose. Each links to the guide that
teaches it in context.

## Routes and responses

| Keyword | Form | Summary |
|---|---|---|
| `route` | `route VERB "/path" [take payload as Schema]` | Declares an HTTP route. |
| `reply` | `reply STATUS [as Schema], body` | Returns an HTTP response, optionally shaped by a schema. |
| `fail` | `fail STATUS, message` | Ends the route with a chosen HTTP error. |
| `require` | `require condition else fail ...` | Guards execution and fails when the condition is false. |
| `allow` | `allow expression` | Authorizes the request, returning 403 when false. |

```ruby
route POST "/items" take payload as NewItem
    require payload.name else fail 400, "name required"
    reply 201 as ItemView, { name: payload.name }
```

See [Validate a request payload](../how-to/validate-a-payload.md),
[Shape a response](../how-to/shape-a-response.md), and
[Secure your API](../how-to/secure-your-api.md).

## Schemas and tasks

| Keyword | Form | Summary |
|---|---|---|
| `schema` | `schema Name` (add `db: table` to persist) | Declares a validation, contract, and table shape. |
| `task` | `task name(args)` | Declares a reusable unit of logic. |
| `take ... as` | `take payload as Schema` | Binds and validates a request or message body. |
| `export` | `export schema` / `export task` | Makes a schema or task available across files. |

```ruby
export schema NewItem
    name: string

task title_case(name)
    name.upper()
```

## Errors

| Keyword | Form | Summary |
|---|---|---|
| `raise` | `raise message [if condition]` | Raises a runtime error (an uncaught one becomes a 500). |
| `rescue` | `expr rescue fallback` | Recovers from a fallible operation. |
| `nack` | `nack [requeue]` | Rejects a consumed message, optionally requeuing it. |

```ruby
data = raw >> json.parse() rescue { ok: false }
```

See [Handle errors](../how-to/handle-errors.md).

## Messaging and persistence

| Keyword | Form | Summary |
|---|---|---|
| `on queue` / `on topic` | `on queue "name" take msg [as Schema]` | Declares an async consumer or subscriber. |
| `transaction` | `transaction` (block) | Runs the enclosed database operations atomically. |

```ruby
on queue "emails" take msg
    log.info("sending to #{msg.to}")
```

See [Process work asynchronously with a queue](../how-to/async-work-with-a-queue.md)
and [Make it event-driven](../tutorials/make-it-event-driven.md).

## Control flow and operators

Conditionals (`if` / `else`, `match`, `while`), pipelines (`>>`, `*>>`), boolean
operators (`and` / `or` / `not`), and collection transforms (`map` / `keep` / `skip`,
`reduce`) have their own page, with snippets:
[Control flow and operators](control-flow.md).
