---
title: "Read request inputs"
category: how-to
slug: "how-to/read-request-inputs"
summary: "Bind the body, query string, and headers with take — raw or schema-validated — and read each one correctly."
---

# Read request inputs

A route reads its inputs with `take`. There are five input sources, each can be bound
raw (a plain map or string) or, for the body / query / headers, validated and coerced
against a schema. This page covers every variation and, just as important, how you
*read* each result, because the access pattern differs by source.

## The five inputs

| `take` | Binds | Reads as |
| --- | --- | --- |
| `take payload` | JSON request body | a map (`payload.field`, nestable) |
| `take query` | query-string parameters | a map of strings |
| `take headers` | request headers | a map of strings |
| `take form` | form-encoded body (`application/x-www-form-urlencoded`) | a map of strings |
| `take raw` | the unparsed request body | a single string |

`take raw` exists for the cases where there is no structure to parse: a webhook whose
signature you verify over the exact bytes, a plain-text or non-JSON body, or a payload
you forward verbatim. It hands you the body as a string and does nothing else. Bind the
headers alongside it when you need them (`take raw, headers`):

```ruby
route POST "/webhooks/stripe" take raw, headers
    signature = headers["stripe-signature"] or fail 401, "missing signature"
    reply 200, { received: true, bytes: raw.length() }
```

## Raw vs schema-validated

Each binding is independent: bind it raw, or add `as <Schema>` to validate and coerce
it. A schema-bound body / query / headers is checked before the route runs, an invalid
value returns a `422`, and the fields appear in the generated OpenAPI. A raw bind does
none of that, it just hands you the values.

```ruby
# raw: a map of strings, no validation, undocumented
route GET "/search" take query
    term = query.term or "none"

# schema-bound: validated, coerced, documented
schema ProductSearch
    term: string
    limit?: integer

route GET "/products" take query as ProductSearch
    reply 200, { term: query.term, limit: query.limit or 20 }
```

`take raw` and `take form` are always raw, they do not take a schema. The body, query,
and headers do.

## One take or many: inline and multi-line

Write the bindings one of two ways. Do not mix them in the same route.

**Inline** — a single `take` on the route line, bindings comma-separated. Good for one
input or a few:

```ruby
route POST "/products/search" take query as ProductSearch, payload as NewItem, headers as ApiHeaders
    reply 200, { ok: true }
```

**Multi-line** — one `take` per indented line, before any logic. Clearer with several:

```ruby
route POST "/products/search"
    take query as ProductSearch
    take payload as NewItem
    take headers as ApiHeaders

    reply 200, { ok: true }
```

A binding can be raw or schema-bound regardless of layout, and the two can be mixed in
one route (`take query as ProductSearch, payload` binds a typed query and a raw body).
What you cannot do is put a `take` on the route line *and* an indented `take` below: a
route is fully inline or fully multi-line, so its input contract is read in one place.

## How to read each input

The declaration is half the story. How you reach a value depends on the source.

### Payload

A map, accessed by field with `.`, and it nests:

```ruby
route POST "/orders" take payload as NewOrder
    sku = payload.item.sku
```

### Query — names match exactly

Query parameter names are matched **exactly** (case-sensitive, no name rewriting),
because query strings have no canonical naming convention the way headers do.

- **Raw:** the key is the parameter name as sent. Use `.` for an identifier-shaped name,
  and the `["..."]` subscript for anything with a hyphen or other non-identifier
  character:

  ```ruby
  route GET "/search" take query
    term = query.term                 # ?term=...
    full = query["complete-name"]     # ?complete-name=...  (subscript: has a hyphen)
  ```

- **Schema-bound:** the schema field name must equal the parameter name exactly, and you
  read it by that field name. Because a field name is a snake_case identifier, it can
  only bind a parameter whose name is also a valid identifier (`limit`, `complete_name`).
  A parameter literally named `complete-name` or `Complete-Name` cannot be bound by a
  schema, use the raw `take query` and a subscript for those.

  ```ruby
  schema Search
    term: string
    complete_name?: string

  route GET "/search" take query as Search
    name = query.complete_name        # binds ?complete_name=... exactly
  ```

> Heads up: query matching is exact. Declaring a field `complete_name` does **not**
> capture `?complete-name=` or `?Complete-Name=` — those stay reachable only through the
> raw `take query` subscript. (Headers are different, see below.)

### Headers — normalized name, with a convention

Header names are case-insensitive by the HTTP standard, so Marreta normalizes them.

- **Raw:** the key is the header name **lowercased**. A hyphenated name needs the
  subscript; a simple name can use `.`:

  ```ruby
  route GET "/secure" take headers
    token = headers["x-auth-token"]   # X-Auth-Token arrives lowercased, hyphen -> subscript
    auth  = headers.authorization     # simple name -> dot
  ```

  > Heads up: the raw key is always **lowercased**, so the subscript must be lowercase.
  > `headers["x-auth-token"]` works; `headers["X-Auth-Token"]` does **not** (it returns
  > nothing). The `.` form works only for a simple name with no hyphen (`headers.authorization`),
  > because the key after lowercasing has to be a valid identifier. This is the opposite of
  > **query**, where the raw key keeps its original case (`query["Complete-Name"]`).

- **Schema-bound:** a field maps to a header by a convention, case-insensitive with `_`
  and `-` treated as the same. So the field `x_auth_token` captures `X-Auth-Token`,
  `x-auth-token`, etc., and you read it by the field name:

  ```ruby
  schema ApiHeaders
    x_auth_token?: string

  route GET "/secure" take headers as ApiHeaders
    token = headers.x_auth_token       # captures X-Auth-Token by convention
  ```

This is the key difference from query: a header schema bridges `Title-Case-Hyphenated`
wire names to snake_case fields (correct, because headers are case-insensitive), while a
query schema matches exactly (correct, because query names are case-sensitive).

### Coercion (schema-bound query and headers)

Query and header values arrive as text. A schema coerces each to its declared type, a
value that cannot be coerced is a `422`, and a missing required field is a `422`:

- `limit: integer` turns `"20"` into `20`; `"abc"` is a 422.
- a boolean accepts only `true` or `false`.
- a `list of <scalar>` field is fed by a repeated key (`?tags=a&tags=b` -> `["a", "b"]`).
- an empty value (`?term=`) is treated as absent.

A schema bound to query or headers must be **flat**: scalar fields and lists of scalars
only, never a nested object or a list of objects (those belong to the body). See
[Validate a request payload](validate-a-payload.md) and
[Schemas](../concepts/schemas.md).
