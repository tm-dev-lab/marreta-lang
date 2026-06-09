---
title: "Use transactions"
category: how-to
slug: "how-to/use-transactions"
summary: "Group several database writes so they all commit together or all roll back, with a transaction block."
---

# Use transactions

When a request makes more than one database write that must succeed or fail as a unit, wrap
them in a `transaction` block. Every `db` operation inside the block runs on one connection,
commits together when the block finishes, and rolls back as a whole if anything goes wrong.

## Write several records atomically

Put the writes inside `transaction`. If the block completes, all of them commit:

```ruby
route POST "/orders" take payload
    transaction
        a = db.items.save({ name: payload.first, active: true })
        b = db.items.save({ name: payload.second, active: false })

    reply 201, { a_id: a.id, b_id: b.id }
```

The values created inside the block (`a`, `b`) are available after it, so you can use the
generated ids in the response.

## Rollback happens on any error

You do not commit or roll back by hand. The block commits when it finishes normally and
rolls back when anything inside it raises, including a database failure, a `fail`, or a
failed `require`. So a guard inside the transaction undoes the writes that ran before it:

```ruby
route POST "/orders" take payload
    transaction
        order = db.orders.save({ customer: payload.customer })
        require payload.items else fail 422, "an order needs at least one item"
        db.line_items.save({ order_id: order.id, sku: payload.sku })

    reply 201, { id: order.id }
```

If `payload.items` is empty, the `fail` rolls back the `db.orders.save` that already ran, so
no orphan order is left behind.

## Broadcast is not allowed inside a transaction

A transaction is sequential by definition, so `*>>` ([broadcast](../concepts/broadcast.md))
inside a `transaction` block is a runtime error. Parallel branches and a single atomic
connection are mutually exclusive. Keep fan-out work outside the transaction.

## Test it

A scenario test drives the route with mocked database responses, so you assert the behavior
without a real database:

```ruby
scenario "commits both saves in a transaction"
    given db.items.save({ name: "a", active: true }) returns { id: 1, name: "a", active: true }
    given db.items.save({ name: "b", active: false }) returns { id: 2, name: "b", active: false }

    when POST "/orders" with { first: "a", second: "b" }

    then status 201
    then response is { body: { a_id: 1, b_id: 2 } }
```

This checks the route's logic and the database calls it makes, not a real rollback in the
database, since the mocks always return what you tell them. The rollback itself is exercised
against a live database in the project's own test suite.

## When to use it

- Use a transaction when two or more writes have to agree, like an order and its line items.
- Skip it for a single write, which is already atomic on its own.
- Keep the block small. Long transactions hold a connection and a lock for longer, so do
  slow work (outbound calls, queue publishing) before or after, not inside.

## Notes

- Transactions need a relational provider. See [`db`](../reference/namespaces/db.md) and
  [Persist data with local services](use-local-services.md).
- Every operation in the block shares one connection, which is what makes the commit and
  rollback atomic.
