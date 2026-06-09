---
title: "queue"
category: namespaces
slug: "reference/namespaces/queue"
summary: "Push a message to a point-to-point queue for one consumer to process asynchronously."
---

# queue

The `queue` namespace pushes a message onto a named queue through the configured
messaging provider. A queue is point-to-point: each message is handled by exactly one
consumer, declared elsewhere with `on queue`.

## When to use

Use a queue to hand work off so a route can return immediately, when each message
should be processed once by a single consumer (sending an email, resizing an image).
To broadcast an event to several independent subscribers instead, use
[`topic`](topic.md).

See [Process work asynchronously with a queue](../../how-to/async-work-with-a-queue.md)
for the producer, the consumer, and ack/nack semantics.

## Operations

`queue.push` enqueues a message. Add `as <Schema>` to validate and strip it to the
declared fields before it goes out:

```ruby
queue.push "welcome_emails" as Signup, payload
```

| Name | Signature | Summary |
|---|---|---|
| `queue.push` | `queue.push "<queue>" [as Schema], payload` | Enqueues a point-to-point message. |

The consumer is a top-level handler, not a method on this namespace:

```ruby
on queue "welcome_emails" take signup as Signup
    log.info("sending to #{signup.email}")
```

## Notes

- The messaging provider must be configured and reachable before `marreta serve`. Run
  `marreta doctor` to check the connection.
- A consumer acknowledges on a clean run. A runtime error or a schema mismatch nacks
  without requeue, and only `nack requeue` retries. See
  [ack and nack](../../how-to/async-work-with-a-queue.md#acknowledge-and-reject).
