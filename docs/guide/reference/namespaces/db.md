---
title: "db"
category: namespaces
slug: "reference/namespaces/db"
summary: "Read and write rows in a relational table through the database provider, by primary key or with a query pipeline."
---

# db

The `db` namespace reads and writes rows in a relational table through the configured
database provider. Each table is addressed by name, as `db.<table>`, and a schema
marked `db:` defines its columns.

## When to use

Use `db` for structured data with typed columns and relationships that you evolve
with versioned migrations. If you want to store flexible, nested documents without a
schema or a migration step, use [`doc`](doc.md) instead.

See [Persist data with local services](../../how-to/use-local-services.md) for the
workflow and [Evolve your database with migrations](../../how-to/migrations.md) for
schema changes.

## Persistent schemas

A table is defined by a schema that declares `db: <table>`. The fields below it are
the columns, and `id` is the primary key:

```ruby
export schema Product
    db: products

    id: integer
    sku: string
    name: string
    price: decimal
```

Declaring the schema does not create the table. You create it, and evolve it as the
schema changes, with versioned migrations. See
[Evolve your database with migrations](../../how-to/migrations.md).

## Operations

For single rows, call a method on the table. `save` returns the stored record with
its generated `id`:

```ruby
product = db.products.save({ sku: "abc", name: "Widget" })
found = db.products.find(product.id)
```

| Operation | Signature | Summary |
|---|---|---|
| save | `db.<table>.save(map)` | Inserts a row and returns it, including the generated `id`. |
| find | `db.<table>.find(id)` | Returns the row with that primary key, or null. |
| find_all | `db.<table>.find_all()` | Returns every row in the table. |
| update | `db.<table>.update(id, map)` | Updates the row by id and returns it. |
| delete | `db.<table>.delete(id)` | Deletes the row by id. |

For anything beyond a primary-key lookup, open a query pipeline with `>>`. Steps
accumulate clauses, and a terminal step runs the query:

```ruby
premium = db.products >> where(price > 100) >> order_by("price asc") >> fetch
```

| Step | Form | Summary |
|---|---|---|
| where | `where(col: val)` or `where(col > val)` | Equality or comparison filter. |
| like | `like("col", "pattern")` | LIKE filter. |
| in | `in("col", list)` | IN filter. |
| order_by | `order_by("col asc")` | Sort. |
| limit / offset | `limit(n)` / `offset(n)` | Page the results. |
| fetch (terminal) | `>> fetch` | Returns the matching rows as a list. |
| fetch_one (terminal) | `>> fetch_one` | Returns the first row, or null. |
| count (terminal) | `>> count` | Returns the number of matches. |
| exists (terminal) | `>> exists` | Returns whether any row matches. |
| update (terminal) | `>> update(map)` | Updates every matching row and returns the count. |
| delete (terminal) | `>> delete` | Deletes every matching row and returns the count. |

## Relations

A schema field that references another persistent schema is a foreign-key relation,
not a copy of the row. Here an order belongs to a customer, and a customer has many
orders:

```ruby
export schema Customer
    db: customers

    id: integer
    name: string
    orders: list of Order

export schema Order
    db: orders

    id: integer
    total: decimal
    customer: Customer
```

`Order.customer` is a relation handle. After you materialize a row, navigate it with
the pipeline steps, starting from the field:

```ruby
order = db.orders.find(params.id)
customer = order.customer >> fetch
```

`order.customer` is a singular relation, so `>> fetch` returns the single related row
(not a list), and `>> exists`, `>> count`, and `>> where(...)` work as well. The
inverse `list of` relation, `customer.orders`, is a collection, so its `>> fetch`
returns a list.

## Raw SQL

When the pipeline cannot express a query (joins, CTEs, window functions), drop to
`db.native_query` with raw SQL. Values interpolated with `#{}` are evaluated and
bound as prepared-statement parameters, not concatenated into the SQL string:

```ruby
rows = db.native_query("SELECT * FROM items WHERE name = #{params.name}")
```

It is an escape hatch, so reach for it only when the pipeline and the per-row methods
fall short.

## Column identifiers are validated

Filter values are bound as parameters, but column identifiers (the `select` columns, the
`order_by` clause, and the columns in `where`, `like`, and `in`) are part of the SQL, so the
runtime validates and quotes them. A column identifier must be a plain name (`price`) or a
`table.column`. Anything else is rejected with a `400 invalid_identifier`, which makes a
runtime-derived identifier safe by construction:

```ruby
route GET "/products"
    take query
    # query.sort is request input. A column name passes and is sorted; an injection attempt
    # ("price; DROP TABLE products") is rejected with a 400, never concatenated into SQL.
    products = db.products >> order_by(query.sort) >> fetch
    reply 200, products
```

`order_by` accepts `column` optionally followed by `asc` or `desc`, comma-separated for several
columns (`order_by("status, created_at desc")`). When a `db:` schema declares the table, an
unknown column is rejected with a `400 unknown_column`. The dev-time `non_literal_sql_identifier`
lint (see the lint reference) flags a runtime-built identifier before you ship; this runtime guard
is the defense in depth that closes it regardless.

## Notes

- The database provider must be configured and reachable before `marreta serve`. Run
  `marreta doctor` to check the connection.
- A table does not exist until you create it with a migration. Run
  `marreta migrate generate` and `marreta migrate apply` after declaring or changing
  a `db:` schema.
- Operations inside a `transaction` block share one connection and commit or roll
  back together.
- A query can fan out to concurrent branches with `*>>`, for example to fetch a count
  and a page of rows in one round-trip.
