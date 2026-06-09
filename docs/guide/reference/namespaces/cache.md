---
title: "cache"
category: namespaces
slug: "reference/namespaces/cache"
summary: "Store short-lived values through the cache provider, with TTLs, counters, and bulk reads and writes."
---

# cache

The `cache` namespace stores short-lived key-value data in a fast store, through the
configured cache provider. It is for data you can afford to lose and rebuild, not for
your source of truth.

## When to use

Reach for the cache to avoid repeating expensive work: a computed result, a slow
upstream response, or a counter. Set a TTL so entries clean themselves up. Because a
value can expire or be evicted at any time, never keep anything here that you cannot
recompute. For durable data, use [`db`](db.md) or [`doc`](doc.md).

The [Cache expensive work](../../how-to/use-cache.md) guide walks through the
cache-aside pattern from end to end.

## Operations

`cache.set` stores a value and `cache.get` reads it back, returning `null` on a miss:

```ruby
cache.set("greeting", "hello", ttl: 60)
value = cache.get("greeting")
```

| Name | Signature | Summary |
|---|---|---|
| `cache.get` | `cache.get(key)` | Returns the cached value, or null when missing. |
| `cache.set` | `cache.set(key, value, ttl: N, only_if_absent: true)` | Stores a value, optionally with a TTL or only when absent. |
| `cache.delete` | `cache.delete(key)` | Deletes a key and returns whether it existed. |
| `cache.exists` | `cache.exists(key)` | Returns whether a key exists. |
| `cache.ttl` | `cache.ttl(key)` | Returns the remaining TTL in seconds, or null. |
| `cache.expire` | `cache.expire(key, ttl: N)` | Updates a key's TTL. |
| `cache.incr` | `cache.incr(key, by: N)` | Increments an integer counter atomically. |
| `cache.decr` | `cache.decr(key, by: N)` | Decrements an integer counter atomically. |
| `cache.get_many` | `cache.get_many(keys)` | Reads multiple keys at once. |
| `cache.set_many` | `cache.set_many(values)` | Writes multiple entries at once. |

## Notes

- The cache provider must be configured and reachable before `marreta serve`. Run
  `marreta doctor` to check the connection.
- A value set without `ttl:` does not expire on its own. Give it a TTL, or
  `cache.delete` it when the underlying data changes.
- `incr` and `decr` are atomic, so they are safe for counters under concurrent
  requests.
- `only_if_absent: true` stores only when the key is unset, and returns whether it
  stored. It is the building block for a simple lock or a write-once flag.
