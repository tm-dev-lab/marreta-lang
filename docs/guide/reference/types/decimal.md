---
title: "decimal"
category: types
slug: "reference/types/decimal"
summary: "Exact, money-safe decimal numbers and the methods available on them."
---

# decimal

A `decimal` is an exact decimal number, safe for money where a `float` would lose
precision. As a schema field it is written `amount: decimal`. A request can send it
as a number or a string (`"19.90"`), and it is coerced without rounding error.

```ruby
total = order.amount
cents = total.round(2)
```

## Methods

| Name | Signature | Summary |
|---|---|---|
| `decimal.round` | `round(places)` | Rounds using banker's rounding. |
| `decimal.ceil` | `ceil()` | Rounds up. |
| `decimal.floor` | `floor()` | Rounds down. |
| `decimal.trunc` | `trunc()` | Truncates toward zero. |
| `decimal.abs` | `abs()` | Returns the absolute value. |
| `decimal.scale` | `scale()` | Returns the number of decimal places. |
| `decimal.to_integer` | `to_integer()` | Converts to an integer, truncating toward zero. |
| `decimal.to_float` | `to_float()` | Converts to a float. |
| `decimal.to_string` | `to_string()` | Converts to a string. |
