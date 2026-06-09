---
title: "Use feature flags"
category: how-to
slug: "how-to/use-feature-flags"
summary: "Gate a route or a branch behind a feature flag backed by a MARRETA_FEATURE_* variable, so behavior is configuration, not code."
---

# Use feature flags

A feature flag turns behavior on or off through configuration, without changing code. Use
one to roll a change out gradually, to keep an unfinished path off in production, or to
disable something in one environment. Flags are read with the
[`feature`](../reference/namespaces/feature.md) namespace.

## Check a flag

`feature.enabled(name)` returns `true` or `false`. A flag name maps to an uppercased
environment variable: `beta` is backed by `MARRETA_FEATURE_BETA`, and `new_pricing` by
`MARRETA_FEATURE_NEW_PRICING`.

Gate a whole route by guarding on it at the top:

```ruby
route GET "/beta"
    require feature.enabled("beta") else fail 404, "not available"
    reply 200, { beta: true }
```

With the flag on, the route answers `200`. With it off, the guard fails and the route
answers `404`:

```bash
# Flag on: the route is available
MARRETA_FEATURE_BETA=true marreta serve
# GET /beta  ->  200 {"beta":true}

# Flag off (or unset): the guard rejects the request
marreta serve
# GET /beta  ->  404 {"error":"not available"}
```

## Branch on a flag

You can also switch a branch inside a route instead of gating the whole thing:

```ruby
route GET "/products/:id"
    product = db.products.find(params.id)
    require product else fail 404, "not found"

    if feature.enabled("new_pricing")
        reply 200, { id: product.id, price: product.sale_price }

    reply 200, { id: product.id, price: product.price }
```

## Notes

- An unset flag reads as `false`, so an unknown or unconfigured flag is off by default.
- An invalid value for a `MARRETA_FEATURE_*` variable is a configuration error at load, so a
  typo fails the server at startup rather than silently turning the flag off.
- Flags are read from `MARRETA_FEATURE_*` at startup, so changing one means restarting the
  server. See [Configure environment variables](configure-environment.md).
- Flags are static configuration, not per-user targeting. For that, decide in your own code
  from the request or the authenticated user.
