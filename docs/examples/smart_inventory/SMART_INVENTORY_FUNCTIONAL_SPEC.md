# Smart Inventory Functional Spec

Status: Benchmark Draft

## 1. Purpose

Smart Inventory is a maturity benchmark application for Marreta.

The goal is not to define a new language feature. The goal is to verify how naturally the current language can implement a realistic event-driven service using the features that already exist:

- HTTP routes
- schemas
- tasks
- relational DB
- cache
- queue consumers
- topic consumers
- document store
- scenario tests
- functional tests
- runtime event logs and trace propagation

Each implemented capability should be classified after the implementation as:

- `Natural`: expressed directly with existing Marreta primitives.
- `Workaround`: possible, but requires awkward structure, duplicated logic, or operational coupling.
- `Gap`: not possible or not appropriate without a future language/runtime feature.

## 2. Business Description

Smart Inventory manages product availability for a supply-chain system.

The service stores the consolidated inventory state, accepts stock reservations, processes incoming shipments and cancellations asynchronously, emits domain events, and keeps an auditable event log that can be used to rebuild inventory state.

This benchmark intentionally avoids scheduler/cron behavior and DB-offline guarantees. Those concerns are valuable, but they would test infrastructure capabilities rather than Marreta's current application composition model.

## 3. Components

### 3.1 Relational DB

The relational database stores the consolidated state.

Required tables:

- `products`: SKU metadata and current stock.
- `reservations`: active stock reservations.

Expected role:

- Source of truth for current stock.
- Source of truth for active reservations.
- Target of reconciliation corrections.

### 3.2 Cache

The cache stores a fast stock projection by SKU.

Expected keys:

- `stock:{sku}` -> current available quantity.
- `movement_count:{sku}` -> number of processed stock movement events for benchmark logic.

Expected role:

- Fast pre-check before reservation.
- Immediate projection update after stock mutations.
- Optional counter store for reconciliation trigger evaluation.

### 3.3 Queue

Queues are used for external asynchronous inputs that should be processed by the inventory service.

Required queues:

- `incoming_shipment`: external shipment arrivals.
- `order_cancelled`: cancellation events from the order system.

Expected role:

- Buffer external inputs.
- Let consumers update DB/cache/doc store and publish domain events.

### 3.4 Topic

Topics are used for domain events emitted by the inventory service.

Required topics:

- `inventory.increased`
- `inventory.reserved`
- `inventory.cancelled`
- `inventory.low_stock`

Expected role:

- Notify downstream systems.
- Validate topic fan-out semantics.
- Feed an internal audit subscriber.

### 3.5 Document Store

The document store keeps the stock movement log.

Expected collection:

- `inventory_events`

Expected role:

- Append one document for every stock-changing event.
- Preserve enough data to reconstruct a SKU balance.
- Store snapshots such as cancellation reason and low-stock marker.

## 4. Domain Model

### 4.1 Product

Fields:

- `id: integer`
- `sku: string`
- `name: string`
- `initial_stock: integer`
- `current_stock: integer`
- `reserved_stock: integer`
- `low_stock_threshold: integer`

Rules:

- `sku` is the public inventory identifier.
- `current_stock` is the available stock after reservations.
- `reserved_stock` tracks stock currently reserved but not yet fulfilled or cancelled.
- `low_stock_threshold` is derived from the initial stock for test data. For this benchmark, use `ceil(initial_stock * 0.05)`.

### 4.2 Reservation

Fields:

- `id: integer`
- `order_id: string`
- `sku: string`
- `quantity: integer`
- `status: string`

Allowed statuses:

- `active`
- `cancelled`

### 4.3 Inventory Event Document

Fields:

- `event_id: string`
- `event_type: string`
- `sku: string`
- `quantity: integer`
- `order_id: string?`
- `reason: string?`
- `stock_after: integer`
- `status_critical: boolean`
- `created_at: string`

Expected event types:

- `shipment_received`
- `order_reserved`
- `order_cancelled`
- `low_stock_detected`
- `reconciliation_applied`
- `audit_recorded`

## 5. Functional Rules

### RN01 - Reserve Stock

When the API receives a reservation request:

1. Validate the request payload with a schema.
2. Read the projected stock from cache.
3. Reject the reservation when cached stock is lower than requested quantity.
4. Decrement stock in the relational DB.
5. Create an active reservation.
6. Update the cache projection.
7. Append an `order_reserved` document to `inventory_events`.
8. Publish `inventory.reserved` to the topic.
9. If stock after reservation is lower than or equal to the low-stock threshold, publish `inventory.low_stock` and append a `low_stock_detected` document with `status_critical: true`.

Expected classification target:

- Should be `Natural`.

### RN02 - Incoming Shipment

When a message arrives on `incoming_shipment`:

1. Validate the message with a schema.
2. Increase stock in the relational DB.
3. Update the cache projection.
4. Append a `shipment_received` document to `inventory_events`.
5. Publish `inventory.increased` to the topic.

Expected classification target:

- Should be `Natural`.

### RN03 - Cancellation Compensation

When a message arrives on `order_cancelled`:

1. Validate the message with a schema.
2. Find the active reservation by `order_id`.
3. Mark the reservation as `cancelled`.
4. Return the reserved quantity to stock in the relational DB.
5. Update the cache projection.
6. Append an `order_cancelled` document to `inventory_events`, including the cancellation reason.
7. Publish `inventory.cancelled` to the topic.

Expected classification target:

- Should be `Natural` or `Workaround`, depending on how cleanly the reservation lookup/update flow reads in Marreta.

### RN04 - Manual Reconciliation

When an admin route receives a reconciliation request for a SKU:

1. Read all inventory event documents for the SKU.
2. Recalculate expected stock from event quantities.
3. Compare the calculated value with the relational DB value.
4. If values differ, update relational DB and cache to the recalculated value.
5. Append a `reconciliation_applied` document to `inventory_events`.

This benchmark uses an explicit admin route instead of cron/scheduler.

Expected classification target:

- Should be `Natural` if document query and accumulation are expressive enough.
- May be `Workaround` if event replay requires awkward loops or manual data reshaping.

### RN05 - Internal Audit Subscribers

When an inventory domain event is published:

1. An internal exact-topic subscriber receives it.
2. The subscriber writes an audit document to the document store.
3. The subscriber writes a cache marker or log entry that makes the delivery verifiable in functional tests.

This benchmark follows Spec 013: topic subscriptions are exact strings, not wildcard patterns. To consume multiple domain events, declare one `on topic` handler per topic.

Expected classification target:

- Should be `Natural`.

## 6. HTTP API

### 6.1 Reserve Stock

Route:

```text
POST /inventory/reserve
```

Request:

```json
{
  "order_id": "ord-1001",
  "sku": "SKU-RED-001",
  "quantity": 2
}
```

Success response:

```json
{
  "reserved": true,
  "order_id": "ord-1001",
  "sku": "SKU-RED-001",
  "quantity": 2,
  "stock_after": 18
}
```

Insufficient stock response:

```json
{
  "reserved": false,
  "sku": "SKU-RED-001",
  "available": 1,
  "requested": 2
}
```

### 6.2 Get Stock Projection

Route:

```text
GET /inventory/:sku
```

Response:

```json
{
  "sku": "SKU-RED-001",
  "stock": 18,
  "source": "cache"
}
```

### 6.3 Reconcile SKU

Route:

```text
POST /inventory/:sku/reconcile
```

Response:

```json
{
  "sku": "SKU-RED-001",
  "before": 17,
  "after": 18,
  "changed": true
}
```

## 7. Queue Inputs

### 7.1 incoming_shipment

Message:

```json
{
  "shipment_id": "ship-9001",
  "sku": "SKU-RED-001",
  "quantity": 50
}
```

Expected result:

- DB stock increases.
- Cache stock updates.
- `shipment_received` document is created.
- `inventory.increased` topic event is published.

### 7.2 order_cancelled

Message:

```json
{
  "order_id": "ord-1001",
  "reason": "customer_changed_mind"
}
```

Expected result:

- Reservation status changes to `cancelled`.
- DB stock increases by reservation quantity.
- Cache stock updates.
- `order_cancelled` document is created with reason.
- `inventory.cancelled` topic event is published.

## 8. Topic Events

### 8.1 inventory.reserved

Payload:

```json
{
  "event_type": "inventory.reserved",
  "order_id": "ord-1001",
  "sku": "SKU-RED-001",
  "quantity": 2,
  "stock_after": 18
}
```

### 8.2 inventory.increased

Payload:

```json
{
  "event_type": "inventory.increased",
  "shipment_id": "ship-9001",
  "sku": "SKU-RED-001",
  "quantity": 50,
  "stock_after": 68
}
```

### 8.3 inventory.cancelled

Payload:

```json
{
  "event_type": "inventory.cancelled",
  "order_id": "ord-1001",
  "sku": "SKU-RED-001",
  "quantity": 2,
  "stock_after": 20,
  "reason": "customer_changed_mind"
}
```

### 8.4 inventory.low_stock

Payload:

```json
{
  "event_type": "inventory.low_stock",
  "sku": "SKU-RED-001",
  "stock_after": 2,
  "threshold": 3
}
```

## 9. Functional Test Plan

### TF01 - Reserve Stock Updates DB, Cache, Topic, and Document Store

Action:

1. Seed product `SKU-RED-001` with stock `20`.
2. Seed cache key `stock:SKU-RED-001` with `20`.
3. Call `POST /inventory/reserve` with quantity `2`.

Expected:

- Response says `reserved: true`.
- Relational DB stock is `18`.
- Cache stock is `18`.
- `inventory_events` contains `order_reserved`.
- Topic subscriber/audit path observes `inventory.reserved`.

### TF02 - Incoming Shipment Is Processed Asynchronously

Action:

1. Publish message to `incoming_shipment` with quantity `50`.
2. Wait for consumer processing.

Expected:

- Relational DB stock increases by `50`.
- Cache stock matches DB.
- `inventory_events` contains `shipment_received`.
- Topic subscriber/audit path observes `inventory.increased`.

### TF03 - Cancellation Compensation Restores Stock

Action:

1. Reserve stock for `order_id = ord-1001`.
2. Publish message to `order_cancelled` for the same order.
3. Wait for consumer processing.

Expected:

- Reservation status is `cancelled`.
- Relational DB stock is restored by reserved quantity.
- Cache stock matches DB.
- `inventory_events` contains `order_cancelled` with reason.
- Topic subscriber/audit path observes `inventory.cancelled`.

### TF04 - Low Stock Alert

Action:

1. Seed product with initial stock `100` and threshold `5`.
2. Make reservations until stock is `5` or lower.

Expected:

- `inventory.low_stock` is published.
- `inventory_events` contains `low_stock_detected`.
- Low-stock document has `status_critical: true`.

### TF05 - Manual Reconciliation Restores DB Projection

Action:

1. Produce a known sequence of shipment/reservation/cancellation events.
2. Manually alter DB stock to an incorrect value.
3. Call `POST /inventory/:sku/reconcile`.

Expected:

- Route recalculates stock from document events.
- DB stock is corrected.
- Cache stock is corrected.
- `inventory_events` contains `reconciliation_applied`.

### TF06 - Trace and Runtime Events Are Useful

Action:

1. Send a traced HTTP reservation request.
2. Trigger an async shipment flow.

Expected:

- Request logs contain `kind: "request"`.
- App logs contain `kind: "app_log"` when used.
- Consumer logs contain `kind: "consumer"` for queue/topic handlers.
- `trace_id` is preserved across HTTP -> queue/topic -> consumer paths when trace context is enabled.

## 10. Maturity Evaluation Matrix

The implementation review must fill this matrix after the benchmark is implemented.

| Capability | Result | Notes |
| --- | --- | --- |
| HTTP reserve flow | TBD | |
| Schema validation | TBD | |
| Relational consolidated stock | TBD | |
| Cache stock projection | TBD | |
| Queue shipment ingestion | TBD | |
| Queue cancellation ingestion | TBD | |
| Topic domain events | TBD | |
| Topic fan-out audit subscriber | TBD | |
| Document event log | TBD | |
| Manual reconciliation | TBD | |
| Cross-component functional tests | TBD | |
| Trace propagation across async paths | TBD | |
| Runtime event log usefulness | TBD | |

Allowed result values:

- `Natural`
- `Workaround`
- `Gap`

## 11. Success Criteria

The benchmark succeeds when:

1. The service can be implemented without adding new Marreta language/runtime features.
2. Functional tests prove the main HTTP and async flows.
3. The implementation review clearly identifies every workaround.
4. Any missing capability is documented as a gap instead of hidden behind infrastructure-specific behavior.
5. The final service remains readable as a Marreta application, not as a collection of framework workarounds.

## 12. Non-Goals

This benchmark does not test:

- cron or scheduler support
- DB-offline writes
- exactly-once distributed processing
- transactional outbox
- public runtime image distribution
- hard latency SLOs such as cache reads under 10ms
- cloud-provider-specific semantics
- multi-service orchestration beyond the local functional environment

## 13. Expected Project Structure

Expected folder layout:

```text
examples/smart_inventory/
  SMART_INVENTORY_FUNCTIONAL_SPEC.md
  IMPLEMENTATION_REVIEW.md
  app.marreta
  marreta.env
  docker-compose.yml
  routes/
    inventory.marreta
    admin.marreta
  schemas/
    inventory.marreta
  tasks/
    inventory.marreta
    reconciliation.marreta
  tests/
    inventory_test.marreta
  test.sh
```

Only this spec file is required in the first step. The implementation files should be added in a later implementation pass.
