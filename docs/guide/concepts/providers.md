---
title: "Providers"
category: concepts
slug: "concepts/providers"
summary: "How Marreta exposes databases, caches, and messaging as pluggable providers behind a single namespace per concern."
---

# Providers

Marreta exposes each integration concern (a relational database, a document store,
a cache, messaging) as a language namespace backed by a **provider**. A provider is
a high-level abstraction: you select it, set its connection details in
`marreta.env`, and then use the namespace directly, with no client library and no
wiring.

This is why the how-to pages talk about "the cache provider" or "the database
provider" rather than naming a specific technology. The technology is an
implementation detail of the selected provider.

## One namespace per concern

Each concern has one namespace, configured by its own `MARRETA_*` variables:

| Concern | Namespace | Configured with | Current provider |
|---|---|---|---|
| Relational database | `db.*` | `MARRETA_DB_*` | PostgreSQL |
| Document store | `doc.*` | `MARRETA_DOC_*` | MongoDB |
| Cache | `cache.*` | `MARRETA_CACHE_*` | Redis |
| Messaging | `queue.*`, `topic.*`, `on queue`, `on topic` | `MARRETA_QUEUE_*` | RabbitMQ |

Today each namespace ships with a single provider, shown in the last column. The
abstraction is deliberate: a provider can have more than one implementation, so the
same `db.*` code could run on PostgreSQL today and another relational engine later
without changing your routes.

## Selecting a provider

You choose a provider with the `MARRETA_<concern>_PROVIDER` variable and give it
connection details:

```bash
MARRETA_DB_PROVIDER=postgres
MARRETA_DB_HOST=127.0.0.1
MARRETA_DB_PORT=5432
MARRETA_DB_NAME=catalog
MARRETA_DB_USER=catalog
MARRETA_DB_PASSWORD=...
```

`marreta init --with db,cache,doc,queue` scaffolds these variables and a
`docker-compose.yml` for the current providers, so a fresh project runs locally
with no extra setup.

## What the abstraction does and does not do

The goal is not to make infrastructure disappear. Connection settings, credentials,
and operational behavior still matter, and you still run the provider somewhere.
What the abstraction buys you is application code that stays focused on API
behavior: you write against the namespace, refer to the selected provider, and the
app does not change if the implementation behind it does.

## See also

- [Configure environment variables](../how-to/configure-environment.md): set the
  provider and its connection details.
- [Persist data with local services](../how-to/use-local-services.md): the database
  provider in practice.
