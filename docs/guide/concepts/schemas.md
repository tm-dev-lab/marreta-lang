---
title: "Schemas"
category: concepts
slug: "concepts/schemas"
summary: "One schema primitive describes your data once and serves as a request contract, a response shape, a database table, and the typed message a consumer receives."
---

# Schemas

A `schema` describes the shape of some data: a set of named fields with types. Marreta has
just one schema primitive, and you use the same one everywhere data crosses a boundary. The
same declaration can validate a request, shape a response, define a database table, and type
a message a consumer receives. Describing your data once, in one place, is the point.

```ruby
schema NewAccount
    owner: string
    email?: string
    balance: decimal
```

Schema names are `PascalCase`, fields are `snake_case` one per line, and a trailing `?`
marks a field optional. Fields can be scalars, collections, an `enum ["a", "b"]`, another
schema (`address: Address`), or a list of one (`items: list of LineItem`).

## A request contract

Bind a schema to the request body with `take payload as <Schema>`. The body is validated
before the route runs, so invalid input never reaches your code and returns a `422`
automatically:

```ruby
route POST "/accounts" take payload as NewAccount
    account = doc.accounts.save({ owner: payload.owner, balance: 0 })
    reply 201, account
```

## Query and header inputs

The same schema also declares query-string and header inputs, with `take query as <Schema>` and
`take headers as <Schema>`. The values arrive as text and are validated and coerced to the declared
types (an integer that does not parse, or a required field that is missing, returns a `422`), and the
parameters appear named and typed in the generated OpenAPI:

```ruby
schema ProductSearch
    term: string
    limit?: integer
    tags?: list of string

route GET "/products" take query as ProductSearch
    reply 200, { term: query.term, limit: query.limit or 20, tags: query.tags or [] }
```

A query or header value repeated in the request (`?tags=a&tags=b`) feeds a `list of <scalar>` field.
Because query and header parameters are flat on the wire, a schema bound to query or headers must be
**flat**: only scalar fields and lists of scalars, never a field that references another schema (a
nested object) or a list of objects. Binding a non-flat schema there is a load-time error (and a
[lint](../reference/lint.md) warning). A boolean accepts only `true` or `false`, and an empty value
(`?term=`) is treated as absent. Without a schema, `take query` / `take headers` still give a raw map
of strings, as before.

Names match differently by source. A **query** field matches the parameter name **exactly** (query
names are case-sensitive), so a field can only bind a parameter whose name is a valid identifier
(`complete_name` binds `?complete_name=`, not `?complete-name=`). A **header** field matches by a
case-insensitive convention with `_` and `-` treated as the same (`x_request_id` binds
`X-Request-Id`), because headers are case-insensitive by the HTTP standard. For the full set of
binding variations and how to read each one, see [Read request inputs](../how-to/read-request-inputs.md).

## A response shape

The same kind of schema shapes what goes out. `reply <status> as <Schema>` keeps only the
fields the schema declares, so internal values never leak to the client:

```ruby
route GET "/accounts/:id"
    account = doc.accounts.find(params.id)
    require account else fail 404, "not found"
    reply 200 as AccountResponse, account
```

Both bindings also feed the generated [OpenAPI document](../how-to/openapi-docs.md), so the
contract documents itself.

## A database table

Add `db: <table>` to a schema and it defines a relational table: the fields are the columns
and `id` is the primary key. The same type that validated a payload can be the stored record:

```ruby
export schema Product
    db: products

    id: integer
    sku: string
    name: string
    price: decimal
```

A field that references another persistent schema is a foreign-key relation, which is how
tables connect. Declaring the schema does not create the table; you do that with
[migrations](../how-to/migrations.md). A document store works differently: it is schemaless,
so there the schema validates and shapes the payloads at the edges rather than defining
storage.

## A typed message

Consumers receive typed messages the same way routes receive typed payloads. Bind the
message with `as <Schema>` on an `on queue` or `on topic` handler, and it is validated before
your handler body runs:

```ruby
on queue "orders.created" take order as NewOrder
    db.fulfillments.save({ order_id: order.id })
```

## Why one primitive

Because validation, response shaping, persistence, and messaging all use the same schema,
you describe a concept once and reuse it at every boundary it crosses. There is no separate
ORM model, request DTO, and response DTO to keep in agreement. See
[Validate a request payload](../how-to/validate-a-payload.md) and
[Shape a response](../how-to/shape-a-response.md) for the request and response sides, and
[`db`](../reference/namespaces/db.md) for persistent schemas and relations.
