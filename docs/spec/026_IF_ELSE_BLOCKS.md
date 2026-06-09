# 026 If/Else Blocks

> Status: Delivered
> Type: Language Feature

## Goal

Add block-style `if/else` for general branching, without forcing every multi-line conditional flow into `match`, `require/reject`, or one-line conditional suffixes.

## Motivation

Today the language covers these cases well:

- validation gates with `require` / `reject`
- one-line conditional assignment
- value-based branching with `match`

What is still missing is a natural construct for general multi-line imperative conditional logic.

Desired shape:

```marreta
if cached
    reply 200, cached
else
    order = load_order_details(params.id)
    reply 200, order
```

The `omni_hub` example showed that some specific cases are still acceptable with:

```marreta
cached = cache.get(key)
details = cached or load_order_details(params.id)
```

But that does not solve general branching in a readable way.

## Non-Goals

- replacing `require` / `reject` as the idiomatic route-validation form
- replacing `match` for branching over multiple discrete values
- introducing a ternary operator

## Syntax

### Simple

```marreta
if cond
    expr_a
else
    expr_b
```

### Chained

```marreta
if score > 90
    "excellent"
else if score > 70
    "good"
else
    "regular"
```

## Semantics

1. `if/else` is an expression.
2. The value of the construct is the value of the last evaluated expression in the chosen branch.
3. `else` is optional.
4. Without `else`, if the condition is false, the result of the `if` expression is `null`.
5. `fail` inside any branch remains `Never` and aborts the route immediately.
6. `reply` inside any branch also terminates the route immediately, following the same early-return semantics already defined by the core language.
7. `else if` is syntactic sugar for `else` followed by `if`.
8. Condition evaluation follows the language's normal truthiness rules. At minimum, `false` and `null` select the `else` branch. Empty values must follow the same truthiness contract already used by `require/reject`.

## Scope and Resolution

Variables assigned inside an `if` branch are block-scoped to that branch.

Example:

```marreta
if score > 90
    bonus = 10
else
    bonus = 0

print(bonus) # invalid
```

The idiomatic way to carry a value out of the conditional is to assign the `if` expression itself:

```marreta
bonus = if score > 90
    10
else
    0
```

This keeps `if/else` aligned with the expression-oriented style of the language and avoids implicit variable leakage across branches.

## Examples

```marreta
status = if balance > 0
    "positive"
else
    "empty"
```

```marreta
result = if cached
    cached
else
    fresh = db.orders.find(params.id)
    fresh
```

```marreta
if not payload.items
    fail 400, "items required"
else
    reply 200, payload.items
```

## Parser / Runtime Constraints

1. `if` uses indentation like every other block construct.
2. `else` must align with the opening `if`.
3. `else if` must align with the opening `if`.
4. Nested `if` blocks must remain indentation-driven only; no braces.
5. In Phase 1, the condition must fit on a single logical line. Multi-line `if` conditions are out of scope until the language defines a general continuation rule for multi-line expressions.
6. A pipeline aligned after the full `if` expression applies to the result of the whole `if`, not only to the last branch line.

Example:

```marreta
result = if cached
    cached
else
    db.orders.find(params.id)
>> transform_data()
```

The example above is equivalent to:

```marreta
tmp = if cached
    cached
else
    db.orders.find(params.id)

result = tmp >> transform_data()
```

If the implementation cannot support this safely in Phase 1, the parser/runtime must reject it clearly rather than parse it ambiguously.

## Interaction With Existing Constructs

- `require/reject` remain preferred for validation gates.
- `match` remains preferred when branching on one subject with multiple arms.
- the conditional suffix remains preferred for simple single-line assignment.

## Implementation Plan

### Phase 1

- extend the parser AST with `IfExpression`
- support `if`, `else`, and `else if`

### Phase 2

- implement interpreter/runtime evaluation
- guarantee `null` when the condition is false and there is no `else`

### Phase 3

- add parser tests
- add interpreter tests
- add end-to-end route examples

## Delivery Notes

- Phase 1 delivered in `cbaf79e`:
  - AST support for `If`
  - parser support for `if`, `else`, `else if`
  - pipeline precedence after full `if` expressions
- Phase 2 delivered in `cbaf79e` and corrected in `55a1ee2`:
  - runtime evaluation for `if/else`
  - `null` when `else` is omitted
  - `reply` / `fail` early-return propagation
  - final branch-scope isolation for both new bindings and reassignment of outer bindings
- Phase 3 delivered in `cbaf79e` and `55a1ee2`:
  - parser tests
  - interpreter tests
  - end-to-end functional coverage in `examples/functional_tests`

## Test Plan

1. `if` with a true condition returns the true-branch value
2. `if` with a false condition and no `else` returns `null`
3. `if/else` returns the chosen branch value
4. an `else if` chain resolves the first matching branch
5. `fail` inside a branch aborts route execution
6. `reply` inside a branch aborts route execution
7. branch-local variables do not leak outside the block
8. a pipeline applied after the dedented `if` consumes the full `if` result
9. single-line conditions parse correctly and multi-line conditions are rejected clearly until supported
10. nested indentation is parsed correctly
