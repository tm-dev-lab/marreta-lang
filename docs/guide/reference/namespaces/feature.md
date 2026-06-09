---
title: "feature"
category: namespaces
slug: "reference/namespaces/feature"
summary: "Check static feature flags backed by MARRETA_FEATURE_* environment variables."
---

# feature

The `feature` namespace checks static feature flags. A flag named `<NAME>` is backed
by the `MARRETA_FEATURE_<NAME>` environment variable, so you turn behavior on or off
through configuration without changing code.

## When to use

Use a feature flag to gate a route or a branch behind configuration, for example to
roll out a change or disable a path in one environment.

## Operations

```ruby
route GET "/beta"
    require feature.enabled("beta") else fail 404, "not available"
    reply 200, { ok: true }
```

| Name | Signature | Summary |
|---|---|---|
| `feature.enabled` | `feature.enabled(name)` | Returns whether the flag is enabled. |

## Notes

- A flag that is not set returns `false`, so an unknown or unconfigured flag is off by
  default.
- Flags are read from `MARRETA_FEATURE_*` at startup. See
  [Configure environment variables](../../how-to/configure-environment.md).
