---
title: "uuid"
category: namespaces
slug: "reference/namespaces/uuid"
summary: "Generate UUID strings, random (v4) or time-ordered (v7)."
---

# uuid

The `uuid` namespace generates UUID strings for identifiers, idempotency keys, and
correlation ids.

## When to use

Use `uuid.v7` when you want time-ordered ids that sort and index well, and `uuid.v4`
when you want a purely random id.

## Operations

```ruby
id = uuid.v7()
```

| Name | Signature | Summary |
|---|---|---|
| `uuid.v7` | `uuid.v7()` | Generates a time-ordered UUID v7 string. |
| `uuid.v4` | `uuid.v4()` | Generates a random UUID v4 string. |
