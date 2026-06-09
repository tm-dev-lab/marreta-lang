---
title: "Broadcast"
category: concepts
slug: "concepts/broadcast"
summary: "How the *>> operator sends one value to several branches, runs independent work in parallel, and collects the results in order."
---

# Broadcast

Broadcast sends the same value to several branches at once with the `*>>` operator and
collects their results into a list. Where a [pipeline](pipelines.md) is a chain, where each
step feeds the next, a broadcast is a fan-out: every branch receives the same input and
runs on its own.

```ruby
task load_profile(user_id) => http_client.get("https://users.internal/#{user_id}").body
task load_orders(user_id) => http_client.get("https://orders.internal/#{user_id}").body
task load_recommendations(user_id) => http_client.get("https://recs.internal/#{user_id}").body

route GET "/users/:id/dashboard"
    sections = params.id *>>
        -> load_profile
        -> load_orders
        -> load_recommendations

    reply 200, {
        profile: sections[0],
        orders: sections[1],
        recommendations: sections[2]
    }
```

Each branch is one of the three tasks, and each receives the same input, `params.id`. They
are independent: `load_orders` does not need anything from `load_profile`, so the runtime
can run all three at once. The results come back as a list in the order the branches are
written, so `sections[0]` is always `load_profile` regardless of which call returned first.

## Independent work runs in parallel

The reason to reach for broadcast is parallelism. When the branches do independent work,
like three separate outbound calls or queries, the runtime runs them at the same time
instead of one after another. Three calls that each take a second finish together in about
a second, not three. That is the whole point: a fan-out of independent work without the
wiring to manage threads yourself.

This makes broadcast the right tool when the branches do not depend on each other. If one
branch needs the result of another, that is a chain, so use a [pipeline](pipelines.md).

## The runtime may optimize trivial branches

Parallelism here is an optimization the runtime applies, not a timing guarantee you should
build on. When every branch is a trivial, side-effect-free computation (no calls, no
database or cache or queue or HTTP, no nested broadcast), the runtime runs the branches
sequentially on purpose: spawning parallel work for a handful of cheap expressions would
cost more than just computing them. The results and their order are identical either way,
so this is invisible to your code.

In other words, the runtime parallelizes when there is real, independent work to overlap,
and skips the overhead when there is not. You write the same `*>>` regardless.

## Notes

- Results are positional and follow declaration order, even though execution may not.
- A branch can be a task, a namespaced task (`-> file.task`), or a call.
- `*>>` is not allowed inside a `transaction` block, since the branches run independently.

See [Control flow and operators](../reference/control-flow.md) for the operator in the
wider syntax, and [Pipelines](pipelines.md) for the sequential counterpart.
