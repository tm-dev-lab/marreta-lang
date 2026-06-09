---
title: "boolean"
category: types
slug: "reference/types/boolean"
summary: "True or false values, and where they show up in conditions and guards."
---

# boolean

A `boolean` is `true` or `false`. As a schema field it is written `active: boolean`,
and the literals are `true` and `false`. Comparisons and the boolean operators
(`and`, `or`, `not`) also produce booleans.

```ruby
active = true
adult = age >= 18

route GET "/account/:id"
    account = db.accounts.find(params.id)
    require account.active else fail 403, "account is disabled"
    reply 200, account
```

Booleans are the clearest values to use in `require` and `allow`. Guards also use
truthiness, so `null` and `false` stop the flow.

## Notes

- A `boolean` has no methods. You combine booleans with `and`, `or`, and `not` (see
  [Keywords](../keywords.md)).
- `cache.get`, `db.<table>.find`, and similar return `null` when absent, which is
  falsy, so you can guard them with `require value else ...` directly.
