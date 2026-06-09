---
title: "Make it event-driven"
category: tutorials
slug: "tutorials/make-it-event-driven"
summary: "Publish an event from a route and have several independent subscribers react to it, using topics."
---

# Make it event-driven

[Process work asynchronously with a queue](../how-to/async-work-with-a-queue.md)
handed work to a single consumer. A **topic** goes further: it broadcasts an event to
any number of subscribers. This is publish and subscribe (pub/sub), and it is how you
let several parts of a system react to the same thing without knowing about each
other.

In this tutorial an order is placed, your route publishes one `order_placed` event,
and two independent subscribers react to it: one sends a confirmation, the other
records analytics.

## Prerequisites

- The [Quickstart](quickstart.md) finished, and ideally
  [Process work asynchronously with a queue](../how-to/async-work-with-a-queue.md).
- Docker with Compose, to run the local messaging provider (Docker Desktop on macOS
  or Windows, or Docker Engine on Linux or Windows via WSL).

## 1. Scaffold with the messaging provider

```bash
marreta init shop --with queue
cd shop
docker compose up -d --wait
```

## 2. Model the event

Put the event shape in `schemas/orders.marreta`:

```ruby
export schema Order
    id: integer
    total: decimal
```

## 3. Publish the event

Create `routes/orders.marreta`. The route validates the order, publishes an event,
and returns right away. It does not know or care who is listening:

```ruby
route POST "/orders" take payload as Order
    topic.publish "order_placed" as Order, payload
    reply 202, { published: true }
```

Publishing `as Order` shapes the event to the schema before it goes out, so every
subscriber receives the same well-formed contract.

## 4. Subscribe to the event

Add two subscribers for the same topic. Each one receives every `order_placed` event,
independently. This is the difference from a queue, where only one consumer would get
each message:

```ruby
on topic "order_placed" take event as Order
    log.info("EMAIL: confirmation for order #{event.id}")

on topic "order_placed" take event as Order
    log.info("ANALYTICS: recorded order #{event.id}")
```

Each subscriber takes the event `as Order`, so it validates the incoming event
against the same schema the publisher used. The contract holds on both ends, and a
malformed event is rejected rather than handed to your code.

A subscriber that finishes without error acknowledges the event. A runtime error
rejects it without requeue, and you can reject explicitly with `nack` (or
`nack requeue` to retry). See
[ack and nack](../how-to/async-work-with-a-queue.md#acknowledge-and-reject) for the
full semantics.

The producer route and the subscribers can live in the same project, or the
subscribers can live in entirely separate services. The publisher does not change
either way.

## 5. Test it

Write a scenario test for the publisher. Stub the publish with `given`, so it asserts
the route's behavior without a running provider:

```ruby
scenario "publishes an order_placed event"
    given topic.publish "order_placed", anything returns true

    when POST "/orders" with {
        id: 7,
        total: "19.90"
    }
    then status 202

scenario "rejects a malformed order"
    when POST "/orders" with {
        total: "19.90"
    }
    then status 422
```

```bash
marreta test
```

The publisher is testable this way. The subscribers are not: they run asynchronously,
triggered by messages rather than HTTP requests, so a scenario test (which drives
routes with `when`) does not reach them. You verify the pub/sub delivery by running
the app, next.

## 6. Run it

Start the server:

```bash
marreta serve
```

In another terminal, place an order:

```bash
curl -s -o /dev/null -w '%{http_code}\n' -X POST http://localhost:8080/orders \
  -H 'content-type: application/json' \
  -d '{"id":7,"total":"19.90"}'
# → 202
```

The request returns `202` immediately. In the server log, both subscribers react to
the single event:

```text
... "EMAIL: confirmation for order 7"
... "ANALYTICS: recorded order 7"
```

One publish, two reactions. Add a third subscriber and it joins in without touching
the publisher.

## Result checkpoint

You should now have a route that publishes an `order_placed` event and returns `202`,
and two subscribers that both react to each event. You have seen the pub/sub
difference from a queue: a topic delivers every event to every subscriber.

## Next steps

- [Process work asynchronously with a queue](../how-to/async-work-with-a-queue.md):
  point-to-point work, where one consumer handles each message.
- [Providers](../concepts/providers.md): the messaging provider behind `topic` and
  `queue`.
- [Configuration](../reference/configuration.md): the `MARRETA_QUEUE_*` and
  `MARRETA_TOPIC_EXCHANGE` variables.
