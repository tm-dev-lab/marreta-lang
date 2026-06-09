---
title: "Process work asynchronously with a queue"
category: how-to
slug: "how-to/async-work-with-a-queue"
summary: "Hand work off to a queue so a route returns immediately, and process it asynchronously with a consumer."
---

# Process work asynchronously with a queue

Some work does not need to finish before you answer the request: sending an email,
resizing an image, calling a slow downstream. A queue lets a route hand that work off
and return right away, while the work happens asynchronously. One route pushes a
message, and a separate consumer processes it. This is point-to-point messaging
through the configured messaging provider: each message is handled by one consumer.

## Prerequisites

- A project with a running messaging provider (`marreta init app --with queue`,
  then `docker compose up -d --wait`). See
  [Persist data with local services](use-local-services.md).
- The [Quickstart](../tutorials/quickstart.md) finished.

## Push a message

`queue.push "<queue>", <message>` enqueues a message. The route returns immediately,
typically with `202 Accepted`, because the work has been accepted but not yet done:

```ruby
route POST "/signups" take payload
    queue.push "welcome_emails", payload
    reply 202, { accepted: true }
```

The client gets its response without waiting for the email to be sent.

## Consume the message

An `on queue "<queue>"` handler receives each message and processes it
asynchronously, separately from the request that pushed it:

```ruby
on queue "welcome_emails" take msg
    log.info("sending welcome email to #{msg.email}")
```

The producer route and the consumer can live in the same project. The producer
already answered the client with `202`. The consumer does the real work whenever the
message arrives.

## Validate the message

Add a schema on both ends to enforce the message shape. Declare it once:

```ruby
export schema Signup
    email: string
```

The producer validates and strips the message to the declared fields, and the
consumer rejects anything that does not match (the message is nacked, not delivered
to your handler):

```ruby
route POST "/signups" take payload as Signup
    queue.push "welcome_emails" as Signup, payload
    reply 202, { accepted: true }

on queue "welcome_emails" take signup as Signup
    log.info("sending welcome email to #{signup.email}")
```

## Acknowledge and reject

A consumer acknowledges a message automatically when its handler finishes without
error. Success means the message is done and removed, so you only step in when
something is wrong:

- A **runtime error** in the handler (`raise`, `fail`, or anything unhandled) nacks
  the message **without** requeue. Retrying is opt-in: if a failure is transient and
  worth another attempt, reject it explicitly with `nack requeue`.
- A **schema mismatch** (the message does not match `take ... as <Schema>`) nacks it
  without requeue and never reaches your handler, because a malformed message will
  not become valid on retry.
- Reject explicitly with `nack` to discard a message, or `nack requeue` to put it
  back for another attempt, usually behind a guard:

```ruby
on queue "welcome_emails" take signup as Signup
    require signup.email else nack
    log.info("sending welcome email to #{signup.email}")
```

| Handler outcome | Result |
|---|---|
| Finishes without error | Acknowledged and removed |
| `nack` | Rejected, not requeued |
| `nack requeue` | Rejected, requeued for retry |
| Schema mismatch on arrival | Rejected, not requeued |
| Runtime error (`raise`, `fail`, unhandled) | Rejected, not requeued |

The only path that retries is an explicit `nack requeue`. Every other outcome either
acknowledges (a clean run) or rejects without requeue, so retries never happen by
accident. The same semantics apply to `on topic` subscribers.

## Test it

A scenario test stubs `queue.push` with `given`, so it asserts the producer's
response without a running provider. Because the consumer runs asynchronously, a
scenario verifies the producer side (the `202`), and the full push-to-consume flow is
exercised against the live provider:

```ruby
scenario "enqueues a welcome email"
    given queue.push "welcome_emails", anything returns true

    when POST "/signups" with {
        email: "ada@example.com"
    }
    then status 202
    then response is {
        body: { accepted: true }
    }
```

## Try it

```bash
marreta test
```

The producer scenario passes. To watch the consumer run, start the provider and the
server (`docker compose up -d --wait`, then `marreta serve`) and push a message with
`curl`. The handler processes it asynchronously.

## Result checkpoint

You should now have a route that enqueues work and returns `202`, and a consumer that
processes each message asynchronously, optionally validated by a schema.

## Next steps

- [Handle errors](handle-errors.md): `nack` and guards in a consumer.
- [Configuration](../reference/configuration.md): the `MARRETA_QUEUE_*` variables.
- [Providers](../concepts/providers.md): the messaging provider behind `queue`.
