---
title: "string"
category: types
slug: "reference/types/string"
summary: "Text values and the methods available on them."
---

# string

A `string` is text. As a schema field it is written `name: string`, and a literal
uses double quotes, as in `"hello"`. Interpolation embeds values with `#{}`, as in
`"Hello, #{name}"`.

```ruby
name = "  Ada  "
clean = name.trim().upper()
parts = "a,b,c".split(",")
```

## Methods

| Name | Signature | Summary |
|---|---|---|
| `string.length` | `length()` | Returns the string length. |
| `string.upper` | `upper()` | Converts to uppercase. |
| `string.lower` | `lower()` | Converts to lowercase. |
| `string.trim` | `trim()` | Trims surrounding whitespace. |
| `string.contains` | `contains(value)` | Returns whether the string contains text. |
| `string.starts_with` | `starts_with(value)` | Returns whether the string starts with text. |
| `string.ends_with` | `ends_with(value)` | Returns whether the string ends with text. |
| `string.index_of` | `index_of(value)` | Returns the index of text, or -1 when missing. |
| `string.replace` | `replace(from, to)` | Replaces text. |
| `string.split` | `split(separator)` | Splits into a list. |
