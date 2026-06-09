---
title: "map"
category: types
slug: "reference/types/map"
summary: "Key-value objects and the methods available on them."
---

# map

A `map` is a key-value object. As a schema field it is written `meta: map` for a
free-form object, or you reference another schema for a typed, nested object. A
literal uses braces, as in `{ name: "Ana", age: 30 }`.

```ruby
user = { name: "Ana", age: 30 }
fields = user.keys()
```

## Methods

| Name | Signature | Summary |
|---|---|---|
| `map.has` | `has(key)` | Returns whether the map has a key. |
| `map.keys` | `keys()` | Returns the keys. |
| `map.values` | `values()` | Returns the values. |
| `map.size` | `size()` | Returns the number of entries. |
| `map.merge` | `merge(other)` | Returns a new map with another map merged in. |
| `map.delete` | `delete(key)` | Removes a key and returns the map. |
