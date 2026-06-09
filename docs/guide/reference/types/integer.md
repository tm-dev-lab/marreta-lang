---
title: "integer"
category: types
slug: "reference/types/integer"
summary: "Whole numbers and the methods available on them."
---

# integer

An `integer` is a whole number. As a schema field it is written `age: integer`, and a
literal is plain digits, as in `42`.

```ruby
count = 42
bounded = count.min(100).max(0)
```

## Methods

| Name | Signature | Summary |
|---|---|---|
| `integer.abs` | `abs()` | Returns the absolute value. |
| `integer.min` | `min(other)` | Returns the smaller value. |
| `integer.max` | `max(other)` | Returns the larger value. |
| `integer.to_string` | `to_string()` | Converts to a string. |
