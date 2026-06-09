---
title: "Control flow and operators"
category: language
slug: "reference/control-flow"
summary: "Conditionals, pattern matching, loops, pipelines, and the boolean and collection operators."
---

# Control flow and operators

These are the constructs you use inside a route or task to branch, loop, transform
data, and compose values. The declarative keywords (`route`, `schema`, and so on) are
in [Keywords](keywords.md).

## Conditionals

`if` / `else` works as a statement and as an expression that produces a value:

```ruby
status = if cached
    cached
else
    "fresh"
```

## Pattern matching

`match` tests a value against patterns, with `fallback` as the default branch:

```ruby
label = match status
    "active" -> "on"
    fallback -> "off"
```

## Loops

`while` repeats a block while its condition holds:

```ruby
counter = 0
while counter < 3
    counter = counter + 1
```

## Pipelines

`>>` passes the result of each step to the next, which reads left to right instead of
nesting calls. It drives database queries, HTTP calls, and collection transforms:

```ruby
recent = db.orders >> where(active: true) >> order_by("id desc") >> fetch
```

## Broadcast

`*>>` sends the same value to several branches and collects their results positionally
into a list. The runtime runs independent branches (outbound calls, separate queries) in
parallel, so they overlap instead of waiting on each other:

```ruby
sections = user_id *>>
    -> load_profile
    -> load_orders
    -> load_recommendations
```

See [Broadcast](../concepts/broadcast.md) for how the parallelism, result order, and the
runtime's optimization of trivial branches work.

## Boolean operators

`and`, `or`, and `not` combine booleans. `or` also supplies a default when the left
side is null or false:

```ruby
name = user.name or "guest"
allowed = user.active and not user.banned
```

## Collections

Transform a list with `map`, emitting values with `keep` and dropping elements with
`skip`, and fold a list with `reduce`:

```ruby
names = users >> map user
    skip if not user.active
    keep user.name

total = prices >> reduce(0) acc, item
    acc + item
```
