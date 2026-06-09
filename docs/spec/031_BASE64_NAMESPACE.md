# 031 — Base64 Namespace

> Status: Delivered
> Type: Native namespace
> Scope: Small native Base64 helper surface for backend/API use cases

---

## 1. Purpose

This spec introduces a small native `base64` namespace for MarretaLang.

The purpose of `base64` is not to turn the language into a binary-processing
toolkit.

The purpose is narrower:

- encode textual data into Base64 for transport
- decode Base64 text back into native string data
- support common backend/API workflows where Base64 appears as an interchange
  format

Examples of legitimate use cases:

- HTTP headers or tokens that carry Base64 payloads
- queue/doc/cache snapshots that need Base64 wrapping
- webhooks or third-party integrations that deliver Base64 strings
- filesystem workflows where an encoded text blob must be stored or restored

---

## 2. Why `base64` Matters

Base64 is a very common wire format in API/backend work.

It appears often enough that forcing every project to reimplement it outside the
language creates friction, but it is still small enough to deserve a focused
surface instead of a broad encoding framework.

Like `json`, the value of a native `base64` namespace is explicitness:

- the developer can clearly encode before transport
- the developer can clearly decode before using the content

---

## 3. Design Principles

The `base64` namespace must follow these rules:

1. It should stay very small.
2. It should remain text-first in the first cut.
3. It should be explicit about invalid input.
4. It should not introduce a binary/blob type in the first cut.
5. It should work naturally in normal calls and pipelines.

---

## 4. Proposed Surface

The delivered surface is:

```marreta
base64.encode(text)
base64.decode(text)
base64.encode(text, url_safe: true)
base64.decode(text, url_safe: true)
```

This is the complete first-cut surface.

---

## 5. Function Purposes

## 5.1 `base64.encode(text)`

Purpose:

- encode plain text into a Base64 string

Example:

```marreta
token = base64.encode("client:secret")
```

## 5.2 `base64.decode(text)`

Purpose:

- decode a Base64 string back into plain text

Example:

```marreta
credentials = base64.decode(auth_token)
```

---

## 6. Semantics

## 6.1 Input types

- `base64.encode(text)` accepts `string`
- `base64.decode(text)` accepts `string`

## 6.2 Return types

- `base64.encode(text) -> string`
- `base64.decode(text) -> string`

## 6.3 Text-first scope

The first cut is intentionally text-first.

That means:

- the namespace only guarantees string input/output
- it does not introduce a binary/blob value type
- it does not expose byte-array APIs

This keeps the surface aligned with the rest of the language and with the most
common API/backend use cases.

## 6.4 Encoding flavor

The first cut supports two explicit modes:

- standard Base64 (default)
- URL-safe Base64 via `url_safe: true`

Examples:

```marreta
token = base64.encode("client:secret")
cursor = base64.encode("???", url_safe: true)
```

The default is standard Base64.

The namespace must not silently switch between standard and URL-safe alphabets.

## 6.5 Decode permissiveness

`base64.decode(...)` is permissive about missing padding.

This means:

- padded standard input is accepted
- unpadded standard input is accepted
- padded URL-safe input is accepted
- unpadded URL-safe input is accepted

Permissiveness applies only to padding.

It does **not** mean the decoder may silently mix alphabets:

- standard decode must reject URL-safe alphabet input
- URL-safe decode must reject standard alphabet input

## 6.6 UTF-8 output contract

The first cut remains string-only.

That means `base64.decode(...)` must return a valid `string`.

If the decoded bytes cannot be represented as valid UTF-8 text, decoding fails
explicitly.

## 6.7 Pipeline behavior

The namespace should work naturally in pipelines:

```marreta
"client:secret" >> base64.encode()
token >> base64.decode()
```

---

## 7. Error Behavior

The namespace should fail clearly for invalid input.

Examples:

```marreta
base64.encode(42)
base64.decode(42)
base64.decode("%%%")
```

Expected outcomes:

- invalid input type -> type error
- malformed Base64 text -> runtime error with clear context
- decoded non-UTF-8 bytes -> runtime error with clear context

The namespace must not silently coerce non-string values or malformed encoded
data.

## 7.1 Error table

| Operation | Invalid case | Expected failure |
|---|---|---|
| `base64.encode(text)` | input is not `string` | `type_error` |
| `base64.decode(text)` | input is not `string` | `type_error` |
| `base64.decode(text)` | string is malformed Base64 | `runtime_error` |
| `base64.decode(text)` | decoded bytes are not valid UTF-8 | `runtime_error` |

---

## 8. What Does Not Belong

The following should stay out of the initial `base64` surface:

- raw byte APIs
- alternate encodings
- streaming encoders/decoders
- file-specific helpers
- auto-detection of Base64 payloads

These would expand the namespace too far for the first cut.

---

## 9. Examples

## 9.1 Encode credentials for transport

```marreta
token = "client:secret" >> base64.encode()
```

## 9.2 Decode a header-derived token

```marreta
raw = headers["x-debug-token"]
decoded = raw >> base64.decode()
```

## 9.3 Build an Authorization header

```marreta
token = "client:secret" >> base64.encode()
authorization = "Basic #{token}"
```

## 9.4 Store encoded text in cache

```marreta
encoded = base64.encode(payload)
cache.set("payload:b64", encoded)
```

## 9.5 Combine with filesystem

```marreta
content = fs.read("debug.txt")
encoded = content >> base64.encode()
```

## 10. Delivery Notes

- Delivered on branch `feature/base64-031`
- Runtime implementation:
  - reserved namespace token
  - native namespace value
  - direct dispatch in standard calls and pipelines
  - support for `url_safe: true`
  - permissive decode without padding
- Functional coverage:
  - headers
  - filesystem
  - cache
  - queue
  - HTTP client
  - scenario tests with request headers and request body
- Validation:
  - `cargo test --lib`
  - `bash examples/functional_tests/test.sh`

## 11. VS Code Bundle Review

This feature introduces a new reserved namespace: `base64`.

Delivery must review `docs/vscode-marreta` for:

- syntax highlighting of the reserved namespace
- package version bump
- regenerated `.vsix` bundle if the extension is versioned in-repo
