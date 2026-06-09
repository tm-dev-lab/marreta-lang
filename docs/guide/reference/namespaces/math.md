---
title: "math"
category: namespaces
slug: "reference/namespaces/math"
summary: "Numeric helpers for rounding, clamping, and comparing numbers."
---

# math

The `math` namespace provides numeric helpers that work across `integer`, `float`,
and `decimal` values. Individual numbers also carry their own methods (see
[Types](../types.md)). Use `math` for the standalone helpers like `clamp`.

## When to use

Reach for `math` to constrain or round a number in a route, for example clamping a
page size or rounding a computed total.

## Operations

```ruby
size = math.clamp(requested, min: 1, max: 100)
```

| Name | Signature | Summary |
|---|---|---|
| `math.round` | `math.round(value, places: N)` | Rounds to the given places. |
| `math.ceil` | `math.ceil(value)` | Rounds up to an integer. |
| `math.floor` | `math.floor(value)` | Rounds down to an integer. |
| `math.abs` | `math.abs(value)` | Returns the absolute value. |
| `math.clamp` | `math.clamp(value, min: N, max: N)` | Constrains a number to a range. |
| `math.min` | `math.min(left, right)` | Returns the smaller number. |
| `math.max` | `math.max(left, right)` | Returns the larger number. |
