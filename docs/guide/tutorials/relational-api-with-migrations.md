---
title: "Build a relational API with migrations"
category: tutorials
slug: "tutorials/relational-api-with-migrations"
summary: "Go from an empty folder to a relational, migration-backed product API you can create and read records through."
---

# Build a relational API with migrations

The [Quickstart](quickstart.md) served an in-memory greeting. This tutorial goes
further: a small product catalog backed by a relational database. By the end you will
have created a table from a schema, written a record through `db`, and read it back
over HTTP.

If you are new to persisting data, start with
[Save and read your first data](save-and-read-data.md), which uses the document store
and skips migrations entirely. This page is the relational version, with typed columns
and versioned schema changes.

You will build two endpoints:

- `POST /products` creates a product.
- `GET /products/:sku` reads one back.

Follow the steps in order. Each block runs as shown.

## Prerequisites

- The [Quickstart](quickstart.md) finished, so `marreta init` and `marreta serve`
  are familiar.
- Docker with Compose, to run the local database provider (Docker Desktop on macOS
  or Windows, or Docker Engine on Linux or Windows via WSL).

## 1. Scaffold a project with a database

```bash
marreta init catalog --with db
cd catalog
```

`--with db` adds a `docker-compose.yml` with the database provider and a `marreta.env`
already pointing at it. Start the database and wait until it is ready:

```bash
docker compose up -d --wait
```

## 2. Model the data

A schema becomes a table by declaring `db: <table>`. Create `schemas/catalog.marreta`:

```ruby
export schema Product
    db: products

    id: integer
    sku: string
    name: string
    price: decimal
```

`schema` is the one modeling primitive in Marreta, so the same `Product` you store
is also a response contract. The body a client sends is narrower, so add a payload
schema for the request:

```ruby
export schema NewProduct
    sku: string
    name: string
    price: decimal
```

## 3. Create the table with a migration

Declaring the schema models the table. It does not create it. Generate a migration
from the schema and apply it:

```bash
marreta migrate generate
marreta migrate apply
```

`generate` writes a reviewable pair of SQL files under `migrations/` (an `up` and a
`down`), and `apply` runs them against the database. Re-run both whenever you change
a persistent schema.

## 4. Write the create route

Create `routes/catalog.marreta`. Validate the body with the payload schema, save the
row, and reply with the stored product shaped by `Product`:

```ruby
route POST "/products" take payload as NewProduct
    product = db.products.save({
        sku: payload.sku,
        name: payload.name,
        price: payload.price
    })
    reply 201 as Product, {
        id: product.id,
        sku: product.sku,
        name: product.name,
        price: product.price
    }
```

`save` returns the persisted record, including the `id` the database generated.

## 5. Write the read route

Add a route to fetch one product by `sku`. Open the table, narrow it with `where`,
and take the first match. If there is none, fail with a 404:

```ruby
route GET "/products/:sku"
    product = db.products >> where(sku: params.sku) >> fetch_one
    require product else fail 404, "product not found"
    reply 200 as Product, {
        id: product.id,
        sku: product.sku,
        name: product.name,
        price: product.price
    }
```

## 6. Test it

Before running the server, write a scenario test. Scenario tests run your route
logic in memory and fast, without starting the database provider. Because of that,
you declare what each external call returns with `given`, so the test stays
self-contained. Put
this in `tests/catalog_test.marreta`:

```ruby
scenario "create a product"
    given db.products.save(anything) returns {
        id: 1,
        sku: "tutorial-sku",
        name: "Widget",
        price: "9.90"
    }

    when POST "/products" with {
        sku: "tutorial-sku",
        name: "Widget",
        price: "9.90"
    }
    then status 201

scenario "read the product back"
    given db.products.fetch_one() returns {
        id: 1,
        sku: "tutorial-sku",
        name: "Widget",
        price: "9.90"
    }

    when GET "/products/tutorial-sku"
    then status 200
```

```bash
marreta test
```

Both scenarios pass. The route's validation, save, and shaping all ran, with the
database call answered by your `given`. To learn the testing model in full, see
[Test your API](../how-to/test-your-api.md).

## 7. Run it

Start the server:

```bash
marreta serve
```

In another terminal, create a product and read it back:

```bash
curl -s -X POST http://localhost:8080/products \
  -H 'content-type: application/json' \
  -d '{"sku":"book-1","name":"Notebook","price":"12.50"}'
# example output: { "id": <generated>, "sku": "book-1", "name": "Notebook", "price": "12.50" }

curl -s http://localhost:8080/products/book-1
# the same product, read back by its sku

curl -s -o /dev/null -w '%{http_code}\n' http://localhost:8080/products/missing
# → 404
```

The `id` is assigned by the database, so the exact value depends on what is already
stored.

## Result checkpoint

You should now have a running database provider, a `products` table created by a
committed migration, and two endpoints: one that persists a product and returns it
with a generated `id`, and one that reads a product by `sku` or returns a 404. You
modeled the data once, as a schema, and used it for both the table and the response
contract.

## Next steps

- [Persist data with local services](../how-to/use-local-services.md): the reusable
  recipe for `db`, plus query composition and troubleshooting.
- [Validate a request payload](../how-to/validate-a-payload.md): go deeper on
  request contracts.
- [`db` namespace](../reference/namespaces/db.md): the full set of query and write
  operations.
