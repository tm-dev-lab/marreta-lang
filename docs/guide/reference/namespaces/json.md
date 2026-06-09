---
title: "json"
category: namespaces
slug: "reference/namespaces/json"
summary: "Parse JSON text into Marreta values and serialize values back to JSON."
---

# json

The `json` namespace converts between JSON text and Marreta values. Routes already
parse and serialize JSON bodies for you, so reach for `json` when you handle JSON as
raw text, such as a string field or a third-party payload.

## When to use

Use `json.parse` to turn untrusted JSON text into a value you can read, and
`json.stringify` to produce JSON text to store or send. For a body that arrives on a
route, the request and `reply` already handle JSON, so you rarely need these there.

## Operations

`parse` is fallible, so guard it with `rescue` when the text is untrusted:

```ruby
data = raw >> json.parse() rescue { error: "invalid json" }
```

| Name | Signature | Summary |
|---|---|---|
| `json.parse` | `json.parse(text)` | Parses JSON text into a value. |
| `json.stringify` | `json.stringify(value)` | Serializes a value to compact JSON. |
| `json.pretty` | `json.pretty(value)` | Serializes a value to indented JSON. |

## Notes

- `json.parse` fails on malformed text. Wrap it in `rescue` so one bad input does not
  become a 500. See [Handle errors](../../how-to/handle-errors.md).
