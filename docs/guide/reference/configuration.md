---
title: "Configuration (marreta.env)"
category: runtime
slug: "reference/configuration"
summary: "Every MARRETA_* runtime variable: what it does, the values it accepts, and its default."
---

# Configuration (marreta.env)

The runtime reads these `MARRETA_*` variables from the environment and from
`marreta.env`. Process environment variables override the file, which overrides the
built-in defaults. For how to set them locally and in production, see
[Configure environment variables](../how-to/configure-environment.md).

Feature flags use the dynamic pattern `MARRETA_FEATURE_<NAME>` (see
[`feature`](namespaces/feature.md)). Arbitrary `env.*` values your own code reads, for
example auth secrets, are not listed here.

## Server

| Variable | Purpose | Accepts | Required | Default |
|---|---|---|---|---|
| `MARRETA_HOST` | Address the server binds to. | string | no | `0.0.0.0` |
| `MARRETA_PORT` | Port the server listens on. | integer | no | `8080` |
| `MARRETA_CORS` | Enables CORS. | boolean | no | `true` |
| `MARRETA_CORS_ORIGIN` | Allowed CORS origin. | string | no | `*` |
| `MARRETA_DOCS_ENABLED` | Serves the OpenAPI docs. | boolean | no | `true` |
| `MARRETA_DOCS_PATH` | Path the docs are served at. | string | no | `/docs` |

## Runtime

| Variable | Purpose | Accepts | Required | Default |
|---|---|---|---|---|
| `MARRETA_LOG_LEVEL` | Minimum log level. | `debug`, `info`, `warn`, `error` | no | `info` |
| `MARRETA_REQUEST_LOG` | Logs each HTTP request. | boolean | no | `true` |
| `MARRETA_TRACE_CONTEXT` | Propagates W3C trace context to outbound calls. | boolean | no | `true` |
| `MARRETA_TIMEZONE` | Timezone for local date and time. | IANA name (`America/Sao_Paulo`) | no | `UTC` |
| `MARRETA_WORKER_THREADS` | Size of the runtime thread pool. | integer | no | CPU cores |
| `MARRETA_MAX_RECURSION_DEPTH` | Caps task recursion depth. | integer | no | `500` |
| `MARRETA_HTTP_TIMEOUT_MS` | Default timeout for `http_client` requests. | integer (ms) | no | `30000` |
| `MARRETA_RUNTIME_PROFILE` | Enables hot-path profiling when set. | `hot_path` | no | |

## Database

A `db` provider is opt-in: set `MARRETA_DB_PROVIDER` to enable it. The host, name, and
user are then required.

| Variable | Purpose | Accepts | Required | Default |
|---|---|---|---|---|
| `MARRETA_DB_PROVIDER` | Selects the provider and enables `db`. | `postgres` | to use `db` | |
| `MARRETA_DB_HOST` | Database host. | string | yes | |
| `MARRETA_DB_PORT` | Database port. | integer | no | `5432` |
| `MARRETA_DB_NAME` | Database name. | string | yes | |
| `MARRETA_DB_USER` | Database user. | string | yes | |
| `MARRETA_DB_PASSWORD` | Database password. | string | no | |
| `MARRETA_DB_SSL_MODE` | TLS mode. | `disable`, `require`, `verify-ca`, `verify-full` | no | |
| `MARRETA_DB_POOL_MAX_CONNECTIONS` | Max pool connections. | integer | no | |
| `MARRETA_DB_POOL_MIN_CONNECTIONS` | Min pool connections. | integer | no | |
| `MARRETA_DB_POOL_ACQUIRE_TIMEOUT_SECS` | Wait for a connection. | integer (s) | no | |
| `MARRETA_DB_POOL_IDLE_TIMEOUT_SECS` | Idle connection timeout. | integer (s) | no | |
| `MARRETA_DB_POOL_MAX_LIFETIME_SECS` | Max connection lifetime. | integer (s) | no | |
| `MARRETA_DB_POOL_TEST_BEFORE_ACQUIRE` | Validate a connection before use. | boolean | no | |

## Document store

A `doc` provider is opt-in: set `MARRETA_DOC_PROVIDER` to enable it.

| Variable | Purpose | Accepts | Required | Default |
|---|---|---|---|---|
| `MARRETA_DOC_PROVIDER` | Selects the provider and enables `doc`. | `mongodb` | to use `doc` | |
| `MARRETA_DOC_HOST` | Document store host. | string | yes | |
| `MARRETA_DOC_PORT` | Document store port. | integer | no | `27017` |
| `MARRETA_DOC_NAME` | Database name. | string | yes | |
| `MARRETA_DOC_USER` | User. | string | yes | |
| `MARRETA_DOC_PASSWORD` | Password. | string | no | |
| `MARRETA_DOC_AUTH_SOURCE` | Authentication database. | string | no | |
| `MARRETA_DOC_POOL_MAX_CONNECTIONS` | Max pool connections. | integer | no | |
| `MARRETA_DOC_POOL_MIN_CONNECTIONS` | Min pool connections. | integer | no | |
| `MARRETA_DOC_POOL_CONNECT_TIMEOUT_MS` | Connect timeout. | integer (ms) | no | |
| `MARRETA_DOC_POOL_SERVER_SELECTION_TIMEOUT_MS` | Server selection timeout. | integer (ms) | no | |

## Cache

A `cache` provider is opt-in: set `MARRETA_CACHE_PROVIDER` to enable it.

| Variable | Purpose | Accepts | Required | Default |
|---|---|---|---|---|
| `MARRETA_CACHE_PROVIDER` | Selects the provider and enables `cache`. | `redis` | to use `cache` | |
| `MARRETA_CACHE_HOST` | Cache host. | string | yes | |
| `MARRETA_CACHE_PORT` | Cache port. | integer | no | `6379` |
| `MARRETA_CACHE_USER` | User. | string | no | |
| `MARRETA_CACHE_PASSWORD` | Password. | string | no | |
| `MARRETA_CACHE_DB` | Database index. | integer | no | |
| `MARRETA_CACHE_PREFIX` | Prefix added to every key. | string | no | empty |
| `MARRETA_CACHE_DEFAULT_TTL` | Default TTL when a `set` omits one. | integer (s) | no | |
| `MARRETA_CACHE_POOL_SIZE` | Connection pool size. | integer | no | `10` |
| `MARRETA_CACHE_CONNECT_TIMEOUT_MS` | Connect timeout. | integer (ms) | no | `2000` |
| `MARRETA_CACHE_OPERATION_TIMEOUT_MS` | Per-operation timeout. | integer (ms) | no | `1000` |
| `MARRETA_CACHE_RECONNECT_MAX_RETRIES` | Reconnect attempts. | integer | no | `10` |

## Messaging

A messaging provider is opt-in: set `MARRETA_QUEUE_PROVIDER` to enable `queue` and
`topic`.

| Variable | Purpose | Accepts | Required | Default |
|---|---|---|---|---|
| `MARRETA_QUEUE_PROVIDER` | Selects the provider and enables messaging. | `rabbitmq` | to use messaging | |
| `MARRETA_QUEUE_HOST` | Broker host. | string | yes | |
| `MARRETA_QUEUE_PORT` | Broker port. | integer | no | `5672` |
| `MARRETA_QUEUE_USER` | User. | string | yes | |
| `MARRETA_QUEUE_PASSWORD` | Password. | string | no | |
| `MARRETA_QUEUE_VHOST` | Virtual host. | string | no | `/` |
| `MARRETA_QUEUE_PREFETCH` | Unacked messages a consumer may hold. | integer | no | `10` |
| `MARRETA_QUEUE_RECONNECT_MAX_RETRIES` | Reconnect attempts. | integer | no | |
| `MARRETA_TOPIC_EXCHANGE` | Exchange that topics publish to. | string | no | `marreta.topics` |

## Notes

- `marreta init --with db,cache,doc,queue` scaffolds these with local defaults and a
  `docker-compose.yml`.
- Run `marreta doctor` to check the configured providers are reachable.
- See [Providers](../concepts/providers.md) for the abstraction behind the provider
  groups.
