---
title: "Temporal types"
category: types
slug: "reference/types/temporal"
summary: "Instants, dates, times, durations, and intervals: how to construct them and read their parts."
---

# Temporal types

`instant`, `date`, `time`, `duration`, and `interval` represent points and spans of
time. You construct them through the [`time`](../namespaces/time.md) namespace and
read their parts with property access.

```ruby
created = time.now()
year = created.year
hour = created.hour

window = time.interval(time.date("2026-01-01"), time.date("2026-01-31"))
days = window.duration.total_days
```

## The types

| Type | What it is | Construct with |
|---|---|---|
| `instant` | A point in time. | `time.now()`, `time.instant("...")` |
| `date` | A calendar date. | `time.today()`, `time.date("YYYY-MM-DD")` |
| `time` | A wall-clock time. | `time.at("HH:MM:SS")` |
| `duration` | A length of time. | `time.seconds(n)`, `time.minutes(n)`, `time.hours(n)`, `time.days(n)` |
| `interval` | A span between two instants. | `time.interval(start, end)` |

## Properties

Read the parts of a temporal value with a property:

| On | Properties |
|---|---|
| `instant` | `year`, `month`, `day`, `hour`, `minute`, `second`, `weekday`, `unix`, `date`, `time` |
| `date` | `year`, `month`, `day` |
| `time` | `hour`, `minute`, `second` |
| `duration` | `total_days`, `total_hours`, `total_minutes`, `total_seconds` |
| `interval` | `duration` |

The `total_*` properties return a `float`.

## Placing a time on a date

A `time` has an `on(date)` method that puts it on a date, producing an instant:

```ruby
opening = time.at("09:30:00").on(time.date("2026-01-15"))
```

## Notes

- Local-time values follow `MARRETA_TIMEZONE`. See
  [Configuration](../configuration.md).
- For comparing intervals (`contains`, `overlaps`) and formatting, see the
  [`time`](../namespaces/time.md) namespace.
