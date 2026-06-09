---
title: "Shape a response"
category: how-to
slug: "how-to/shape-a-response"
summary: "Control exactly what your API returns with a response schema, and pick the status code at runtime."
---

# Shape a response

Validating input is half of a contract. The other half is the response: what your
API returns, in what shape, with which status code. This page shows how to shape a
response with a schema, strip fields you do not want to leak, and choose the status
code at runtime.

The snippets are taken from the project's tested example suite, so they behave
exactly as shown.

## Prerequisites

- A scaffolded project (`marreta init hello`).
- The [Quickstart](../tutorials/quickstart.md) finished, so `reply` is familiar.

## Reply with a schema

`reply <status> as <Schema>, <body>` filters the body against the schema. Any field
the schema does not declare is dropped, so internal values never reach the client.
The request can be unvalidated and the response is still shaped:

```ruby
export schema ItemResponse
    id: integer
    name: string
    active: boolean

route POST "/contracts/response-only" take payload
    reply 200 as ItemResponse, {
        id: 1,
        name: payload.name,
        active: true,
        secret: "stripped"
    }
```

The `secret` field is not part of `ItemResponse`, so it never appears in the
response. This is the safe default for any endpoint that builds its body from
internal data.

## Schemas keep your OpenAPI contract precise

Marreta generates an OpenAPI (Swagger) document from your routes. When you reply
`as <Schema>`, the response shows up in that document as a named, typed component
with its required fields, reusable across endpoints. A free-form `reply 200, { ... }`
still produces a document, but the shape is inferred from the literal you return: an
anonymous inline object, with no name and weaker typing.

For a quick prototype, a free-form reply is fine. For a product whose clients depend
on a stable contract, we recommend always replying `as <Schema>`, so the documented
API matches what you actually return.

## A type error in the body is caught

Shaping is also validation. If the body provides a field with the wrong type for
the schema, the response fails rather than sending malformed JSON. Build the body
from values that match the declared types, and `reply as` guarantees the contract
holds on the way out.

## Choose the status code at runtime

The status does not have to be a literal. Any expression that evaluates to a status
works, so you can compute it and reply once:

```ruby
route GET "/response/dynamic_status"
    code = 202
    reply code, { accepted: true }
```

This keeps a route with several outcomes to a single exit point instead of
duplicating the body under each branch.

## Reply with HTML or text

By default `reply` sends `application/json`. For other content types, name it after
`reply`:

```ruby
route GET "/page"
    reply html 200, "<h1>Marreta</h1>"

route GET "/ping"
    reply text 200, "pong"
```

## Try it

```bash
marreta serve &

curl -s -X POST http://localhost:8080/contracts/response-only \
  -H 'content-type: application/json' \
  -d '{"name":"Alice"}'
# → { "id": 1, "name": "Alice", "active": true }
```

The `secret` field never appears, because the response is shaped by `ItemResponse`.

## Result checkpoint

You should now have a route that returns a schema-shaped JSON body with extra
fields stripped, a route that selects its status code at runtime, and routes that
reply with HTML or plain text.

## Next steps

- [Validate a request payload](validate-a-payload.md): contract the input as well
  as the output.
- [Handle errors](handle-errors.md): return failure responses with the right
  status.
