# 027 Time API

> Status: Delivered
> Type: Language Feature

## Goal

Add a coherent date/time API to the language, with a dedicated namespace, first-class temporal values, interval support, formatting support, and consistent behavior across runtime, JSON, and infrastructure modules.

## Delivery Notes

Delivered in `feature/time-027` through:

- runtime/native values for `instant`, `date`, `time`, `duration`, and `interval`
- namespace surface:
  - `time.now()`
  - `time.today()`
  - `time.parse(...)`
  - `time.date(...)`
  - `time.at(...)`
  - `time.instant(...)`
  - `time.days(...)`
  - `time.hours(...)`
  - `time.minutes(...)`
  - `time.seconds(...)`
  - `time.interval(...)`
  - `time.contains(...)`
  - `time.overlaps(...)`
  - `time.format(...)`
  - `time.from_unix(...)`
  - `time.unix(...)`
- schema types:
  - `instant`
  - `date`
  - `time`
  - `duration`
  - `interval`
- timezone-aware local views driven by `MARRETA_TIMEZONE`
- native transport through:
  - HTTP/contracts/tasks
  - `db`
  - `doc`
  - `cache`
  - `queue`
- functional validation through `examples/functional_tests`

## Motivation

The `omni_hub` example exposed a real gap: the language has no official way to represent the moment an order was completed, nor any explicit model for temporal values.

Today this forces workarounds such as plain strings or manual composition.

The correct problem statement is not only “missing `now()`”. The real problem is the absence of a proper time API.

## Namespace Direction

The recommended namespace is:

- `time`

Reasons:

- it covers both date and time without sounding overly technical
- it scales better than a loose builtin
- it matches the existing style of explicit namespaces such as `db`, `doc`, `cache`, and `queue`

Examples:

```marreta
closed_at = time.now()
today = time.today()
parsed = time.parse("2026-04-27T13:10:45Z")
```

## Timezone

The language should follow these rules:

1. `instant` represents an absolute point in time.
2. The default serialized form of `instant` is always UTC.
3. The runtime may have a local project/runtime timezone configured by environment:
   - `MARRETA_TIMEZONE`
4. If `MARRETA_TIMEZONE` is not set, the default is `UTC`.
5. The configured timezone affects local contextual operations such as:
   - `time.today()`
   - extracting local parts from an `instant`, such as `.hour`, `.day`, `.weekday`
6. The configured timezone does not change the absolute nature of `instant`.

## Non-Goals

- introducing a full locale/calendar engine in the first cut
- introducing broad arbitrary-timezone support in the first cut
- introducing a full duration-arithmetic library in the first cut
- trying to solve every temporal problem in a single implementation slice

## What a Language Needs From Time

From a language-design perspective, time normally requires at least these layers:

1. Obtain the current instant
2. Represent temporal values natively
3. Serialize and deserialize them predictably
4. Compare temporal values
5. Work with pure dates and pure times when needed
6. Define clear timezone/UTC rules
7. Perform basic addition/subtraction with durations
8. Work with intervals
9. Format temporal values for presentation
10. Persist and transport these values across `db`, `doc`, `cache`, `queue`, HTTP, and tests

This spec should address the whole conceptual model even if implementation is staged later.

## Value Model

### Recommended Runtime Value Types

The healthiest model for the language is to separate these concepts:

1. `Instant`
   - an absolute point in time
   - example: `2026-04-27T13:10:45Z`

2. `Date`
   - a date without time
   - example: `2026-04-27`

3. `Time`
   - a time of day without a date
   - example: `13:10:45`

4. `Duration`
   - an amount of time
   - example: 5 minutes, 2 hours, 3 days

5. `Interval`
   - a range between two compatible temporal values
   - example: from `2026-04-27T09:00:00Z` to `2026-04-27T18:00:00Z`

These five types form the conceptual time model of the language.

## Why These Must Be Real Types

These temporal values must not behave like decorated strings.

They need to be real language types because they carry:

- their own semantic meaning
- compatible arithmetic rules
- compatible comparison rules
- native properties for reading parts
- native conversions where appropriate

If `instant`, `date`, `time`, `duration`, and `interval` only serialize nicely but do not expose useful semantics, then the language has not really solved time; it has only renamed strings.

## Proposed Namespace Surface

### Core Surface

```marreta
time.now()
time.parse("2026-04-27T13:10:45Z")
time.today()
time.interval(start_at, end_at)
time.format(value, "dd/MM/yyyy HH:mm:ss")
```

### Constructors and Helpers

```marreta
time.date("2026-04-27")
time.at("13:10:45")
time.instant("2026-04-27T13:10:45Z")
time.days(2)
time.hours(3)
time.minutes(15)
time.seconds(30)
time.contains(window, value)
time.overlaps(a, b)
time.from_unix(1714223445)
time.unix(value)
```

## Function Purposes

This section exists to record not only syntax, but also intent.

### `time.now()`

Purpose:

- capture the current runtime instant
- record creation, completion, expiration, and audit timestamps
- avoid manual timestamp strings

Natural uses:

- `completed_at`
- `created_at`
- `expires_at`

### `time.today()`

Purpose:

- obtain the current date in the effective project/runtime timezone
- express business rules based on “today” without time-of-day noise

Natural uses:

- daily due dates
- same-day scheduling
- local-date cutoffs

### `time.parse(string)`

Purpose:

- transform textual input into a native temporal value
- avoid comparing strings against temporal values
- make parsing an explicit act in the language

Natural uses:

- temporal query parameters
- dates received from external payloads
- textual values loaded from configuration

### `time.date(string)`

Purpose:

- construct a pure date value
- represent business calendar concepts

Natural uses:

- due dates
- birth dates
- scheduled dates

### `time.at(string)`

Purpose:

- construct a pure time-of-day value
- represent operating hours and service windows

Natural uses:

- opens at `09:00:00`
- closes at `18:00:00`
- standard business-hour rules

### `time.instant(string)`

Purpose:

- construct an absolute instant explicitly
- represent a precise globally comparable moment

Natural uses:

- absolute deadlines
- processing cutoffs
- deterministic replay in tests

### `time.interval(start, end)`

Purpose:

- represent a temporal window with start and end
- avoid ad hoc map-based interval modeling
- make interval semantics a language primitive

Natural uses:

- business hours
- maintenance windows
- validity periods

### `time.contains(interval, value)`

Purpose:

- check whether a temporal value falls inside an interval
- express availability, validity, and active-window rules

Natural uses:

- “are we inside business hours?”
- “is this token still inside its usable window?”
- “is this promotion active now?”

### `time.overlaps(a, b)`

Purpose:

- check whether two intervals overlap partially or fully
- express schedule, booking, or maintenance conflicts

Natural uses:

- appointment conflict detection
- reservation overlaps
- deploy-window collisions

### `time.format(value, mask)`

Purpose:

- turn a temporal value into presentation text
- support human-facing payloads and reports without losing native temporal values in the runtime

Natural uses:

- UI-oriented HTTP responses
- textual reports
- friendly labels in documents/exports

## Semantics

### `time.now()`

- returns an `instant`
- the value represents the current instant in UTC
- it does not return a string

### `time.parse(string)`

- parses textual temporal input into a native temporal value
- implicit parsing never happens
- the initial implementation may begin with ISO-8601 UTC support for `instant`

### `time.date(string)`

- creates a `date`

### `time.at(string)`

- creates a `time`

### `time.instant(string)`

- creates an `instant`

### `time.interval(start, end)`

- creates an `interval`
- `start` and `end` must be compatible
- `start > end` must fail clearly

### `time.format(value, mask)`

- formats a temporal value using a textual mask
- the language should accept broad mask support
- the initial implementation may reuse the runtime temporal engine internally
- however, the language contract remains owned by the spec, not delegated to an internal library
- invalid masks must fail clearly
- incompatible value types must fail clearly

### `time.today()`

- returns the current date
- it uses the effective runtime timezone

### `time.contains(interval, value)`

- returns `true` when `value` is inside `interval`
- returns `false` otherwise
- the types must be compatible

### `time.overlaps(a, b)`

- returns `true` when two intervals overlap
- returns `false` when they do not intersect
- the intervals must belong to the same temporal domain

## Serialization Rules

### JSON

When temporal values are serialized to JSON:

- `Instant` must serialize as an ISO-8601 UTC string
- `Date` must serialize as `YYYY-MM-DD`
- `Time` must serialize as `HH:MM:SS`
- `Duration` must serialize in the language’s canonical textual form
- `Interval` must serialize as an object with `start` and `end`

Example `instant`:

```json
"2026-04-27T13:10:45Z"
```

Example `interval`:

```json
{
  "start": "2026-04-27T09:00:00Z",
  "end": "2026-04-27T18:00:00Z"
}
```

### Maps / Lists

Maps and lists containing temporal values must serialize recursively without requiring manual conversion.

## Comparison Rules

Temporal values must support direct comparison:

```marreta
expired = token.expires_at < time.now()
```

Expected operators:

- `==`
- `!=`
- `<`
- `<=`
- `>`
- `>=`

Invalid comparisons between incompatible temporal types must fail clearly.

## Arithmetic Rules

Expected operations:

- `instant + duration -> instant`
- `instant - duration -> instant`
- `instant - instant -> duration`
- `date + duration(days) -> date`
- `date - duration(days) -> date`
- `date - date -> duration`

Operations outside this set must fail clearly until explicitly specified.

## Type Properties and Native Behavior

The following properties and behaviors make these temporal values real language types rather than presentation wrappers.

### `instant`

Expected properties:

- `value.year`
- `value.month`
- `value.day`
- `value.hour`
- `value.minute`
- `value.second`
- `value.weekday`
- `value.unix`
- `value.date`
- `value.time`

Examples:

```marreta
created_at.year
created_at.weekday
created_at.date
created_at.time
created_at.unix
```

Purpose:

- inspect an absolute timestamp naturally
- extract calendar/time-of-day views from an instant
- support domain rules without string parsing

Precision note:

- `value.unix` returns Unix epoch seconds, following the POSIX/JWT convention
- if millisecond precision is introduced later, it must use a distinct explicit name such as `unix_ms`

### `date`

Expected properties:

- `value.year`
- `value.month`
- `value.day`
- `value.weekday`
- `value.start_of_day`
- `value.end_of_day`

Examples:

```marreta
due_date.month
due_date.weekday
cutoff = due_date.start_of_day
```

Purpose:

- express calendar rules directly
- derive day boundaries without manual composition

Boundary note:

- `value.end_of_day` means the last valid local instant of that date
- it exists specifically to support safe inclusive comparisons such as `<= due_date.end_of_day`

### `time`

Expected properties:

- `value.hour`
- `value.minute`
- `value.second`

Expected helper behavior:

- `value.on(date)` -> combine a `time` with a `date` to produce an `instant`

Examples:

```marreta
opens_at.hour
opens_at.minute
opening_instant = opens_at.on(time.today())
```

Purpose:

- make time-of-day values useful beyond plain textual display
- support daily schedules without forcing string handling

### `duration`

Expected properties:

- `value.total_days`
- `value.total_hours`
- `value.total_minutes`
- `value.total_seconds`

Examples:

```marreta
elapsed.total_minutes
window.duration.total_days
```

Purpose:

- read durations as measurable values
- avoid each app reimplementing “difference in days/hours/minutes”

Precision note:

- `total_days`, `total_hours`, `total_minutes`, and `total_seconds` return decimal values when needed
- example: a duration of 90 minutes yields `total_hours = 1.5`

### `interval`

Expected properties:

- `value.start`
- `value.end`
- `value.duration`

Examples:

```marreta
window.start
window.end
window.duration.total_hours
```

Purpose:

- treat ranges as first-class domain values
- allow range inspection without decomposing into ad hoc maps

## Interval Rules

Creation:

```marreta
window = time.interval(
    time.instant("2026-04-27T09:00:00Z"),
    time.instant("2026-04-27T18:00:00Z")
)
```

Support functions:

```marreta
time.contains(window, time.now())
time.overlaps(a, b)
```

Expected properties:

- `interval.start`
- `interval.end`
- `interval.duration`

These functions exist to avoid every project reimplementing:

- `start <= value and value <= end`
- start/end overlap checks between two intervals

In other words, the language should treat interval logic as a primitive, instead of pushing repeated interval logic into application code.

## Formatting

Text formatting is exposed through:

```marreta
time.format(value, mask)
```

Examples:

```marreta
short_date = time.format(order.due_date, "dd/MM/yyyy")
stamp = time.format(order.completed_at, "yyyy-MM-dd HH:mm:ss")
```

Contract direction:

- the language should support broad masks
- the initial implementation may reuse the runtime formatter internally
- the feature contract is still defined by the language, not outsourced to the internal library
- invalid masks must fail clearly

Product function:

- separate native temporal value semantics from human-facing textual representation
- allow presentation output without giving up native temporal values in domain logic

## Temporal Parts

Temporal values should expose simple parts directly, with low ceremony.

Examples:

- `value.year`
- `value.month`
- `value.day`
- `value.hour`
- `value.minute`
- `value.second`
- `value.weekday`

These parts exist to support simple domain reads without forcing verbose helper functions for every field access.

## Persistence and Transport

The language needs a clear contract for existing modules:

### `db`

- schemas/drivers must be able to persist temporal values without textual workarounds

### `doc`

- documents must accept temporal values naturally

### `cache`

- temporal values must survive round-trips through serialization

### `queue`

- queue/topic payloads must carry temporal values predictably

### HTTP

- responses and payloads must serialize predictably

## Schema Types

The language should support these types in schemas:

```marreta
created_at: instant
billing_date: date
starts_at: time
sla: duration
business_window: interval
```

### Full Schema Example

```marreta
schema ServiceOrder
    db: service_orders

    id: integer
    created_at: instant
    completed_at?: instant
    billing_date?: date
    starts_at?: time
    resolution_sla: duration
    business_window?: interval
```

### Another Example

```marreta
schema Store
    db: stores

    id: integer
    name: string
    opens_at: time
    closes_at: time
    maintenance_window?: interval
```

## Naming Conventions

Temporal fields should follow plain domain naming, with low ceremony and predictable suffixes.

### `instant`

Recommended names:

- `created_at`
- `updated_at`
- `completed_at`
- `expires_at`
- `processed_at`

Use `*_at` when the field represents a precise moment in time.

### `date`

Recommended names:

- `billing_date`
- `due_date`
- `birth_date`
- `scheduled_date`

Use `*_date` when the field represents a calendar date without time-of-day.

### `time`

Recommended names:

- `opens_at`
- `closes_at`
- `starts_at`
- `ends_at`

Use `*_at` when the field represents a time-of-day value in business language.

The schema type still distinguishes it from `instant`.

### `duration`

Recommended names:

- `sla`
- `ttl`
- `grace_period`
- `processing_timeout`
- `retention_period`

Use names that describe the meaning of the amount of time, not its storage format.

### `interval`

Recommended names:

- `business_window`
- `maintenance_window`
- `booking_window`
- `availability_window`
- `validity_window`

Use `*_window` when the field represents a temporal range with a start and an end.

## Schema Design Guidance

Prefer the simplest temporal type that matches the business meaning:

- use `instant` for audit and event timestamps
- use `date` for calendar rules
- use `time` for hours-of-day
- use `duration` for relative time amounts
- use `interval` for bounded windows

Avoid encoding these concepts as plain strings when the temporal meaning is part of the domain.

### Good

```marreta
created_at: instant
due_date: date
opens_at: time
sla: duration
business_window: interval
```

### Avoid

```marreta
created_at: string
due_date: string
opens_at: string
sla: string
business_window: map
```

These forms lose semantic meaning and push temporal correctness into application code instead of the language.

## Usage Examples

### Audit snapshot

```marreta
snapshot = {
    order_id: order.id,
    status: "CLOSED",
    completed_at: time.now()
}
```

### Expiration

```marreta
expired = session.expires_at < time.now()
```

### Explicit parse

```marreta
cutoff = time.parse("2026-04-27T00:00:00Z")
is_old = order.created_at < cutoff
```

### Business window

```marreta
window = time.interval(
    time.instant("2026-04-27T09:00:00Z"),
    time.instant("2026-04-27T18:00:00Z")
)

open_now = time.contains(window, time.now())
```

### Formatting

```marreta
label = time.format(order.completed_at, "dd/MM/yyyy HH:mm")
```

## Remaining Follow-Ups

The main language model is delivered. What remains open is refinement, not capability:

1. define the officially guaranteed public mask subset for `time.format(...)`
2. decide whether deterministic clock control should become a first-class testing surface
3. decide how broad future parsing support should be beyond the currently accepted canonical forms

## Test Plan

1. `time.now()` returns an `instant` runtime value
2. `instant`, `date`, `time`, `duration`, and `interval` serialize correctly
3. maps and lists containing temporal values serialize correctly
4. temporal comparisons behave correctly
5. invalid comparisons fail clearly
6. `time.interval(...)` rejects invalid ranges
7. `time.contains(...)` and `time.overlaps(...)` behave correctly
8. `time.format(...)` formats correctly and rejects invalid masks
9. `MARRETA_TIMEZONE` affects local temporal operations consistently
10. document snapshots can store `time.now()` without workaround strings
11. `db`, `doc`, `cache`, `queue`, and HTTP preserve temporal values consistently

## Validation Summary

Validated in this branch with:

- targeted runtime/unit coverage for the time namespace and temporal properties
- direct unit coverage for OpenAPI temporal schema mapping
- direct unit coverage for SQL temporal type mapping
- direct unit coverage for BSON temporal transport
- direct unit coverage for relational row retyping into native temporal schema fields
- end-to-end coverage in `examples/functional_tests`, including:
  - core/runtime
  - contracts
  - cache
  - doc
  - db
  - HTTP client
  - iteration
  - parallel
  - queue
  - auth
