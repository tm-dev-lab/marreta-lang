---
title: "Keywords"
category: language
slug: "reference/keywords"
summary: "The language keywords and constructs, grouped by what they do."
---

# Keywords

Marreta keeps a small reserved set, and the rule is one sentence: **namespaces are
reserved, directives and vocabularies are contextual.** Reserved words fall into two
layers.

**Layer 1, reserved.** A reserved word can never be a variable. These are the
infrastructure namespaces (`db`, `doc`, `feature`, `cache`, `queue`, `topic`, `fs`,
`json`, `base64`, `uuid`, `log`, `time`, `math`, `http_client`), the `env` accessor, the
type tokens (`string`, `integer`, `float`, `boolean`, `instant`, `date`, `duration`,
`interval`), and the structural keywords grouped below.

**Layer 2, contextual.** A contextual word means something in one position and is free
as a name everywhere else: the `db:` schema directive, the type-names `list`, `decimal`,
and `enum`, the pipeline vocabulary (`where`, `fetch`, `limit`, `order`, and the rest on
the [control flow](control-flow.md) page), the scenario DSL (`scenario`, `given`, `when`,
`then`), and the injected bindings (`params`, `auth`, `payload`).

Even a Layer 1 word is free in a **name position**: after a dot, as a map key, as a
schema field name, as a named-argument name, or as a `select` column. It reads as that
name there. It is blocked only as a binder (the left side of an assignment, a parameter,
a task or schema name), where it raises a dedicated error.

```ruby
# Free as a name
flags = { env: "prod", feature: "beta" }       # map keys
created = payload.date                          # a field after a dot
columns = db.events >> select(date, status) >> fetch   # a column

# Blocked as a binder
doc = 1   # error: 'doc' is a reserved word (the document database namespace); rename the variable.
```

A schema field named `doc`, `feature`, or `env` is allowed, because those are not
directives. A field named `db` is not, because the `db:` directive already claims that
line.

The structural keywords below are all Layer 1, grouped by what they do. Each links to the
guide that teaches it in context.

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
