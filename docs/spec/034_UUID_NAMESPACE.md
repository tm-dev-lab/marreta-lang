# 034 — UUID Namespace

> Status: Delivered
> Type: Native namespace
> Scope: Small native UUID helper surface for backend/API use cases

Delivery notes:

- `cebfd89` — refined the UUID namespace spec around RFC 9562 vocabulary,
  canonical string output, and intentionally small first-cut scope.
- `9de3f0b` — delivered `uuid.v4()` and `uuid.v7()` as native namespace
  functions with unit and functional coverage.

---

## 1. Purpose

This spec introduces a small native `uuid` namespace for MarretaLang.

The purpose is not to turn the language into a general identifier toolkit with
many UUID variants and parsing policies.

The purpose is narrower:

- generate UUID values explicitly in application code
- support common backend/API workflows that need stable public identifiers
- avoid every project reinventing UUID generation outside the language

Examples of legitimate use cases:

- public resource IDs
- idempotency keys
- external correlation tokens created by user code
- queue/event identifiers
- filesystem or cache keys that need globally unique names

---

## 2. Why `uuid` Matters

UUID generation is one of the most common tiny primitives in backend work.

It appears often enough that forcing each project to delegate it externally
creates friction, but it is also small enough that the language should expose a
very constrained surface.

Like `base64`, the value of a native `uuid` namespace is explicitness:

- the developer can generate a UUID in the exact place it becomes part of
  application data
- the runtime can guarantee a standard textual representation

---

## 3. Design Principles

The `uuid` namespace must follow these rules:

1. It should stay extremely small.
2. It should be string-first in the first cut.
3. It should favor explicit generation over implicit auto-ID behavior.
4. It should not introduce a custom UUID runtime type in the first cut.
5. It should work naturally in normal calls and pipelines.

## 3.1 Why versions, not intent names

MarretaLang should expose UUID variants by their RFC 9562 version names rather
than by intent aliases such as `random()` or `ordered()`.

Rationale:

- UUID is an external standard whose public vocabulary is its version numbers
- operators and developers already read `v4` and `v7` in surrounding
  ecosystems such as Postgres, application libraries, logs, and observability
  tooling
- preserving the standard vocabulary avoids hidden semantic mappings between a
  language-local alias and the actual variant generated

This follows a broader naming rule that is useful beyond UUID:

- when MarretaLang wraps a public external standard, it should preserve the
  standard's vocabulary
- when MarretaLang exposes language-defined operations, it may name by intent

Examples:

- standard vocabulary preserved:
  - `uuid.v4()`
  - `uuid.v7()`
  - HTTP verbs such as `GET`, `POST`, `PATCH`
- language-defined operations named by intent:
  - `time.now()`
  - `json.pretty()`
  - `fs.read()`
  - `log.info()`

---

## 4. Proposed Surface

The initial surface should be:

```marreta
uuid.v4()
uuid.v7()
```

This is the proposed first-cut surface.

---

## 5. Function Purpose

## 5.1 `uuid.v4()`

Purpose:

- generate a RFC 4122 / RFC 9562 version 4 UUID as canonical lowercase text

Example:

```marreta
public_id = uuid.v4()
```

Recommended use:

- opaque public IDs
- externally visible identifiers where temporal ordering is not needed
- identifiers where revealing creation time is undesirable

## 5.2 `uuid.v7()`

Purpose:

- generate a RFC 9562 version 7 UUID as canonical lowercase text

Example:

```marreta
record_id = uuid.v7()
```

Recommended use:

- persistent IDs
- queue/event identifiers
- time-ordered identifiers for storage and pagination
- anything compared or ordered by creation time

---

## 6. Semantics

## 6.1 Return type

The first cut should return:

- `string`

The returned value should use canonical hyphenated lowercase form, for example:

```text
550e8400-e29b-41d4-a716-446655440000
```

This canonical representation is part of the contract, not an implementation
detail.

The runtime must always emit:

- lowercase text
- hyphenated UUID form
- 36 characters total

The runtime must also guarantee RFC-compliant layout:

- 32 hexadecimal digits
- `8-4-4-4-12` grouping
- correct version nibble for the selected UUID version
- correct variant bits

This output format is part of the public contract and must remain stable across
runtime minor versions.

## 6.2 No custom UUID runtime type

The first cut should not introduce a distinct UUID runtime type.

The runtime contract is intentionally simple:

- generate UUID
- return canonical string

This keeps the feature easy to transport through:

- JSON
- filesystem
- cache
- queue
- HTTP payloads

## 6.3 Randomness source

The runtime should use a cryptographically secure random source through the
Rust runtime and dependencies.

In practice, the contract should assume OS-backed cryptographically secure
randomness.

The language must not expose any seed or deterministic UUID generation knobs in
the first cut.

## 6.4 Pipeline behavior

The namespace should work naturally in expression contexts and pipelines.

Example:

```marreta
id = uuid.v4()
```

```marreta
ordered_id = uuid.v7()
```

```marreta
payload = {
    id: uuid.v4(),
    name: "widget"
}
```

The first cut does not need a tap-like pipeline mode because `uuid.v4()` and
`uuid.v7()` are zero-argument generators, not pass-through transforms.

---

## 7. Error Semantics

In the normal case, `uuid.v4()` and `uuid.v7()` should not surface user-facing
validation errors.

If generation fails because the runtime cannot obtain secure randomness, that
should surface as a runtime error.

The first cut does not need alternate variants or fallback generation policies.

---

## 8. Non-Goals

The first cut does not include:

- `uuid.parse(...)`
- `uuid.validate(...)`
- `uuid.nil()`
- `uuid.now()`
- timestamp extraction helpers from UUID text
- `uuid.v1()`
- `uuid.v3()`
- `uuid.v5()`
- `uuid.v6()`
- UUID binary/byte representations
- a custom UUID schema/runtime type
- implicit auto-generation in schema IDs

If any of those become necessary later, they should be justified separately
instead of inflating the first cut.

---

## 9. Implementation Plan

### Phase 1 — Namespace

- reserve `uuid`
- expose `uuid.v4()` and `uuid.v7()` in the interpreter

### Phase 2 — Runtime generation

- use a stable Rust UUID dependency
- return canonical lowercase hyphenated text
- support both random (`v4`) and time-ordered (`v7`) generation

### Phase 3 — Validation

- add unit coverage for shape/pattern expectations
- add functional coverage in realistic API flows

### Phase 4 — Examples and docs

- add examples with:
  - HTTP payloads
  - queue messages
  - cache keys
- review `docs/vscode-marreta` if the reserved namespace needs syntax updates

---

## 10. Test Plan

### Phase 1 tests

- `uuid` resolves as native namespace

### Phase 2 tests

- `uuid.v4()` returns `string`
- `uuid.v7()` returns `string`
- output matches canonical UUID v4 pattern
- output matches canonical UUID v7 pattern
- output is lowercase
- repeated `v4` calls generate distinct values
- repeated `v7` calls generate distinct values

### Phase 3 tests

- functional test in route payload
- functional test in queue message/event ID integration
- functional test in cache key interpolation
- functional test proving `uuid.v7()` values sort lexicographically in creation
  order for persistent/database use
- scenario coverage if the example value is returned through HTTP

### Phase 4 tests

- review `docs/vscode-marreta` for reserved namespace highlighting/version bump

---

## 11. Recommendation

This spec should remain intentionally tiny.

The best first cut is:

- `uuid.v4()`
- `uuid.v7()`

and nothing else.

That gives MarretaLang a highly common API/backend primitive without starting a
new branch of runtime complexity.
