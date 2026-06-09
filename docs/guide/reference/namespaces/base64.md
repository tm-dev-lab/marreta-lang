---
title: "base64"
category: namespaces
slug: "reference/namespaces/base64"
summary: "Encode text to base64 and decode it back, with an optional URL-safe alphabet."
---

# base64

The `base64` namespace encodes text to base64 and decodes it back. Pass
`url_safe: true` for the URL and filename safe alphabet, used in tokens and query
values.

## Operations

```ruby
encoded = base64.encode(text)
back = encoded >> base64.decode() rescue null
```

| Name | Signature | Summary |
|---|---|---|
| `base64.encode` | `base64.encode(text, url_safe: true)` | Encodes text as base64. |
| `base64.decode` | `base64.decode(text, url_safe: true)` | Decodes base64 text. |

## Notes

- `base64.decode` fails on input that is not valid base64. Wrap it in `rescue` when
  the input is untrusted.
