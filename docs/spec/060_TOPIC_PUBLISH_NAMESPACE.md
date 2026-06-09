# 060 - Topic Publish Namespace (`topic.publish`)

> Status: Delivered
> Type: Language syntax / messaging
> Scope: Split the producer side of messaging so topics publish through a dedicated
> `topic` namespace and queues keep `queue.push`, making the producer side
> symmetric with the consumer side (`on topic` / `on queue`). Pre-release breaking
> change: `queue.publish` is removed in favor of `topic.publish`.

---

## 1. Purpose

Today the producer side is asymmetric and confusing:

- `queue.push "name", payload` sends to a point-to-point **queue**.
- `queue.publish "name", payload` publishes to a pub/sub **topic** — but under the
  `queue` namespace.

Yet the consumer side already separates them: `on queue "name"` reads a queue and
`on topic "name"` reads a topic. So a topic is published through `queue.*` but
consumed through `on topic` — the two halves do not line up.

There is also a **bug**: the tooling catalog already advertises a `topic` namespace
and `topic.publish`, but the parser does not accept `topic.publish` (it is a
`source_load_error`). So the editor suggests `topic.publish`, which then fails to
parse.

This spec makes the producer side symmetric with the consumer side and fixes the
catalog: **topics publish via `topic.publish`, queues push via `queue.push`**.

## 2. The change

### 2.1 Producer syntax

- Queue (point-to-point), unchanged:
  - `queue.push "queue.name" [as Schema], payload`
  - pipeline form: `value >> queue.push("queue.name")`
- Topic (pub/sub), new namespace:
  - `topic.publish "topic.name" [as Schema], payload`
  - pipeline form: `value >> topic.publish("topic.name")`

`queue.publish` is **removed** (see 2.3).

### 2.2 Consumer syntax (unchanged)

`on queue "name" take binding [as Schema]` and `on topic "name" take binding
[as Schema]` stay exactly as they are. After this spec the pairing is symmetric:

- `queue.push` ↔ `on queue`
- `topic.publish` ↔ `on topic`

### 2.3 Breaking: remove `queue.publish`

`queue.publish` is removed, not aliased. This is a pre-release window (no public
release has shipped), so a clean cutover is preferred over carrying a deprecated
alias. A `.marreta` file using `queue.publish` after this spec fails to load with a
clear error pointing at `topic.publish`.

## 3. Implementation outline

- **AST**: the `QueuePublish` node already exists (`topic`, `schema`, `payload`); it
  is retained and now reached only via `topic.publish` (rename to `TopicPublish` for
  clarity).
- **Lexer/parser**: accept `topic.publish` (statement and pipeline forms) the same
  way `queue.push` is parsed; stop accepting `queue.publish`.
- **Interpreter**: dispatch `topic.publish` to the same topic-publish path
  `queue.publish` used; the queue driver/topic semantics are unchanged.
- **Operational surface (rename, not just parser)**: every user-facing string for
  this operation moves from `queue.publish` to `topic.publish` — the `operation`
  field on errors, the type/validation error messages, the provider/driver error
  text, and any log/trace operation name (today in `src/interpreter.rs`, e.g. the
  `operation: "queue.publish"` and `"queue.publish: …"` messages). After this spec no
  user-facing message, `operation`, or tooling string announces `queue.publish`.
- **Scenario testing (mock contract)**: the test runner maps the publish expression
  to a `given` key (today `GivenKey { namespace: "queue", method: "publish" }` in
  `src/scenario_tests.rs`). It moves to `namespace: "topic", method: "publish"`, so
  `given topic.publish "t", anything returns …` mocks the published operation and
  matches a route that publishes with `topic.publish`. The old
  `given queue.publish …` key no longer exists.
- **Catalog/tooling**: the `topic` namespace and `topic.publish` entries already
  exist in the catalog — they become correct. Remove any `queue.publish` entry.
  Completions/hover follow.
- **Editor assets**: grammar (`tmLanguage`) and snippets updated to `topic.publish`.
- **Project sources**: update `.marreta` files that use `queue.publish` in
  `functional_tests`, `e2e`, and the example/benchmark projects.
- **Docs**: update the still-authoritative user/contract docs — notably
  `docs/spec/013_QUEUE.md` (the queue module's base contract) and the testing DSL doc
  `docs/spec/023_TESTING_DSL.md` where it documents the `given` for publish — plus the
  SPEC.md messaging section. Older delivered specs that mention `queue.publish` only
  as historical session records are left as-is (they document what was true then), not
  rewritten, per the repo convention.

## 4. Out of scope

- Consumer syntax (`on queue` / `on topic`) — unchanged.
- Queue/topic provider semantics, ack/nack, schemas — unchanged.
- Any change to `queue.push`.

## 5. Acceptance criteria

1. `topic.publish "t", payload` and `value >> topic.publish("t")` parse, lint, and
   run, publishing to the topic exactly as `queue.publish` did.
2. `queue.push` is unchanged; `queue.publish` no longer parses and fails to load
   with an error that points at `topic.publish`.
3. The tooling catalog advertises `topic.publish` (and a `topic` namespace) that
   actually parse; no `queue.publish` entry remains. Completions/hover agree.
4. Grammar and snippets use `topic.publish`; the extension still loads.
5. Scenario testing: `given topic.publish "t", anything returns true` mocks the
   publish, and a route that publishes via `topic.publish` passes a scenario asserting
   it; the old `given queue.publish …` key no longer resolves.
6. No user-facing surface still announces `queue.publish`: error messages, the
   `operation` field, driver/provider errors, logs/trace operation names, and tooling
   strings all read `topic.publish`. (Older delivered specs kept as historical records
   are the only allowed mentions.)
7. All `.marreta` sources in the repo use the new form, and the authoritative
   user/contract docs (013, 023, SPEC.md) are updated; `functional_tests`,
   `migrations_functional`, and `e2e` pass.
8. Standard gates: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
   the full test suite, `functional_tests`, and `migrations_functional` green.

## 6. Delivery notes

- **AST**: `QueuePublish` renamed to `TopicPublish` (fields unchanged) across all
  consumers (`ast`, `parser`, `interpreter`, `lint`, `openapi`, `route_loader`,
  `doctor`, `scenario_tests`).
- **Parser** (`src/parser.rs`): the `queue` producer arm now accepts only `push`;
  a new `topic` producer arm accepts `publish` (statement and `>>` pipeline forms).
  `queue.publish` no longer parses.
- **Interpreter**: `eval_queue_publish` → `eval_topic_publish`; the operation name
  and all error messages read `topic.publish`. Topic semantics/driver path unchanged.
- **Scenario testing**: the publish `GivenKey` moved from `queue`/`publish` to
  `topic`/`publish`, so `given topic.publish …` mocks the operation; producer and
  given share the same `expression_to_matcher`, so they stay consistent.
- **Catalog/tooling**: already advertised `topic.publish` + the `topic` namespace
  (and no `queue.publish`) — now correct against the parser.
- **Sources/docs**: `.marreta` in `functional_tests`, `omni_hub`, `smart_inventory`
  migrated; authoritative docs `013_QUEUE.md`, `023_TESTING_DSL.md`, and SPEC.md
  messaging updated. Older delivered specs left as historical.
- **Tests**: queue/parser publish unit tests migrated to `topic.publish`.
- **Semantics certification (real RabbitMQ)**: a functional test in
  `functional_tests/routes/queue.marreta` + `test.sh` certifies the queue↔topic
  distinction end to end — two consumers on `ft.fanout` each receive every
  `topic.publish` (N publishes → 2N receipts, fan-out), while two consumers on
  `ft.compete` compete on `queue.push` (N pushes → N receipts, point-to-point). The
  N-vs-2N counter contrast proves both semantics at the broker level.
- **Gates**: fmt + clippy(`-D warnings`) clean; suite 1480 + 3 + 35 + 37;
  `functional_tests` 557/557 (after rebuilding `marreta-lang:dev`);
  `migrations_functional` PASS; `e2e` green.

---

## P.S. Do not forget the docs of record

On delivery, update both `CHANGELOG.md` and `docs/spec/SPEC.md`. See SPEC.md
section 1.3.
