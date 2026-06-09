---
title: "Types"
category: types
slug: "reference/types"
summary: "The value types Marreta uses for schema contracts, payloads, responses, and runtime values."
---

# Types

Marreta uses one set of types everywhere. The same types describe schema contracts,
request payloads, responses, and the values your code works with at runtime. A field
declared `amount: decimal` is validated on the way in, shaped on the way out, and is a
real decimal in between.

## Scalar types

| Type | Meaning | Page |
|---|---|---|
| `string` | Text. | [string](types/string.md) |
| `integer` | A whole number. | [integer](types/integer.md) |
| `float` | A floating-point number. | [float](types/float.md) |
| `decimal` | An exact decimal, safe for money. | [decimal](types/decimal.md) |
| `boolean` | `true` or `false`. | [boolean](types/boolean.md) |

## Collection types

| Type | Meaning | Page |
|---|---|---|
| `list` | An ordered list of values. | [list](types/list.md) |
| `map` | A key-value object. | [map](types/map.md) |

## Temporal types

| Type | Meaning |
|---|---|
| `instant` | A point in time. |
| `date` | A calendar date. |
| `time` | A wall-clock time. |
| `duration` | A length of time. |
| `interval` | A span between two instants. |

These are constructed and read through the [`time`](namespaces/time.md) namespace.
See [Temporal types](types/temporal.md) for construction, properties, and the `on`
method.

## Schema constructs

These appear in schema definitions to compose other types:

| Form | Meaning |
|---|---|
| `enum ["a", "b"]` | One of a fixed set of strings. |
| `<Schema>` | A reference to another schema, a foreign-key relation when the target is persistent. |
| `list of <Schema>` | A typed list of another schema. |
