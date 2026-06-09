# Implementation Plan — v0.11 Iteration & Accumulation

**Status:** ✅ Complete

## Overview

v0.11 closes the gap between MarretaLang's declarative pipeline model and the
imperative constructs every developer expects from a general-purpose language.
The language already handles conditionals (`match`, `require`) and list
transformation (`map/keep`) well. What it cannot do today:

- Generate a numeric sequence (no range)
- Accumulate a value over a list with custom logic (no reduce)
- Repeat an operation while a condition holds (no while)
- Call a task from within itself (no recursion)

These limitations force developers to reach outside the language for problems
that are fundamentally simple. The guiding principle for this version:

> **Better to have and not need than to need and not have.**
> Every construct added here must feel like it belongs — same syntax rhythm,
> same English keywords, same pipeline-first bias. Nothing from this version
> should look alien next to `map/keep` or `require ... else fail`.

---

## Syntax Reference

### `range` — numeric sequence generator

`range` is a built-in function that returns a `List` of integers. It feeds
naturally into `map`, `keep`, `reduce`, and any pipeline stage.

```marreta
# range(end) — 1-based, inclusive
numeros = range(10)           # [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]

# range(start, end) — inclusive on both ends
numeros = range(5, 10)        # [5, 6, 7, 8, 9, 10]

# range + map — "print 1 to 10"
resultado = range(10)
    >> map i
        keep { n: i, dobro: i * 2 }

# range + keep — pares de 1 a 20
pares = range(20)
    >> map n
        keep n if n % 2 == 0

# range + reduce — somar 1 a 100
soma = range(100) >> reduce(0) acc, n
    acc + n
```

**Design decisions:**

- `range(n)` starts at **1**, not 0. APIs deal with counts and IDs — 1-based
  is the natural default (pages, ranks, iterations). Developers who need
  0-based write `range(0, n-1)`.
- Both forms are inclusive at both ends, consistent with how MarretaLang
  handles all ranges in the language (no exclusive upper bound surprise).
- `range` is a function, not syntax. No `1..10` literal — that would require
  a new token and creates ambiguity with subtraction.

---

### `reduce` — accumulate a list into a single value

`reduce` is a pipeline stage, consistent with `map`. It takes an initial value
and a two-variable block that combines the accumulator with each list item.

```marreta
# Basic: sum
soma = [1, 2, 3, 4, 5] >> reduce(0) acc, n
    acc + n
# → 15

# Product
produto = range(5) >> reduce(1) acc, n
    acc * n
# → 120

# Build a string
frase = ["Ana", "João", "Carlos"] >> reduce("") acc, nome
    acc + nome + " "
# → "Ana João Carlos "

# Max value
maior = [3, 7, 2, 9, 4] >> reduce(0) acc, n
    match n > acc
        true => n
        false => acc

# Pipeline chain: range → reduce
soma_pares = range(20)
    >> map n
        keep n if n % 2 == 0
    >> reduce(0) acc, n
        acc + n
# → 110
```

**Syntax:**

```
EXPR >> reduce(INITIAL) ACC, ITEM
    BODY
```

- `INITIAL` is any expression — integer, string, list, map.
- `ACC` is the accumulator variable name (user-chosen).
- `ITEM` is the current element variable name (user-chosen).
- The `BODY` uses the exact same `TaskBody` AST model as regular declarations. It can be a single expression or a block where the **last expression** becomes the new accumulator value.
- Returns the final accumulator value after all items are processed.
- If the list is empty, returns `INITIAL` unchanged.

**Why as a pipeline stage, not a built-in function?**

`reduce(list, initial, fn)` as a function requires passing a function reference
— MarretaLang doesn't have first-class functions. The pipeline stage form keeps
the block syntax consistent with `map` and is more readable at the call site.

---

### `while` — condition-based loop

`while` is the escape hatch for imperative logic that cannot be expressed as
a pipeline. It is intentionally the most "foreign" construct in the language —
if you reach for `while`, ask first whether `map` + `reduce` + `range` solve
the problem.

```marreta
# Contar até 5
contador = 0
while contador < 5
    contador = contador + 1
# contador == 5

# Acumular até threshold
soma = 0
i = 1
while soma < 1000
    soma = soma + i
    i = i + 1
# soma is the first sum that exceeds 1000

# Retry pattern — call external service up to 3 times
tentativas = 0
resultado = null
while tentativas < 3 and not resultado
    resultado = http_client.get("https://service/status") rescue null
    tentativas = tentativas + 1
require resultado else fail 503, "service unavailable"
reply 200, resultado.body

# Consume items until sentinel
index = 0
acumulado = []
while index < payload.itens.length()
    item = payload.itens[index]
    acumulado = acumulado.push(item) if item.ativo
    index = index + 1
```

**Syntax:**

```
while CONDITION
    BODY
```

- `CONDITION` is any expression that evaluates to a boolean or truthy value.
- `BODY` is an indented block of statements.
- Variables mutated inside the block are visible outside (same scope).
- The loop returns `null` — it is a statement, not an expression.
- **Safety:** a hard limit of **10 000 iterations** per loop. Exceeding it raises
  `RuntimeError` with a clear message. This prevents runaway routes from hanging
  the server. The limit is intentionally non-configurable — if you need more
  than 10k iterations in a route, the logic belongs in a background job.

**Design position:**

`while` is the only imperative escape hatch in the language. It is deliberately
kept minimal — no `break`, no `continue`, no `loop`. If you need early exit,
invert the condition. If you need to skip items, use `map/keep` instead.

---

### Recursion in tasks

Tasks may call themselves. MarretaLang enforces a **call depth limit of 500**
(configurable via `MARRETA_MAX_RECURSION_DEPTH`). Exceeding it raises
`RuntimeError: maximum recursion depth exceeded`.

```marreta
# Factorial
task fatorial(n)
    match n <= 1
        true => 1
        false => n * fatorial(n - 1)

# Fibonacci
task fib(n)
    match n <= 1
        true => n
        false => fib(n - 1) + fib(n - 2)

# Flatten nested list (practical example)
task flatten_deep(lista)
    lista
        >> map item
            match type(item) == "list"
                true => flatten_deep(item)
                false => item
```

**Note on mutual recursion:** Task A calling Task B calling Task A is also
supported — the depth counter spans the full call stack.

---

### New list methods

Complement `map/keep/reduce` with one-liner methods for common data aggregation and pairing. Note that filtering, finding, and mapping are deliberately left out of this list to preserve the **Pipeline-first** bias of the language. Instead of predicate-based methods (`list.any() u: ...`), developers use the native pipeline stages.

```marreta
# sum — integer and float lists
total = [1, 2, 3, 4, 5].sum()          # 15

# Statistical methods on numeric lists
notas = [7.5, 8.0, 6.5, 9.0, 7.0]
media    = notas.mean()            # 7.6
mediana  = notas.median()          # 7.5
desvio   = notas.std_dev()         # standard deviation

# zip — pair two lists by index
pares = nomes.zip(idades)   # [["Ana", 30], ["João", 25]]
```

**Idiomatic Pipeline equivalents for predicates:**
Instead of fighting the execution model with fake lambdas, use what the language relies on to modify iteration control flows.
```marreta
# any/all
tem_admin = (usuarios >> map u \n keep u if u.role == "admin").length() > 0

# find
admin = (usuarios >> map u \n keep u if u.role == "admin")[0]

# flatten
todos_itens = pedidos >> map p
    p.itens
  >> reduce([]) acc, itens
    acc + itens
```

**Design decisions:**
- **No Vector Operations (R-inspired).** Unlike pandas or R, MarretaLang does NOT broadcast arithmetic operators (`+`, `-`, `>`, etc) across lists. It conflicts directly with the explicit pipeline and dynamic type safety. To transform data, `>> map` and `>> keep` are the one true path.
- **Statistical methods on non-numeric lists** raise `TypeError`. `sum()`, `mean()`, `median()`, `std_dev()` are numeric-only.

---

### Type conversions

```marreta
# String → Integer
idade = "25".to_integer()     # 25

# String → Float
preco = "19.90".to_float()    # 19.9

# Integer / Float → String
label = 42.to_string()        # "42" (already exists in language, but noted here for completeness)

# Any → Boolean
flag = valor.to_boolean()     # null/false/0/"" → false, everything else → true
```

`to_integer()` on a non-numeric string returns `0`. `to_float()` same. These never raise — callers who need strictness use `require`.

---

## Environment Variables

One new env var is introduced. The iteration depth and while loop limit are built-in safety
rails, but the recursion limit is selectively tunable.

```
MARRETA_MAX_RECURSION_DEPTH=500    # optional, default 500
```

---

## Error Semantics

| Failure | Behavior |
|---|---|
| `while` loop exceeds 10 000 iterations | Raises `RuntimeError` |
| Recursion depth exceeds limit | Raises `RuntimeError` |
| `range(start, end)` where start > end | Returns empty list `[]` |
| `reduce` on empty list | Returns `INITIAL` unchanged |
| `to_integer()` on non-numeric string | Returns `0` |
| `to_float()` on non-numeric string | Returns `0.0` |
| `mean()` / `median()` / `std_dev()` on non-numeric list | Raises `TypeError` |
| `mean()` / `median()` / `std_dev()` on an empty list | Returns `null` |
| `sum()` on an empty list | Returns `0` |
| `zip()` with lists of different lengths | Raises `RuntimeError` |

---

## AST & Parser

### New tokens

| Token | Keyword |
|---|---|
| `TokenKind::While` | `while` |
| `TokenKind::Reduce` | `reduce` |

`range` is not a keyword — it is parsed via the existing identifier + call machinery.

### New AST nodes

```rust
// while loop statement
Statement::While {
    condition: Box<Expression>,
    body: Vec<Statement>,
}

// reduce pipeline stage
PipelineStage::Reduce {
    initial: Box<Expression>,
    accumulator: String,
    item: String,
    body: TaskBody,   // Reuses the TaskBody structure for implicit returns!
}
```

`range` is a new case in `builtin_function()` — no AST node needed.

---

## Implementation Phases

### Phase 1 — `range` built-in + `reduce` pipeline stage

**Scope:** Core iteration primitives. Closes cases 5, 6, and 9 from the
motivation list.

- `range(n)` / `range(start, end)` in `builtin_function()`
- `TokenKind::Reduce` + `PipelineStage::Reduce` in AST
- Parser: `>> reduce(INITIAL) ACC, ITEM\n  BODY`
- Interpreter: `evaluate_reduce_stage()` — iterate list, run body, last
  expression becomes new accumulator
- Unit tests: empty list, integer sum, string concat, map construction,
  chained with `map/keep`

**Files:** `src/token.rs`, `src/ast.rs`, `src/parser.rs`, `src/interpreter.rs`

### Phase 2 — `while` loop + safety limit

**Scope:** Imperative escape hatch. Closes case 8 and arbitrary condition loops.

- `TokenKind::While`
- `Statement::While { condition, body }`
- Parser: `while CONDITION\n  BODY`
- Interpreter: `execute_while()` — iteration counter, 10k limit, mutates
  existing scope (no new scope pushed)
- Unit tests: basic counter, early convergence, limit enforcement, nested while

**Files:** `src/token.rs`, `src/ast.rs`, `src/parser.rs`, `src/interpreter.rs`

### Phase 3 — Recursion

**Scope:** Allow tasks to call themselves (and each other).

- Add `call_depth: usize` counter to `Interpreter`
- Increment on `call_task_value()`, decrement on return
- Check against `max_recursion_depth` from config (default 500)
- The depth limit is tracked within the `Interpreter` directly.
- Unit tests: factorial, fibonacci, mutual recursion, depth limit enforcement

**Files:** `src/interpreter.rs`, `src/main.rs`

### Phase 4 — Data methods and Type conversions

**Scope:** List aggregate functions and simple scalar coercions.

- `sum()`, `mean()`, `median()`, `std_dev()`, `zip()` on `Value::List` in `call_method()`
- `to_integer()`, `to_string()`, `to_float()`, `to_boolean()` on all scalar types
- Unit tests for methods and conversions

**Files:** `src/value.rs`, `src/interpreter.rs`

### Phase 5 — Functional tests & docs

- `examples/functional_tests/routes/iteration.marreta` — Section 31
- Must include strict test coverage for all edge cases verified by `test.sh`:
  - `range` inclusive bounds validation
  - `reduce` over lists and empty lists fallback
  - `while` loop aborting at the 10,000 threshold (`RuntimeError`)
  - Recursion aborting at depth 500 (`RuntimeError`)
  - `zip` mismatching lengths raising `RuntimeError`
  - Empty lists returning `null` in stats (`mean`, `median`, `std_dev`)
  - `to_integer()`/`to_float()` fallback to `0` / `0.0`
- `examples/functional_tests/test.sh` — Section 31
- `docs/spec/SPEC.md` — new section covering all new constructs
- `CHANGELOG.md` update
- Mark `016_ITERATION.md` ✅ Complete

---

## Design Watch Points

### 1. `range(n)` is 1-based — deliberate

Zero-based ranges (`range(10)` → `[0..9]`) are a programmer convenience
inherited from C. API developers think in counts and ranks — "10 items", "page
1", "rank 3". Starting at 1 is the natural default for this language's audience.
Developers who need 0-based use `range(0, n-1)`.

### 2. `while` has no `break`/`continue` — deliberate

`break` inverts the loop condition — it's always expressible as a tighter
`while` condition or a boolean flag. `continue` is a skip — expressible with
an `if` guard inside the body. Excluding them keeps the construct minimal and
forces cleaner loop conditions.

### 3. `reduce` body last-expression semantics

The accumulator update is the **last expression** in the body — not an explicit
`acc = ...` assignment. This mirrors how tasks return values (last expression)
and keeps the reduce block free of imperative noise. A developer who assigns
`acc = ...` explicitly and then doesn't write a final expression will get a bug
— this is a known sharp edge, documented prominently.

### 4. Rejection of Predicate Methods

It is tempting to add `.any()`, `.all()`, and `.find()` with some trick syntax like `list.any() u: u.ativo`, but doing so directly contradicts MarretaLang's execution guarantees. Arguments are evaluated *eagerly* before the caller handles them. Evaluating a variable like `u` before the scope assigns it causes undefined variable crashes. Since adding lambdas/closures breaks Zero Ceremony design principles, those methods were discarded. Developers filter and find via familiar and uniform pipeline composition (`keep` and array index).

### 5. `while` is the last resort

Documentation, functional tests, and error messages should reinforce that `map/keep/reduce/range` are the idiomatic tools. `while` is presented as the escape hatch — named explicitly in the spec, not in the quick-start guide.

### 6. Rejection of Vectorized Operations

Implicit broadcasting seen in R or Pandas (`lists > 50` producing boolean lists) were explicitly evaluated and rejected. A huge pillar of MarretaLang is explicit data flow (`>> map p \n p * 0.9`). Letting a developer modify vectors silently creates a second, competing paradigm that erodes the core Pipeline identity. Vector arithmetic also risks severe bugs due to unexpected list types leaking into what was assumed to be scalar math.

### 8. Recursion depth vs. tail-call optimization

Rust is not tail-call optimized. A `fatorial(500)` will consume 500 stack
frames. The 500-frame default is conservative — deep recursion should be
reimplemented iteratively with `reduce` + `range`. The limit exists to give
a clear error instead of a stack overflow segfault.
