# 030 — JSON Namespace

> Status: Delivered
> Type: Implementation candidate
> Scope: Small native JSON helper surface for backend/API use cases

---

## 1. Purpose

This spec introduces a small native `json` namespace for MarretaLang.

The purpose of `json` is not to create a second object model inside the
language.

The purpose is narrower:

- parse explicit JSON text into native Marreta values
- serialize native Marreta values into explicit JSON text
- support common API/backend workflows that need textual JSON at runtime

Examples of legitimate use cases:

- persist a payload snapshot into a file
- store a canonical JSON string in cache
- emit a textual audit body to queue or doc storage
- parse raw JSON received through a text-based integration
- pretty-print JSON for debugging or local export

---

## 2. Why `json` Matters

MarretaLang already uses JSON heavily in HTTP transport, but most of that is
implicit at the route boundary.

What is still missing is a small explicit runtime surface for the moments when
the developer wants to control JSON directly in code.

Without a native JSON namespace, these workflows become awkward:

- `fs.write(...)` needs explicit text
- queue/doc/cache snapshots may need canonical textual output
- raw text integrations need explicit parsing

`json` fills that gap without adding a second data model, because parsed JSON
should become native language values immediately.

---

## 3. Design Principles

The `json` namespace must follow these rules:

1. It should stay small.
2. It should map directly to native Marreta values.
3. It should avoid query/path mini-languages.
4. It should prefer explicit parse/serialize operations over magic coercion.
5. It should be predictable across HTTP, cache, queue, doc, and filesystem use.

---

## 4. Proposed Surface

The initial surface should be:

```marreta
json.parse(text)
json.stringify(value)
json.pretty(value)
```

This is the proposed first-cut surface.

---

## 5. Function Purposes

## 5.1 `json.parse(text)`

Purpose:

- turn explicit JSON text into native language values

Example:

```marreta
payload = json.parse(raw_body)
customer = payload.customer.name
```

## 5.2 `json.stringify(value)`

Purpose:

- serialize a native Marreta value into canonical JSON text

Example:

```marreta
snapshot_text = json.stringify(snapshot)
```

## 5.3 `json.pretty(value)`

Purpose:

- serialize a native Marreta value into indented JSON text for debugging,
  exports, or local inspection

Example:

```marreta
report = json.pretty(payload)
```

---

## 6. Semantics

## 6.1 Input types

- `json.parse(text)` accepts `string`
- `json.stringify(value)` accepts any native value that can be represented in
  JSON
- `json.pretty(value)` accepts the same input domain as `json.stringify`

## 6.2 Return types

- `json.parse(text) -> native value`
- `json.stringify(value) -> string`
- `json.pretty(value) -> string`

Parsed JSON must become native runtime values immediately:

- JSON object -> `map`
- JSON array -> `list`
- JSON string -> `string`
- JSON integer -> `integer`
- JSON decimal -> `float`
- JSON boolean -> `boolean`
- JSON null -> `null`

`json.parse(...)` must accept any valid JSON value at the root.

That includes:

- object
- array
- string
- number
- boolean
- null

That means the language should use normal property and index access after
parsing:

```marreta
data = json.parse(raw)
name = data.customer.name
first = data.items[0]
```

No JSON-specific traversal API is required for this first cut.

## 6.3 Text semantics

`json.stringify` should produce compact canonical JSON text.

That means:

- no trailing comments
- no extra whitespace
- deterministic JSON syntax
- stable key ordering

For map/object serialization, the first cut should preserve insertion order.

That means:

- if a native map was built in a given key order, `json.stringify(...)` and
  `json.pretty(...)` should preserve that same order
- parsed JSON objects should preserve the key order they appeared with in the
  original JSON text
- the runtime must not reorder keys lexicographically in the default JSON
  serialization path

`json.pretty` should produce human-readable JSON text with stable indentation.

`json.pretty(...)` and `json.stringify(...)` must share the same value
semantics.

That means:

- the same native input value must produce the same JSON structure in both
  functions
- temporal values must use the same canonical representation in both
  functions
- the only intended difference is whitespace/indentation

The exact indentation width may be implementation-defined in the first cut, but
it must be consistent.

## 6.4 Value boundaries

The first cut should only guarantee JSON support for native values that map
cleanly into JSON.

This includes:

- `null`
- `boolean`
- `integer`
- `float`
- `string`
- `list`
- `map`

The first cut must explicitly reject values that do not have a stable JSON
representation policy yet.

That includes, for now:

- task values
- namespace/runtime service values
- query builders

Temporal values should be supported in the first cut using the same canonical
textual forms already exposed by the language at HTTP boundaries.

That means:

- `instant` -> ISO 8601 UTC string
- `date` -> canonical date string
- `time` -> canonical time-of-day string
- `duration` -> canonical duration string
- `interval` -> canonical interval object using canonical temporal component
  forms

`json.stringify(...)` must not diverge from the language's existing canonical
transport rules for these values.

---

## 7. Error Behavior

The namespace should fail clearly for invalid input or unsupported values.

Examples:

```marreta
json.parse(42)
json.parse("{bad json}")
json.stringify(task_ref)
```

Expected outcomes:

- invalid parse input type -> type error
- invalid JSON text -> runtime error with clear parse context
- unsupported runtime value for serialization -> type error

The namespace must not silently coerce malformed JSON or unsupported values.

## 7.1 Error table

| Operation | Invalid case | Expected failure |
|---|---|---|
| `json.parse(text)` | input is not `string` | `type_error` |
| `json.parse(text)` | string is malformed JSON | `runtime_error` |
| `json.stringify(value)` | value has no supported JSON representation | `type_error` |
| `json.pretty(value)` | value has no supported JSON representation | `type_error` |

---

## 8. What Does Not Belong

The following should stay out of the initial `json` surface:

- `json.get(path)`
- `json.set(path, value)`
- JSONPath
- patch/merge patch helpers
- schema validation inside `json.*`
- implicit file or HTTP parsing
- alternate encodings

These would either duplicate the language’s own access semantics or expand the
surface too far.

---

## 9. Examples

## 9.1 Write a JSON snapshot to disk

```marreta
payload
    >> json.stringify()
    >> fs.write("out/request.json")
```

## 9.2 Pretty-print a local export

```marreta
content = json.pretty(report)
fs.write("out/report.json", content)
```

## 9.3 Parse a raw JSON integration body

```marreta
route POST "/ingest" take raw
    data = json.parse(raw)
    reply 200, {
        customer: data.customer.name,
        items: data.items.length()
    }
```

## 9.4 Cache a canonical JSON payload

```marreta
snapshot = json.stringify(order)
cache.set("orders:last", snapshot)
```

## 9.5 Read native values after parsing

```marreta
data = json.parse("{\"customer\":{\"name\":\"Ana\"},\"items\":[1,2]}")

customer_name = data.customer.name
first_item = data.items[0]
```

---

## 10. Delivery Notes

Delivered in:

- `feature/json-030`

Delivered behavior:

- native `json.parse(...)`
- native `json.stringify(...)`
- native `json.pretty(...)`
- insertion-order-preserving JSON parse/serialize through native runtime maps
- direct `json.*` pipeline support
- functional integration with `fs`, `cache`, `queue`, `http_client`, and raw
  request bodies
- expanded native `.marreta` scenario coverage for request body, headers, query
  params, path params, and non-GET verbs

---

## 11. Recommendation

Current recommendation:

1. keep `json` small
2. parse directly into native values
3. rely on normal language access for reading parsed data
4. avoid JSONPath or traversal mini-languages
5. keep serialization explicit
6. keep malformed JSON as an explicit runtime failure
7. include `json.pretty(...)` in the first cut
8. serialize temporal values using the language's existing canonical forms
9. accept any valid JSON root value during parse

The best first implementation target is:

- `json.parse`
- `json.stringify`
- `json.pretty`

---

## 12. Implementation Plan

Suggested implementation order:

### Phase 1

- reserve the `json` namespace
- finalize pretty-print stability expectations

### Phase 2

- add parser/runtime namespace support
- add unit tests for parse/stringify/pretty behavior and failure cases

### Phase 3

- add functional coverage in `examples/functional_tests`
- validate interaction with:
  - `fs`
  - `cache`
  - `queue`
  - `http_client`
  - route raw-body workflows
- add `.marreta` scenario coverage for:
  - body
  - headers
  - query params
  - path params
  - non-GET verbs
- review `docs/vscode-marreta` for namespace highlighting, reserved-word
  treatment, and any syntax updates needed by the delivered `json` surface

---

## 13. Test Plan

Implementation of this spec must include:

1. unit tests for `json.parse`
2. unit tests for `json.stringify`
3. unit tests for `json.pretty`
4. unit tests for unsupported-value failures
5. functional tests in `examples/functional_tests`
6. integration checks with `fs`, `cache`, `queue`, and raw request handling
7. `.marreta` scenario tests covering request body, headers, query params, path
   params, and non-GET verbs
8. a delivery-time review of `docs/vscode-marreta` to determine whether
   highlighting or indentation updates are needed for the `json` namespace
