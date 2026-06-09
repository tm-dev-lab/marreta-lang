# 028 — Math Namespace

> Status: Delivered
> Type: Delivered feature
> Scope: Small native numeric helper surface for backend/API use cases

---

## 1. Purpose

This spec introduces a small native `math` namespace for MarretaLang.

The purpose of `math` is not to turn the language into a numeric or scientific
computing platform. The purpose is to cover the recurring numeric helpers that
show up in API and backend code and that are currently awkward when rebuilt
manually in tasks and routes.

This spec is intentionally narrow:

- it complements existing arithmetic
- it avoids broad standard-library expansion
- it keeps the surface obvious and small

---

## 2. Why `math` Fits MarretaLang

`math` fits the language better than many other utility areas because:

1. API and backend code often needs small numeric helpers.
2. The feature can stay compact and predictable.
3. It does not blur the identity of MarretaLang as an API language.
4. It reinforces readability instead of adding ceremony.

Typical real-world use cases:

- clamp a score between valid limits
- round or ceil a billing amount
- force pagination/page-count rounding
- choose min/max thresholds
- compute an absolute delta

---

## 3. Design Principles

The `math` namespace must follow these rules:

1. It should expose only functions that are common in API/backend work.
2. It should avoid overlapping ways to do the same thing.
3. It should not replace native arithmetic operators.
4. It should not become a scientific computing library.
5. It should work naturally with existing `integer` and `float` runtime values.

---

## 4. Delivered Surface

The initial surface should be:

```marreta
math.abs(x)
math.floor(x)
math.ceil(x)
math.round(x)
math.round(x, places: 2)
math.min(a, b)
math.max(a, b)
math.clamp(x, min: 0, max: 100)
```

This is the delivered first-cut surface.

---

## 5. Function Purposes

## 5.1 `math.abs(x)`

Purpose:

- express positive distance or magnitude without manual sign handling

Example:

```marreta
delta = math.abs(paid_amount - expected_amount)
```

## 5.2 `math.floor(x)`

Purpose:

- force rounding down for pagination, counts, and billing rules

Example:

```marreta
pages = math.floor(total / page_size)
```

## 5.3 `math.ceil(x)`

Purpose:

- force rounding up when partial units must count as full units

Example:

```marreta
pages = math.ceil(total / page_size)
```

## 5.4 `math.round(x)`

Purpose:

- normalize a numeric result when raw float precision is not appropriate for
  output or further business use

Example:

```marreta
final_total = math.round(base_total * 1.17)
```

## 5.5 `math.min(a, b)`

Purpose:

- choose the lower of two values without a full conditional

Example:

```marreta
discount = math.min(requested_discount, 30)
```

## 5.6 `math.max(a, b)`

Purpose:

- choose the higher of two values without a full conditional

Example:

```marreta
score = math.max(score, 0)
```

## 5.7 `math.clamp(x, min: a, max: b)`

Purpose:

- keep values inside a valid range in one expression

Example:

```marreta
score = math.clamp(raw_score, min: 0, max: 100)
```

Both `min:` and `max:` are mandatory.

For a lower-bound-only rule, `math.max(...)` should be used.

For an upper-bound-only rule, `math.min(...)` should be used.

---

## 6. Semantics

## 6.1 Input types

`math` functions should accept:

- `integer`
- `float`

They should reject:

- `string`
- `boolean`
- `list`
- `map`
- `null`
- temporal types

with a clear runtime type error.

## 6.2 Return types

Return types should be predictable:

- `math.abs(integer)` -> `integer`
- `math.abs(float)` -> `float`
- `math.floor(...)` -> `integer`
- `math.ceil(...)` -> `integer`
- `math.round(integer)` -> `integer`
- `math.round(float)` -> `integer` in the one-argument form
- `math.round(x, places: n)` -> `float`, including `places: 0`
- `math.min(a, b)` -> same numeric family if both inputs are same type; `float`
  if mixed
- `math.max(a, b)` -> same numeric family if both inputs are same type; `float`
  if mixed
- `math.clamp(...)` -> same promotion rule as `min/max`

These rules are the delivered runtime contract for the first implementation.

## 6.3 Mixed numeric inputs

Mixed `integer` + `float` input should be allowed.

Example:

```marreta
math.max(10, 10.5)
```

Expected behavior:

- numeric coercion stays explicit in runtime semantics
- result becomes `float`

Example:

```marreta
math.round(5, places: 2)
```

Expected behavior:

- valid call
- result is `5.0`

---

## 7. Error Behavior

The namespace should fail clearly for invalid input.

Examples:

```marreta
math.abs("10")
math.sqrt("9")
math.min([1], 2)
math.clamp(null, min: 0, max: 10)
```

Expected outcome:

- runtime type error
- no implicit parsing or coercion from strings

`math.round(x, places: n)` must also define invalid-argument behavior.

`math.round(x, places: n)` controls numeric rounding semantics, not display
formatting semantics.

This means trailing zeroes are not part of the runtime numeric value model.
If a caller needs `"5.00"` instead of the numeric value `5.0`, that is a
formatting concern and should be handled by formatting APIs rather than `math`.

Recommended direction:

- reject `places < 0` with a runtime error
- reject non-integer `places` with a runtime type error
- require both `min:` and `max:` in `math.clamp(...)`
- reject `math.clamp(x, min: a, max: b)` when `a > b` with a runtime error

---

## 8. What Does Not Belong

The following should stay out of the initial `math` surface:

- trigonometry
- random number generation
- advanced statistics
- combinatorics
- matrix/vector helpers
- scientific constants library
- `pow`
- `sqrt`

Those would increase surface area without matching the main purpose of the
language.

---

## 9. Examples

## 9.1 API rule examples

```marreta
route POST "/scores/normalize" take payload
    normalized = math.clamp(payload.score, min: 0, max: 100)
    reply 200, { score: normalized }
```

```marreta
route GET "/pages"
    total = 95
    page_size = 20
    pages = math.ceil(total / page_size)
    reply 200, { pages: pages }
```

```marreta
task bounded_discount(requested)
    math.max(0, math.min(requested, 30))
```

## 9.2 Precision use in backend code

```marreta
route POST "/totals/round" take payload
    reply 200, {
        rounded: math.round(payload.total),
        rounded_cents: math.round(payload.total, places: 2),
        lower: math.floor(payload.total),
        upper: math.ceil(payload.total)
    }
```

---

## 10. Delivery Notes

Delivered on branch `feature/math-028`.

Relevant commits:

- `4d3b1e2` — refined the math namespace spec and closed the first-cut contract
- `fb5d4d2` — implemented the math namespace in the runtime and functional suite

Delivered behavior:

- `math.*` works in direct calls, pipelines, and parallel broadcast
- no task wrappers are required to use `math.*` inside `>>` or `*>>`
- unit coverage exists for all delivered functions and error paths
- functional coverage exists in `examples/functional_tests` across:
  - core routes
  - contracts
  - cache
  - HTTP client
  - iteration/reduce
  - parallel broadcast
  - queue
  - auth

Validation performed during delivery:

1. `cargo test --lib interpreter::tests::test_math_ -- --nocapture`
2. `cargo test --lib`
3. `bash examples/functional_tests/test.sh`
