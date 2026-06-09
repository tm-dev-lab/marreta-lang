# Smart Inventory Implementation Review

Status: Implemented

The first Smart Inventory pass was implemented without adding new Marreta language or runtime features.

Functional validation:

```text
./examples/smart_inventory/test.sh
Results: 30 passed, 0 failed / 30 total
```

The benchmark validates:

- scenario test smoke coverage
- DB-backed product and reservation state
- Redis-backed stock projection
- RabbitMQ queue consumers
- RabbitMQ exact-topic audit subscribers
- MongoDB inventory event log
- manual reconciliation from document events
- runtime consumer events, app logs, and async trace propagation

## Maturity Matrix

| Capability | Result | Notes |
| --- | --- | --- |
| HTTP reserve flow | Natural | Route, schema validation, DB update, cache update, doc write, and topic publish compose directly. |
| Schema validation | Natural | HTTP payloads and queue payloads use `take ... as Schema` without extra glue. |
| Relational consolidated stock | Natural | Persistent schemas and direct/pipeline DB operations are sufficient. |
| Cache stock projection | Natural | `cache.get/set/delete` maps directly to projection behavior. |
| Queue shipment ingestion | Natural | `queue.push` plus `on queue` expresses external async input cleanly. |
| Queue cancellation ingestion | Natural | Lookup/update compensation flow is direct; transaction block is useful for DB state changes. |
| Topic domain events | Natural | `topic.publish` expresses domain events with no extra abstraction. |
| Topic audit subscribers | Natural | Spec 013 topics are exact strings, so the benchmark declares one `on topic` handler per inventory domain event and delegates shared behavior to `record_inventory_audit`. |
| Document event log | Natural | `doc.inventory_events.save` and document query pipelines are enough for the audit/event log. |
| Manual reconciliation | Natural | `doc.* >> where >> fetch_all` plus `reduce` expresses event replay directly. |
| Cross-component functional tests | Natural | Shell-level functional test can validate DB/Redis/Mongo/RabbitMQ/server behavior end-to-end. |
| Trace propagation across async paths | Natural | Functional test sends W3C `traceparent` through an HTTP queue producer and verifies the consumer runtime event keeps the same `trace_id`. |
| Runtime event log usefulness | Natural | `kind: "consumer"` and `log.info` app logs are visible enough to validate async processing. |

Allowed result values:

- `Natural`
- `Workaround`
- `Gap`

## Workarounds Observed

No workaround in the scoped benchmark requires a language feature.

The only test-specific workaround is explicit cleanup of known cache keys during `/inventory/seed`. This is acceptable for benchmark setup and should not become `cache.delete_prefix` in the language.

Scheduler/cron and DB-offline guarantees are intentionally excluded. Those remain outside this benchmark by design, not hidden implementation gaps.

## Language Findings

### Fixed language issue: topic wildcards

The first implementation used one wildcard subscriber:

```marreta
on topic "inventory.*" take event
```

That was not intended Marreta semantics. Topics in Marreta are exact strings, not RabbitMQ binding patterns. The language now rejects `*` and `#` in `on topic` declarations, and the benchmark declares one exact handler per domain event:

```marreta
on topic "inventory.reserved" take event
    record_inventory_audit(event)

on topic "inventory.increased" take event
    record_inventory_audit(event)

on topic "inventory.cancelled" take event
    record_inventory_audit(event)

on topic "inventory.low_stock" take event
    record_inventory_audit(event)
```

This was the only benchmark finding that required a language/runtime correction.

### No remaining language gaps from this benchmark

After the exact-topic correction, the remaining observations are application or architecture concerns, not Marreta language gaps:

| Observation | Classification | Decision |
| --- | --- | --- |
| Cache cleanup by prefix | Not a language feature | Marreta should not provide prefix flush/delete helpers. They can degrade Redis and encourage broad destructive operations. The benchmark uses explicit key deletion because it is test setup. |
| `/inventory/seed` mixes teardown and creation | Test harness concern | Seed endpoints are benchmark convenience, not production language design. Multi-backend teardown belongs to tests or app tooling, not the core language. |
| Topic self-loop risk | Coding error | With exact topics, a loop only happens if code subscribes to and republishes the same topic intentionally. The runtime should not hide that bug behind a guard. |
| Cross-backend consistency / outbox / saga | Application architecture pattern | Marreta exposes DB, cache, document, and queue primitives. It should not imply distributed atomicity across them. Outbox, retry, and saga patterns remain explicit application design. |
| Request idempotency | Application pattern | Marreta already provides the needed primitives: `cache.set(... only_if_absent: true)`, DB constraints, `transaction`, and `db.native_query`. A generic language primitive would be misleading because idempotency keys and semantics are domain-specific. |
| Concurrent stock reservation | Natural via escape hatch | The high-level DB API is intentionally simple. For conditional atomic updates, `db.native_query` is the correct language feature, not a workaround. |

The benchmark therefore does not justify adding new high-level abstractions such as `cache.delete_prefix`, automatic idempotency middleware, topic loop detection, distributed transactions, outbox primitives, or a combined "save document and publish topic" operation.

## Native Query Use

`db.native_query` should be treated as an intentional feature of the language, not as a workaround. It exists for cases where the provider-native database operation is the clearest and safest expression of the requirement.

For example, a production-grade stock reservation should avoid a read-modify-write race. The correct implementation style is an atomic SQL update:

```marreta
updated = db.native_query("
  update products
  set current_stock = current_stock - $1,
      reserved_stock = reserved_stock + $1
  where sku = $2
    and current_stock >= $1
  returning *
", [payload.quantity, payload.sku])
```

If `updated` is empty, the reservation fails because stock was insufficient at update time. This is database-native concurrency control exposed through an explicit Marreta escape hatch.

## Notes

- Topic subscribers are exact-topic handlers, not wildcard handlers. The repeated `on topic` declarations are intentional and align the benchmark with Spec 013.
- Reconciliation is explicit through `POST /inventory/:sku/reconcile`; no scheduler is assumed.
- Document writes and topic publication remain separate operations by design. Marreta should not provide a combined primitive because it would couple two infrastructure components and imply atomicity the runtime cannot guarantee.
