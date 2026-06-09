---
title: "topic"
category: namespaces
slug: "reference/namespaces/topic"
summary: "Publish an event to a topic so every subscriber receives it (publish and subscribe)."
---

# topic

The `topic` namespace publishes an event to a named topic through the configured
messaging provider. A topic is publish and subscribe: every subscriber declared with
`on topic` receives a copy of each event.

## When to use

Use a topic to broadcast an event so several independent parts of a system can react
to it, without the publisher knowing who is listening. When each message should be
handled once by a single consumer instead, use [`queue`](queue.md).

[Make it event-driven](../../tutorials/make-it-event-driven.md) is a tutorial built
on topics.

## Operations

`topic.publish` broadcasts an event. Add `as <Schema>` to shape it to the event
contract before it goes out:

```ruby
topic.publish "order_placed" as Order, payload
```

| Name | Signature | Summary |
|---|---|---|
| `topic.publish` | `topic.publish "<topic>" [as Schema], payload` | Publishes an event to every subscriber. |

A subscriber is a top-level handler, not a method on this namespace, and a topic can
have any number of them:

```ruby
on topic "order_placed" take event as Order
    log.info("order #{event.id}")
```

## Notes

- The messaging provider must be configured and reachable before `marreta serve`. Run
  `marreta doctor` to check the connection.
- Subscribers share the queue ack/nack rules: a clean run acknowledges, an error or a
  schema mismatch nacks without requeue. See
  [ack and nack](../../how-to/async-work-with-a-queue.md#acknowledge-and-reject).
