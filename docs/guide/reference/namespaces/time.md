---
title: "time"
category: namespaces
slug: "reference/namespaces/time"
summary: "Construct and work with instants, dates, times, durations, and intervals."
---

# time

The `time` namespace constructs and manipulates the temporal types (`instant`,
`date`, `time`, `duration`, `interval`). Those types are described in
[Types](../types.md). This namespace is how you create and combine them.

## When to use

Use `time` to get the current moment, parse a temporal string, build a duration, or
compare intervals.

## Operations

```ruby
now = time.now()
deadline = time.now()
window = time.interval(time.date("2026-01-01"), time.date("2026-01-31"))
```

| Name | Signature | Summary |
|---|---|---|
| `time.now` | `time.now()` | Returns the current instant. |
| `time.today` | `time.today()` | Returns the current local date. |
| `time.instant` | `time.instant(text)` | Parses an ISO instant. |
| `time.date` | `time.date(text)` | Parses `YYYY-MM-DD` into a date. |
| `time.at` | `time.at(text)` | Parses `HH:MM:SS` into a time. |
| `time.parse` | `time.parse(text)` | Parses a temporal string. |
| `time.from_unix` / `time.unix` | `time.from_unix(seconds)` / `time.unix(instant)` | Converts between epoch seconds and an instant. |
| `time.seconds` / `minutes` / `hours` / `days` | `time.minutes(value)` | Creates a duration. |
| `time.interval` | `time.interval(start, end)` | Creates an interval between two instants. |
| `time.contains` | `time.contains(interval, value)` | Returns whether an interval contains a value. |
| `time.overlaps` | `time.overlaps(left, right)` | Returns whether two intervals overlap. |
| `time.format` | `time.format(value, mask)` | Formats a temporal value as text. |

## Notes

- The timezone for `time.today` and other local operations follows `MARRETA_TIMEZONE`.
  See [Configuration](../configuration.md).
