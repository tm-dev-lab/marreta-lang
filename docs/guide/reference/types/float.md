---
title: "float"
category: types
slug: "reference/types/float"
summary: "Floating-point numbers and the methods available on them."
---

# float

A `float` is a floating-point number. As a schema field it is written
`rate: float`, and a literal has a decimal point, as in `3.14`. For money, use
[`decimal`](decimal.md) instead, which is exact.

```ruby
price = 3.14159
rounded = price.round(2)
```

## Methods

| Name | Signature | Summary |
|---|---|---|
| `float.abs` | `abs()` | Returns the absolute value. |
| `float.round` | `round(places)` | Rounds to the given number of places. |
| `float.ceil` | `ceil()` | Rounds up. |
| `float.floor` | `floor()` | Rounds down. |
| `float.min` | `min(other)` | Returns the smaller value. |
| `float.max` | `max(other)` | Returns the larger value. |
| `float.to_string` | `to_string()` | Converts to a string. |
