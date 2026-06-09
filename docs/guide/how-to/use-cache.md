---
title: "Cache expensive work"
category: how-to
slug: "how-to/use-cache"
summary: "Store and reuse values in the cache to avoid repeating expensive work, with TTLs and the cache-aside pattern."
---

# Cache expensive work

The `cache` namespace stores short-lived values in a fast key-value store through
the configured cache provider, so you do not repeat expensive work on every request.
Use it for computed results, lookups from a slow upstream, counters, and other
short-lived state. The namespace is the same whichever provider backs it (see
[Providers](../concepts/providers.md)).

## Prerequisites

- A project with a running cache provider (`marreta init shop --with cache`, then
  `docker compose up -d --wait`). See
  [Persist data with local services](use-local-services.md).
- The [Quickstart](../tutorials/quickstart.md) finished.

## Set and get a value

`cache.set(key, value)` stores a value, and `cache.get(key)` reads it back.
`cache.get` returns `null` when the key is absent:

```ruby
route POST "/cache/:key" take payload
    cache.set(params.key, payload.value)
    reply 200, { ok: true }

route GET "/cache/:key"
    value = cache.get(params.key)
    require value else fail 404, "not cached"
    reply 200, { key: params.key, value: value }
```

## Expire with a TTL

Pass `ttl:` (seconds) so a value expires on its own. This is the normal way to use
a cache, since stale entries clean themselves up:

```ruby
route POST "/cache/:key" take payload
    cache.set(params.key, payload.value, ttl: 300)
    reply 200, { ok: true }
```

## Cache expensive work

The common pattern is cache-aside: read from the cache, and only on a miss build the
value and store it. The `if` expression returns the cached value when present, and on
a miss it builds the value, caches it, and returns it:

```ruby
route GET "/greeting/:name"
    cached = cache.get(params.name)
    greeting = if cached
        cached
    else
        fresh = "Hello, #{params.name}!"
        cache.set(params.name, fresh, ttl: 60)
        fresh
    reply 200, { greeting: greeting }
```

`cache.set` lives in the miss branch, so a hit serves the cached value without
re-writing it or renewing its TTL. The value here is trivial to keep the example
focused. In practice it is something worth caching, such as a slow query or an
[upstream call](call-an-external-api.md).

## Other operations

- `cache.delete(key)` removes a key.
- `cache.incr(key)` increments a numeric counter atomically.
- `cache.set(key, value, only_if_absent: true)` stores only if the key is not
  already set, and returns whether it stored.
- `cache.ttl(key)` returns the remaining seconds before a key expires.

## Test it

A scenario test stubs the cache call with `given`, so it needs no running cache
provider:

```ruby
scenario "serves a cached value"
    given cache.get("greeting") returns "hello"

    when GET "/cache/greeting"
    then status 200

scenario "a cache miss is a 404"
    given cache.get("absent") returns null

    when GET "/cache/absent"
    then status 404
```

See [Test your API](test-your-api.md) for the testing model.

## Try it

```bash
docker compose up -d --wait
marreta serve &

curl -s -X POST http://localhost:8080/cache/greeting \
  -H 'content-type: application/json' -d '{"value":"hello"}'
curl -s http://localhost:8080/cache/greeting
# → { "key": "greeting", "value": "hello" }
```

## Result checkpoint

You should now be able to store and read values, set a TTL so they expire, and use
cache-aside to compute a value once and reuse it.

## Troubleshooting

- **`cache.get` always returns `null`.** The cache may be unreachable, or the TTL
  already expired. Run `marreta doctor` to check the connection.
- **A value never refreshes.** A TTL that is too long serves stale data. Lower the
  `ttl:` or `cache.delete` the key when the source changes.

## Next steps

- [Configuration](../reference/configuration.md): the `MARRETA_CACHE_*` variables.
- [Call an external API](call-an-external-api.md): a common source of values worth
  caching.
