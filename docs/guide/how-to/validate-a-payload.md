---
title: "Validate a request payload"
category: how-to
slug: "how-to/validate-a-payload"
summary: "Reject malformed input with a schema, before a single line of your route runs."
---

# Validate a request payload

You want a route that accepts JSON and rejects anything malformed, whether a
missing field or a wrong type, before your logic runs. In Marreta Lang you do this
by attaching a schema to the request. There is no validation library to wire up and
no guard clauses to write for type checks: the schema *is* the contract.

Every snippet below is taken from the project's tested example suite, so it behaves
exactly as shown.

## Prerequisites

- A scaffolded project (`marreta init hello`).
- The [Quickstart](../tutorials/quickstart.md) finished, so routes and `reply` are
  familiar.

## Describe the shape

A schema lists the fields you expect and their types:

```ruby
export schema ItemPayload
    name: string
    active: boolean
```

Both fields are required. To make one optional, give it a trailing `?`. More on
that below.

## Attach it to the route

Use `take <name> as <Schema>` on the route line. Marreta validates the incoming
body against the schema *before* the route body runs:

```ruby
route POST "/contracts/request-only" take payload as ItemPayload
    reply 200, { received: payload.name, active: payload.active }
```

If the request is missing `name`, or sends `active` as text, Marreta returns
**422 Unprocessable Entity** automatically and your code never executes. Inside
the route, `payload` is already typed and safe to use.

This page is about the request. To contract the response as well, by shaping it and
stripping extra fields, see [Shape a response](shape-a-response.md).

## The schema also documents the request

Marreta generates an OpenAPI (Swagger) document from your routes. Binding the body
`as <Schema>` makes the request appear there as a named, typed component with its
required fields, and it documents the automatic 422. A bare `take payload` with no
schema still accepts a body, but the request shows up as a free-form, untyped
object.

For a quick prototype, an unbound body is fine. For a product whose clients depend
on a stable contract, we recommend binding the request `as <Schema>`.

## Optional and nested fields

Mark a field optional with `?`. A field can also be typed as another schema,
which validates the nested object too:

```ruby
export schema Address
    street: string
    city: string
    zipcode: string

export schema ContactPayload
    name: string
    email: string
    age?: integer
    address: Address
```

`ContactPayload` accepts a request with or without `age`, but always requires a
well-formed `address`.

## Validate business rules, not just shape

A schema covers structure. For rules it cannot express, such as "billing is
required" or "items must be present", use `require ... else fail`:

```ruby
route POST "/doc/orders" take payload as OrderPayload
    require payload.billing else fail 400, "billing address is required"
    require payload.items else fail 400, "items are required"
```

Schema validation returns 422. Your own `fail` returns whatever status you give
it. Use the schema for *what the data is*, and `require` for *what your domain
allows*.

## Try it

```bash
marreta serve &

# A well-formed request is accepted:
curl -s -X POST http://localhost:8080/contracts/request-only \
  -H 'content-type: application/json' \
  -d '{"name":"Alice","active":true}'
# → { "received": "Alice", "active": true }

# A malformed one is rejected with 422, before your code runs:
curl -s -o /dev/null -w '%{http_code}\n' -X POST http://localhost:8080/contracts/request-only \
  -H 'content-type: application/json' \
  -d '{"name":"Alice"}'
```

The second call prints `422`, because `active` is missing.

## Result checkpoint

You should now have a route that accepts a typed request body, rejects malformed
input with an automatic 422 before your logic runs, and exposes that contract in
the generated OpenAPI document.

## Common pitfalls

- **A required field is absent.** Fields without `?` reject a missing value with
  422. If a field is genuinely optional, mark it with `?` and handle the absent
  case (for example with `match` or `payload.field or default`).
- **422 vs. 400.** Type and presence failures are validation (422) and are
  automatic. Reserve `fail 400` for your own business rules.

## Validate query and headers too

The same schema mechanism validates query-string and header inputs, with
`take query as <Schema>` and `take headers as <Schema>`. Those values arrive as
text, so they are coerced to the declared types (a non-numeric integer, or a
missing required field, is a 422), and they appear named and typed in the
generated OpenAPI:

```ruby
schema ProductSearch
    term: string
    limit?: integer
    tags?: list of string

route GET "/products" take query as ProductSearch
    reply 200, { term: query.term, limit: query.limit or 20, tags: query.tags or [] }
```

A repeated key (`?tags=a&tags=b`) feeds a `list of <scalar>` field. A schema bound
to query or headers must be **flat** (scalars and lists of scalars, no nested
objects); a header field maps to its header name case-insensitively, with `_` and
`-` treated as the same (`request_id` matches `Request-Id`). Without a schema,
`take query` / `take headers` still give a raw map of strings.

For every `take` variation (raw vs schema, inline vs multi-line) and how to read each
input — including the exact-match rule for query and the lowercased keys for raw
headers — see [Read request inputs](read-request-inputs.md).

## Next steps

- [Read request inputs](read-request-inputs.md): every `take` variation and how to
  read the body, query, and headers.
- [Shape a response](shape-a-response.md): contract the response, not just the
  request.
- [Types](../reference/types.md): every field type a schema can declare.
