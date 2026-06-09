# v0.13 — Secret-Aware Infrastructure Config

> Status: Delivered.

## Motivation

Today Marreta projects configure infrastructure mostly through provider URLs such
as:

- `MARRETA_DB_URL`
- `MARRETA_DOC_URL`
- `MARRETA_CACHE_URL`
- `MARRETA_QUEUE_URL`

This works well for local development, but it is not enough for production and
CI/CD workflows where credentials are typically injected through environment
variables and secret managers.

In those environments, hardcoding full connection URLs in `marreta.env` is not
desirable.

Marreta should support secret-aware configuration by allowing credentials and
connection attributes to be provided as explicit environment variables, while
keeping the runtime configuration model explicit and clean.

## Goal

Allow infrastructure configuration to be expressed in a structured,
secret-friendly way without requiring users to embed credentials in full URLs.

The configuration layer should remain provider-agnostic at the language level,
and explicit at the runtime level.

## Scope

This draft covers configuration for:

- relational database (`db`)
- document database (`doc`)
- cache
- queue

HTTP client auth is intentionally out of scope for this draft.

## Design principles

- make secrets injectable through environment variables
- keep process environment as the highest-precedence override
- avoid requiring users to assemble URLs manually
- keep the default configuration surface small and Marreta-first
- keep the configuration model explicit and non-magical
- avoid mixed-resolution models where part of the config comes from a URL and
  part comes from separate secret variables
- keep provider-specific knobs out of the primary user workflow unless they are
  truly necessary

## Proposed model

Each infrastructure module should be configured in structured form.

### Core structured form

The primary configuration model should cover only the values that most projects
actually need.

Examples:

```env
MARRETA_DB_PROVIDER=postgres
MARRETA_DB_HOST=db.internal
MARRETA_DB_PORT=5432
MARRETA_DB_NAME=app
MARRETA_DB_USER=app_user
MARRETA_DB_PASSWORD=${DB_PASSWORD}
```

```env
MARRETA_DOC_PROVIDER=mongodb
MARRETA_DOC_HOST=doc.internal
MARRETA_DOC_PORT=27017
MARRETA_DOC_NAME=app
MARRETA_DOC_USER=app_user
MARRETA_DOC_PASSWORD=${DOC_PASSWORD}
```

```env
MARRETA_CACHE_PROVIDER=redis
MARRETA_CACHE_HOST=cache.internal
MARRETA_CACHE_PORT=6379
MARRETA_CACHE_PASSWORD=${CACHE_PASSWORD}
```

```env
MARRETA_QUEUE_PROVIDER=rabbitmq
MARRETA_QUEUE_HOST=queue.internal
MARRETA_QUEUE_PORT=5672
MARRETA_QUEUE_USER=app_user
MARRETA_QUEUE_PASSWORD=${QUEUE_PASSWORD}
```

## Resolution rules

The configuration system should resolve values in this order:

1. process environment variables
2. `marreta.env` in the project root
3. internal defaults where the provider semantics allow defaults

There is no `*_URL` fallback in this plan.

Structured config must be complete enough for the selected provider. Partial
config should fail clearly instead of mixing values from multiple sources.

Passwords and credentials are required only when the selected provider and
runtime environment actually require them. The config layer should not invent
credentials, but it may accept omitted passwords for providers or environments
where unauthenticated local access is valid.

Provider defaults may exist for values such as `PORT`, but Marreta should not
invent semantic defaults for identifiers or secrets such as `HOST`, `NAME`,
`USER`, or `PASSWORD`.

## Core variables by module

### DB

- `MARRETA_DB_PROVIDER` — selects the relational provider. Required when `db.*`
  or migrations are used.
- `MARRETA_DB_HOST` — database host or endpoint. Required for configured DB
  providers.
- `MARRETA_DB_PORT` — database port. Optional when the provider has a safe
  default; otherwise required.
- `MARRETA_DB_NAME` — target database name. Required for configured DB
  providers.
- `MARRETA_DB_USER` — database username. Required for configured DB providers.
- `MARRETA_DB_PASSWORD` — database password. Optional only when the target
  environment explicitly allows passwordless auth; otherwise required.

### DOC

- `MARRETA_DOC_PROVIDER` — selects the document DB provider. Required when
  `doc.*` is used.
- `MARRETA_DOC_HOST` — document DB host or endpoint. Required for configured
  document providers.
- `MARRETA_DOC_PORT` — document DB port. Optional when the provider has a safe
  default; otherwise required.
- `MARRETA_DOC_NAME` — target document database name. Required for configured
  document providers.
- `MARRETA_DOC_USER` — document DB username. Optional only when the target
  environment allows unauthenticated access.
- `MARRETA_DOC_PASSWORD` — document DB password. Optional only when the target
  environment allows unauthenticated access.

### CACHE

- `MARRETA_CACHE_PROVIDER` — selects the cache provider. Required when
  `cache.*` is used.
- `MARRETA_CACHE_HOST` — cache host or endpoint. Required for configured cache
  providers.
- `MARRETA_CACHE_PORT` — cache port. Optional when the provider has a safe
  default; otherwise required.
- `MARRETA_CACHE_PASSWORD` — cache password. Optional only when the target
  environment allows unauthenticated access.

### QUEUE

- `MARRETA_QUEUE_PROVIDER` — selects the queue provider. Required when the
  project declares queue consumers or uses queue publishing.
- `MARRETA_QUEUE_HOST` — queue broker host or endpoint. Required for configured
  queue providers.
- `MARRETA_QUEUE_PORT` — queue broker port. Optional when the provider has a
  safe default; otherwise required.
- `MARRETA_QUEUE_USER` — queue username. Required for configured queue
  providers.
- `MARRETA_QUEUE_PASSWORD` — queue password. Optional only when the target
  environment explicitly allows passwordless auth; otherwise required.

## Advanced provider options

Some providers expose extra connection knobs that are real, but too
infrastructure-specific to define the main configuration experience.

Those options may exist as advanced configuration, but they should be clearly
documented as secondary and optional.

Examples:

- `MARRETA_DB_SSL_MODE`
- `MARRETA_DOC_AUTH_SOURCE`
- `MARRETA_CACHE_USER`
- `MARRETA_CACHE_DB`
- `MARRETA_QUEUE_VHOST`

The primary docs and examples should teach only the core structured variables.

Committed examples should not teach advanced provider options in the primary
happy path.

### Advanced variable intent

- `MARRETA_DB_SSL_MODE` — provider-specific transport/security mode for
  relational DBs. Optional.
- `MARRETA_DOC_AUTH_SOURCE` — Mongo-style authentication database when auth is
  performed against a DB different from `MARRETA_DOC_NAME`. Optional.
- `MARRETA_CACHE_USER` — Redis ACL username for environments that do not rely on
  password-only auth. Optional.
- `MARRETA_CACHE_DB` — Redis logical database index. Optional.
- `MARRETA_QUEUE_VHOST` — RabbitMQ virtual host. Optional.

## Runtime behavior

The runtime should expose a single effective config to each driver.

That means:

- config loading should normalize env/file values into provider-specific config
  objects
- driver initialization should receive a single resolved config object
- the DSL itself remains unchanged
- no provider module should read `std::env` directly after this refactor

## Centralization requirement

Today config resolution is split:

- `db` / `doc` go through `MarretaConfig`
- `cache` / `queue` still read process env directly

This plan should centralize all infrastructure configuration in
`MarretaConfig`.

After implementation:

- `DbEngine::from_config(...)` uses resolved structured DB config
- `DocEngine::from_config(...)` uses resolved structured DOC config
- `CacheEngine::from_config(...)` should replace `CacheEngine::from_env()`
- `QueueEngine::from_config(...)` should replace `QueueEngine::from_env()`

## Examples

### Local development

```env
MARRETA_DB_PROVIDER=postgres
MARRETA_DB_HOST=localhost
MARRETA_DB_PORT=5432
MARRETA_DB_NAME=myapp
MARRETA_DB_USER=marreta
MARRETA_DB_PASSWORD=marreta

MARRETA_CACHE_PROVIDER=redis
MARRETA_CACHE_HOST=localhost
MARRETA_CACHE_PORT=6379
MARRETA_CACHE_PASSWORD=redis

MARRETA_DOC_PROVIDER=mongodb
MARRETA_DOC_HOST=localhost
MARRETA_DOC_PORT=27017
MARRETA_DOC_NAME=myapp
MARRETA_DOC_USER=marreta
MARRETA_DOC_PASSWORD=marreta

MARRETA_QUEUE_PROVIDER=rabbitmq
MARRETA_QUEUE_HOST=localhost
MARRETA_QUEUE_PORT=5672
MARRETA_QUEUE_USER=guest
MARRETA_QUEUE_PASSWORD=guest
```

### AWS-style managed services

```env
MARRETA_DB_PROVIDER=postgres
MARRETA_DB_HOST=myapp-db.cluster-abcdefghijkl.us-east-1.rds.amazonaws.com
MARRETA_DB_PORT=5432
MARRETA_DB_NAME=myapp
MARRETA_DB_USER=myapp_user
MARRETA_DB_PASSWORD=super-secret-password

MARRETA_CACHE_PROVIDER=redis
MARRETA_CACHE_HOST=myapp-cache.abc123.use1.cache.amazonaws.com
MARRETA_CACHE_PORT=6379
MARRETA_CACHE_PASSWORD=super-secret-password

MARRETA_DOC_PROVIDER=mongodb
MARRETA_DOC_HOST=myapp-doc.cluster-abcdefghijkl.us-east-1.docdb.amazonaws.com
MARRETA_DOC_PORT=27017
MARRETA_DOC_NAME=myapp
MARRETA_DOC_USER=myapp_user
MARRETA_DOC_PASSWORD=super-secret-password

MARRETA_QUEUE_PROVIDER=rabbitmq
MARRETA_QUEUE_HOST=b-12345678-1234-1234-1234-123456789012.mq.us-east-1.amazonaws.com
MARRETA_QUEUE_PORT=5672
MARRETA_QUEUE_USER=myapp_user
MARRETA_QUEUE_PASSWORD=super-secret-password
```

### CI/CD / production

```env
MARRETA_DB_PROVIDER=postgres
MARRETA_DB_HOST=prod-db
MARRETA_DB_PORT=5432
MARRETA_DB_NAME=app
MARRETA_DB_USER=app_user
MARRETA_DB_PASSWORD=super-secret

MARRETA_CACHE_PROVIDER=redis
MARRETA_CACHE_HOST=prod-cache
MARRETA_CACHE_PORT=6379
MARRETA_CACHE_PASSWORD=super-secret

MARRETA_DOC_PROVIDER=mongodb
MARRETA_DOC_HOST=prod-doc
MARRETA_DOC_PORT=27017
MARRETA_DOC_NAME=app
MARRETA_DOC_USER=app_user
MARRETA_DOC_PASSWORD=super-secret

MARRETA_QUEUE_PROVIDER=rabbitmq
MARRETA_QUEUE_HOST=prod-queue
MARRETA_QUEUE_PORT=5672
MARRETA_QUEUE_USER=app_user
MARRETA_QUEUE_PASSWORD=super-secret
```

Expected result:

- process env wins over file config
- the effective infra config is assembled from structured env vars
- no URL assembly is required from the user side
- CI/CD can inject secrets without editing committed project files

## Open questions

- Which advanced provider-specific options should be exposed in v1?
- Should advanced provider options live in the same section of the docs, or in
  a dedicated "advanced infrastructure config" section?
- Should `marreta doctor` or a future config validation command explain how the
  final effective connection was resolved?

## Implementation outline

1. extend `MarretaConfig` with structured infrastructure sections for db/doc/cache/queue
2. remove direct `std::env` reads from cache and queue modules
3. add provider-specific config validation
4. normalize effective config before driver initialization
5. update committed examples in `examples/` to the new structured variables
6. update docs

## Acceptance criteria

This plan is complete when:

- process env can inject secrets without editing `marreta.env`
- structured config fully replaces hardcoded credentials in examples, CI/CD,
  and production
- core docs and examples use only the minimal structured variables
- precedence rules are explicit and tested
- cache and queue no longer read environment directly
- partial config fails with clear errors instead of silently mixing sources

## Example migration plan

This change should not stop at runtime code. The committed examples must also be
migrated to the structured configuration model so the repository teaches the new
format consistently.

At minimum, the implementation should review and update:

- `examples/migrations_functional/marreta.env`
- `examples/functional_tests/marreta.env`, if present or newly needed
- `examples/ecommerce/marreta.env`, if present or newly needed
- any docker-compose files, test scripts, or README snippets in `examples/`
  that still export or document `MARRETA_*_URL`

The goal is that committed examples stop teaching URL-based infrastructure
configuration as the primary path.

## Validation plan

Validation should cover runtime behavior, configuration precedence, and
repository examples.

### Unit validation

- config parsing succeeds for complete structured DB config
- config parsing succeeds for complete structured DOC config
- config parsing succeeds for complete structured CACHE config
- config parsing succeeds for complete structured QUEUE config
- partial structured config fails with clear errors
- missing required secrets fail with clear, provider-specific errors
- process env overrides `marreta.env`
- cache no longer reads `std::env` directly
- queue no longer reads `std::env` directly

### Functional validation

Use real project examples from `examples/` and validate end-to-end behavior.

#### Phase A: Migrations project

In `examples/migrations_functional/`:

- keep only structured infra variables in `marreta.env`
- run:
  - `marreta migrate status`
  - `marreta migrate apply`
  - `marreta migrate list`
  - `marreta serve`
- confirm migrations and server startup still work with no `*_URL` variables

#### Phase B: HTTP functional project

In `examples/functional_tests/`:

- ensure the project uses structured variables only
- run the existing functional suite
- confirm the server boots and the suite passes with no `*_URL` variables

#### Phase C: Process env override

For at least one example project:

- keep baseline values in `marreta.env`
- override one or more secrets through process environment
- use a baseline value that would fail in practice, then override it with a
  valid process env value
- confirm the project succeeds because the process env value won

### Evidence to record

The implementation PR should capture, at minimum:

- one passing migration flow using structured DB config
- one passing server startup using structured config
- one proof that process env overrides `marreta.env`
- one example of a clear error message for incomplete structured config
