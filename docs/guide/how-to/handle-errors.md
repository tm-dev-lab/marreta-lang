---
title: "Handle errors"
category: how-to
slug: "how-to/handle-errors"
summary: "Fail a request with the right HTTP status, raise on unexpected conditions, and recover from fallible operations with rescue."
---

# Handle errors

Real routes have to say no. A record is missing, input breaks an invariant, an
external call fails. Marreta gives you three tools for this: `fail` for a
deliberate HTTP error, `raise` for an unexpected condition, and `rescue` for
recovering from a fallible operation. This page shows when to use each.

The snippets are taken from the project's tested example suite, so they behave
exactly as shown.

## Prerequisites

- A scaffolded project (`marreta init hello`).
- Familiarity with routes and tasks from the
  [Quickstart](../tutorials/quickstart.md).

## Fail with a chosen status

`fail <status>, <message>` ends the request with that HTTP status. You can fail
outright, or guard a value with `require ... else fail`:

```ruby
route GET "/errors/not_found"
    fail 404, "resource not found"

route POST "/errors/guard" take payload
    require payload.name else fail 400, "name is required"
    require payload.name.length() > 2 else fail 422, "name too short"
    reply 200, { ok: true, name: payload.name }
```

Use `fail` when the status is part of your API contract: 404 for a missing
resource, 400 or 422 for bad input, 502 when an upstream call fails. The guard
reads as a plain sentence, and the route stops the moment the condition does not
hold.

## Raise on an unexpected condition

`raise <message>` is for conditions that should not happen. An uncaught `raise`
reaches the client as HTTP 500:

```ruby
route GET "/errors/raise"
    raise "boom"
```

`raise` can carry a condition, and it propagates out of tasks, so an invariant
deep in your logic still surfaces at the route:

```ruby
task validate_positive(n)
    raise "must be positive" if n <= 0
    n

route GET "/errors/raise-from-task"
    reply 200, { result: validate_positive(-1) }
```

## fail or raise: HTTP layer or domain layer

These two are not interchangeable. The cleanest way to choose is to ask which layer
the error belongs to.

`fail` belongs to the **HTTP layer**. You are deciding the response the client gets,
and the status is part of your API contract. A missing resource is a 404, bad input
is a 400, a failed upstream call is a 502. A `fail` is an expected, designed outcome
of the route.

`raise` belongs to your **business and domain layer**. It signals a condition that
should not happen, such as a broken invariant or an unexpected state, and it is not
tied to a status. It propagates up through tasks like an exception, and only when it
reaches the route uncaught does it become a 500. Closer to where it happens you can
catch it with `rescue` and turn it back into a designed outcome.

A rule of thumb: if you can name the HTTP status the client should see, use `fail`.
If you are protecting an invariant in your logic and the status is not the point,
use `raise`, then decide at the edge (with `rescue`, or by letting it become a 500)
how it should surface.

## Recover with rescue

`rescue` catches a runtime error from a fallible operation and lets you continue.
In block form it returns a fallback shape, with `error.code` and `error.message`
available inside:

```ruby
route POST "/errors/rescue" take raw
    result = raw >> json.parse() rescue {
        recovered: true,
        code: error.code
    }

    reply 200, result
```

`rescue` also has shorter forms. Substitute a fallback value:

```ruby
val = risky("x") rescue "fallback"
```

Or convert the failure into a chosen HTTP status inside a pipeline:

```ruby
result = "input" >> always_fails >> rescue fail 503, "rescued"
```

Reach for `rescue` around operations that can fail for reasons outside your control
(parsing untrusted input, reading a file, calling a service), so one bad input does
not turn into a 500.

## Try it

```bash
marreta serve &

# A deliberate 404 from `fail`:
curl -s -o /dev/null -w '%{http_code}\n' http://localhost:8080/errors/not_found

# A guard that rejects bad input with 422:
curl -s -o /dev/null -w '%{http_code}\n' -X POST http://localhost:8080/errors/guard \
  -H 'content-type: application/json' -d '{"name":"a"}'

# An uncaught `raise` becomes a 500:
curl -s -o /dev/null -w '%{http_code}\n' http://localhost:8080/errors/raise
```

This prints `404`, then `422`, then `500`.

## Result checkpoint

You should now be able to return a deliberate HTTP error with `fail`, surface an
unexpected condition as a 500 with `raise`, and recover from a fallible operation
with `rescue` instead of crashing the request.

## Next steps

- [Shape a response](shape-a-response.md): control the body and status of the
  success path.
- [Validate a request payload](validate-a-payload.md): reject malformed input
  before your logic runs.
