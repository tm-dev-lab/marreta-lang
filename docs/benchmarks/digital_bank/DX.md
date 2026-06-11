---
title: "Digital Bank Benchmark - Developer Experience"
summary: "Objective DX metrics computed from the four identical apps: total source size, direct dependencies, and a built-in-vs-library capability matrix. Regenerate with dx/measure.py."
---

# Digital Bank Benchmark - Developer Experience

Computed by `dx/measure.py` from the four apps that implement the identical contract.
SLOC is non-blank, non-comment lines, counted by one rule for all four apps (block comments
stripped for .ts/.java). A business-vs-wiring breakdown is not published as a number: it
needs subjective per-line classification on single-file apps; anyone who wants it can derive
it from the open sources (see METHODOLOGY).

## Code size and dependencies

| Stack | Total SLOC | Direct deps |
|---|--:|--:|
| marreta | 87 | 0 |
| fastapi | 118 | 3 |
| nest | 236 | 9 |
| spring | 299 | 4 |

## Built-in vs library capability matrix

| Capability | marreta | fastapi | nest | spring |
|---|---|---|---|---|
| Relational DB | built-in | library | library | starter |
| Document DB | built-in | library (motor) | library (mongoose) | starter (data-mongodb) |
| Cache | built-in | library | library | starter |
| Queue / Topic | built-in | library | library | starter |
| HTTP client | built-in | library (httpx) | library | library |
| Validation | built-in | built-in (pydantic) | library (class-validator) | starter (validation) |
| OpenAPI | built-in | built-in | library (swagger) | library (springdoc) |
| Tests | built-in | library (pytest) | built-in (jest) | starter (test) |

## Test feedback loop

The same provider-free, route-level suite per stack (parity of strategy: each hits the
API in-process, exercising real validation and business logic, with only the data
provider mocked). The time is each framework's own reported test time (marreta, pytest,
jest, surefire), which excludes container start and base-image differences; wall time is
kept alongside in `feedback.json`.

| Stack | Test run (s) | Isolation |
|---|--:|---|
| marreta | 0.021 | in-memory scenario runner; provider stubbed |
| fastapi | 0.48 | in-process ASGI (TestClient); no network server; provider mocked |
| nest | 3.934 | in-process app (supertest); no network server; provider mocked |
| spring | 2.525 | MVC test slice (@WebMvcTest); no server; provider mocked |
