---
title: "Persist data with local services"
category: how-to
slug: "how-to/use-local-services"
summary: "Add a database to a project, run it locally with Docker, and read and write through the db namespace."
---

# Persist data with local services

Most APIs need somewhere to store data. Marreta talks to a database, a cache, a
document store, and a message queue through built-in namespaces, and you do not
pick a client or write connection code. This guide adds a database, through its
provider, to a project, runs it locally, and reads and writes records.

Throughout, **local services** are the containers your project runs against, and a
**provider** is the configurable backend for a namespace (see
[Providers](../concepts/providers.md)). The schema and route snippets below are
taken from the project's tested example suite.

## Prerequisites

- Docker with Compose, to run the local database provider.
- The [Quickstart](../tutorials/quickstart.md) finished, so `marreta init` and
  `marreta serve` are familiar.

## Scaffold with a database

Pass `--with db` to `marreta init` and the scaffold includes everything the
database needs: a `docker-compose.yml` with the database provider, and a
`marreta.env` already pointing at it.

```bash
marreta init shop --with db
cd shop
```

You can ask for more than one service at once, as in `--with db,cache,queue,doc`.

## Start the local services

```bash
docker compose up -d --wait
```

`--wait` blocks until the containers are healthy, so the next command finds a
database ready to accept connections. The connection details Marreta uses live in
`marreta.env`:

```bash
MARRETA_DB_PROVIDER=postgres
MARRETA_DB_HOST=127.0.0.1
MARRETA_DB_PORT=5432
MARRETA_DB_NAME=marreta
MARRETA_DB_USER=marreta
```

This file is local configuration and is gitignored. The committed
`marreta.env.example` documents the same keys without secrets.

## Make a schema persistent

A schema becomes a table by declaring `db: <table>`. The fields below the
declaration are the columns, and `id` is the primary key:

```ruby
export schema Product
    db: products

    id: integer
    sku: string
    name: string
    initial_stock: integer
    current_stock: integer
    reserved_stock: integer
    low_stock_threshold: integer
```

`schema` is the one modeling primitive in Marreta. The same keyword that describes
an API contract also describes a table, so adding `db:` makes `Product` persistent
without giving up its role as a contract.

That said, the body a client sends is rarely the whole row. Keep a separate schema
for the request body. It gives you free input validation (see
[Validate a request payload](validate-a-payload.md)) and exposes only the fields a
client should set:

```ruby
export schema SeedProductRequest
    sku: string
    name: string
    initial_stock: integer
    low_stock_threshold: integer
```

## Create the table with a migration

Declaring `db: products` models the table, but it does not create it. Marreta
never changes your database silently at server boot. Instead, you generate a
reviewable migration from the schema and apply it.

`migrate generate` compares your persistent schemas against the database and
writes a pair of SQL files (an `up` and a `down`) under `migrations/`:

```bash
marreta migrate generate
```

Read the generated `up` file if you want to see exactly what will run, then apply
it:

```bash
marreta migrate apply
```

`marreta migrate status` shows whether anything is pending, and
`marreta migrate rollback` reverts the last applied migration using its `down`
file. The migration files are meant to be committed, so every environment evolves
the same way. Run `migrate generate` again whenever you change a persistent
schema, and a new migration captures the delta.

## Write a row

The `db` namespace exposes each table by name. There is no query builder to
import and no ORM to configure. Validate the body with the payload schema, then
`save` the row. `save` returns the persisted record, including its generated
`id`:

```ruby
route POST "/inventory/seed" take payload as SeedProductRequest
    product = db.products.save({
        sku: payload.sku,
        name: payload.name,
        initial_stock: payload.initial_stock,
        current_stock: payload.initial_stock,
        reserved_stock: 0,
        low_stock_threshold: payload.low_stock_threshold
    })

    reply 201, {
        seeded: true,
        sku: product.sku,
        stock: product.current_stock,
        threshold: product.low_stock_threshold
    }
```

## Read a row

For a single record, open the table and narrow it with `>> where(...)`, then take
the first match with `>> fetch_one`:

```ruby
route GET "/inventory/:sku"
    product = db.products >> where(sku: params.sku) >> fetch_one
    require product else fail 404, "product not found"
    reply 200, product
```

## Compose queries

Steps after `>>` accumulate clauses, and nothing runs until a terminal step.
`fetch` returns the full list, `fetch_one` the first row, and `count` an integer:

```ruby
route GET "/db/pipeline/fetch"
    rows = db.items >> fetch
    reply 200, { items: rows }

route GET "/db/pipeline/fetch_one"
    row = db.items >> where(active: true) >> order_by("id asc") >> fetch_one
    reply 200, { item: row }
```

See the [`db` namespace reference](../reference/namespaces/db.md) for the full set
of steps and terminals.

## Try it

```bash
docker compose up -d --wait
marreta migrate generate
marreta migrate apply
marreta serve &

curl -s -X POST http://localhost:8080/inventory/seed \
  -H 'content-type: application/json' \
  -d '{"sku":"abc","name":"Widget","initial_stock":100,"low_stock_threshold":10}'
# → { "seeded": true, "sku": "abc", "stock": 100, "threshold": 10 }
```

Before you commit, confirm the project is wired correctly without starting the
server:

```bash
marreta doctor
```

`doctor` loads the project, reports the configured persistence, and tells you if a
provider is unreachable.

## Result checkpoint

You should now have a running database provider, a `products` table created by a
committed migration, and routes that write and read rows through `db.products`. A
`POST /inventory/seed` returns the persisted record, and `marreta doctor` reports
the database as configured and reachable.

## Troubleshooting

- **`docker compose up` exits but `serve` cannot connect.** Without `--wait`, the
  containers may still be starting. Re-run with `--wait`, or give them a moment.
- **`Connection refused` on serve.** The host or port in `marreta.env` does not
  match the running container. Compare `MARRETA_DB_PORT` against
  `docker compose ps`.
- **`relation "products" does not exist` at runtime.** You declared `db: products`
  but never ran the migration. Run `marreta migrate generate` and
  `marreta migrate apply` before serving, and re-run them after every schema
  change.
- **The runtime refuses to load the project.** Check the `requires_marreta` line
  in `app.marreta` against `marreta --version`. A project can demand a newer
  runtime than the one installed.
- **A field is missing from the saved row.** Only fields declared under the
  schema's `db:` line are persisted. Add the column to the schema.

## Next steps

- [`db` namespace](../reference/namespaces/db.md): the full set of query and
  write operations.
- [Configuration](../reference/configuration.md): every `MARRETA_DB_*` variable
  and what it controls.
- [Validate a request payload](validate-a-payload.md): reject bad input before
  it reaches the database.
