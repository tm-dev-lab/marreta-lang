# Omni Hub API

## Purpose

This example exists to validate the current state of the language against a realistic service-order workflow.

It is not only a CRUD example. It must prove that the language can coordinate:

- relational persistence as the source of truth
- read-through cache
- topic-based fan-out notifications
- queue-based asynchronous billing
- immutable document snapshots for audit

At the end of the implementation, each requirement should be classified as:

- `clean`
- `possible with workaround`
- `not possible yet`

Only language-level friction should count as a language improvement candidate. Test observability that can be solved through logs, direct container inspection, or infrastructure checks must not be treated as a language gap.

## Business Description

The API manages the lifecycle of service orders.

The system must guarantee:

- safe persistence of master data
- low-latency reads for active orders
- immutable audit history
- asynchronous integration with billing and marketing

## Business Components

### Relational Database

The relational database is the source of truth.

It stores:

- customers
- service orders
- current order status
- relational integrity between entities

### Cache

The cache optimizes read performance.

It stores frequently accessed order details so the API can serve repeated reads without hitting the relational database every time.

### Topic

The topic is used for one-to-many event distribution.

When a relevant event happens, multiple downstream consumers may react independently without affecting the HTTP response path.

Concrete topic name for this example:

- `order_created`

### Queue

The queue is used for one-to-one background processing.

It must hold billing commands durably until a consumer processes them.

Concrete queue name for this example:

- `process_billing`

### Document Database

The document database stores immutable audit snapshots.

When an order is completed, the system must archive a frozen JSON snapshot that preserves the historical state as it existed at completion time.

## Domain Model

### Customer

Minimum persistent fields:

- `id: integer`
- `name: string`
- `email: string`

### ServiceOrder

Minimum persistent fields:

- `id: integer`
- `customer: Customer`
- `description: string`
- `total_amount: float`
- `status: string`

Valid status values for this example:

- `OPEN`
- `CLOSED`

### Audit Snapshot

The audit snapshot must be stored as a document and must contain at least:

- `order_id`
- `customer_id`
- `customer_name`
- `description`
- `total_amount`
- `status`
- `completed_at`

`customer_name` must be copied from the relational data at completion time and must remain frozen in the document snapshot even if the customer record changes later.

## Cache Keys

Concrete cache keys for this example:

- `order:{id}`

This example does not require customer-wide collection caches.

## Business Rules

### RN01 - Mandatory Persistence

An order must not be considered created unless the relational write succeeds.

### RN02 - Cache Coherence

Whenever an order is updated or completed, the cached `order:{id}` entry must be deleted.

### RN03 - Event Segregation

Creating an order must publish an event to the `order_created` topic.

The HTTP response must not depend on downstream notification delivery.

### RN04 - Billing Guarantee

Completing an order must enqueue a command in `process_billing`.

Billing must be asynchronous and must not block the user-facing response.

### RN05 - Immutable Audit Snapshot

Completing an order must persist an immutable document snapshot using the values as they existed at completion time.

If relational data changes later, the document snapshot must not change.

## Endpoints

### POST /orders

Behavior:

1. Persist a new order in the relational database with status `OPEN`
2. Publish an `order_created` event to the topic
3. Invalidate `order:{id}` if present
4. Return `201`

Suggested response shape:

```json
{
  "id": 123,
  "status": "OPEN"
}
```

### GET /orders/{id}

Behavior:

1. Check cache key `order:{id}`
2. If present, return the cached value
3. If absent, load from the relational database
4. Save the result into cache
5. Return `200`

### PATCH /orders/{id}/complete

Behavior:

1. Update the relational order status to `CLOSED`
2. Build and persist the immutable audit snapshot in the document database
3. Enqueue a billing command in `process_billing`
4. Delete cache key `order:{id}`
5. Return `200`

Suggested response shape:

```json
{
  "id": 123,
  "status": "CLOSED"
}
```

## Functional Test Plan

### TF01 - Order Creation and Topic Notification

Action:

- send `POST /orders`

Expected result:

- HTTP status `201`
- a relational row is created
- an event is published to topic `order_created`

Validation strategy:

- verify the row directly in the relational database
- verify topic behavior through a test subscriber, broker inspection, or application logs

### TF02 - Read Performance and Cache Consistency

Action:

- call `GET /orders/{id}` twice

Expected result:

- first request loads from relational storage and populates cache
- second request is served through cache

Validation strategy:

- response body must stay consistent across both calls
- direct Redis inspection may be used to verify key creation
- logs may be used to prove `cache miss -> db fetch -> cache hit`

This observability requirement is considered infrastructure/test harness, not a language gap.

### TF03 - Completion and Audit Snapshot

Action:

- send `PATCH /orders/{id}/complete`

Expected result:

- HTTP status `200`
- relational `status` becomes `CLOSED`
- a document snapshot is created with frozen values
- cache key `order:{id}` is invalidated

Validation strategy:

- verify relational row directly in the database
- verify the document directly in the document database
- verify cache invalidation directly in Redis

### TF04 - Billing Resilience

Action:

- send `PATCH /orders/{id}/complete`
- observe the billing queue

Expected result:

- a message is enqueued in `process_billing`
- if the consumer is offline, the message remains queued

Validation strategy:

- inspect queue state directly in the broker
- this persistence proof belongs to infrastructure-level validation, not to language semantics

### TF05 - Snapshot Immutability

Action:

1. create and complete an order
2. update the customer name in the relational database
3. inspect the stored audit document

Expected result:

- the audit document still contains the original customer name captured at completion time

This test is mandatory because it proves the snapshot is historical, not live-linked.

## Success Criteria

- the API can serve repeated `GET /orders/{id}` from cache after the initial load
- completing an order updates relational state, archives a document snapshot, enqueues billing, and invalidates cache
- marketing notifications and billing dispatch do not block the HTTP response path
- the audit snapshot remains readable and historically correct even if the relational source changes later

## Observability Guidance

This example is allowed to use:

- application logs
- direct PostgreSQL inspection
- direct Redis inspection
- direct MongoDB inspection
- direct RabbitMQ inspection

These checks are valid as test infrastructure and must not be recorded as language improvements unless the business rule itself could not be expressed cleanly.

## Evaluation Notes

When implementing this example, record any finding in one of these buckets:

- `clean`: the language expressed the requirement naturally
- `possible with workaround`: the behavior was achievable, but the implementation shape felt wrong or overly manual
- `not possible yet`: the language could not express the requirement satisfactorily

The final evaluation should focus on language shape, not on container orchestration or test harness inconvenience.
