# MarretaLang — File Encapsulation & Module Runtime Semantics

**Status:** ✅ Implemented.

## Overview

The current multi-file loader solves one problem well: it lets a project be
split across many `.marreta` files without introducing imports. But it does so
with an implementation shortcut that breaks the language's own visibility model.

Today, in non-entrypoint files:

- `route` / `on queue` / `on topic` declarations are preserved
- `export ...` declarations are preserved
- everything else is discarded

This means that a non-exported task or constant is not merely "private to the
file" — it is effectively **erased**. As a result, a route in
`routes/iteration.marreta` cannot call a task defined in the same file unless
that task is marked `export`, even if no other file should see it.

That is not real encapsulation. It turns `export` into a survival flag instead
of a visibility modifier.

The goal of this plan is to make file boundaries semantically honest:

- symbols declared without `export` remain usable **inside the same file**
- symbols declared with `export` are usable **across files**
- exported tasks keep access to their own private helpers/constants
- private names from different files never collide

This plan upgrades the current "shared-global + discard private" model into a
real module runtime model, while preserving MarretaLang's no-import,
convention-first philosophy.

---

## The Problem

### Current behavior

Given:

```marreta
# routes/iteration.marreta
task factorial(n)
    match n <= 1
        true -> 1
        fallback -> n * factorial(n - 1)

route GET "/factorial/:n"
    reply 200, { result: factorial(params.n) }
```

The route fails at runtime unless `task factorial` is written as `export task factorial`.

Why?

- `file_loader.rs` keeps the route
- the non-exported task is dropped during loading
- the route later executes without the task ever entering runtime scope

So the current semantics are:

> non-exported top-level declarations in non-entrypoint files are not private;
> they are unavailable.

That contradicts the v0.3.2 multi-file design, which says non-exported symbols
should remain private to their file.

### Why this matters

This is not just a DX annoyance. It creates structural problems:

1. `export` becomes overloaded.
   It now means both "make visible to other files" and "keep alive for this file."

2. Encapsulation is fake.
   A helper task cannot be private if using it internally requires exporting it.

3. Exported tasks cannot safely depend on private helpers.
   Even if a task is exported intentionally, its private support code from the
   same file is currently not part of a stable module context.

4. The runtime model is misleading.
   The language appears file-organized, but execution is effectively global-only
   plus route preservation.

---

## Design Goal

MarretaLang should adopt this visibility model:

### Entry point (`app.marreta`)

- Everything declared in `app.marreta` is implicitly global, as today.
- `export` remains unnecessary in `app.marreta`.

### Non-entrypoint files

- `export` makes a symbol visible to other files.
- no `export` means the symbol is private to that file.
- private symbols remain available to:
  - routes declared in that file
  - queue/topic consumers declared in that file
  - tasks declared in that file
  - exported tasks declared in that file

### Lookup rule

At runtime, identifier resolution should behave conceptually as:

1. local execution scope (params, route vars, task locals)
2. defining file/module scope
3. shared global/exported scope

This is the minimum needed to make "private to file" real.

---

## Non-Goals

This plan does **not** introduce:

- explicit `import` syntax
- package/module names in source code
- nested modules
- public/private keywords beyond `export`
- arbitrary top-level side-effect execution policy changes unless explicitly described

The intent is to fix visibility semantics, not redesign the language into a
general module system.

---

## Proposed Runtime Model

The key idea:

> each `.marreta` file becomes a runtime module with its own persistent private
> environment, while exports are additionally published to a shared public environment.

### New conceptual objects

#### 1. `ModuleId`

A stable identifier for each source file, usually derived from its relative path
or normalized file stem/path pair.

Examples:

- `app`
- `routes/iteration`
- `tasks/auth`

#### 2. `ModuleDefinition`

Produced by the loader for each file.

```rust
struct ModuleDefinition {
    id: ModuleId,
    is_entrypoint: bool,
    startup_private: Vec<Statement>,
    startup_public: Vec<Statement>,
    routes: Vec<RouteDefinition>,
    consumers: Vec<ConsumerDefinition>,
    private_schemas: HashMap<String, SchemaDefinition>,
    public_schemas: HashMap<String, SchemaDefinition>,
}
```

Meaning:

- `startup_private`: top-level declarations visible only inside the file
- `startup_public`: exported declarations (or all declarations for `app.marreta`)
- routes/consumers keep their source module identity

#### 3. `ModuleRuntime`

The persistent runtime state for a file after boot.

```rust
struct ModuleRuntime {
    id: ModuleId,
    private_env: Environment,
    public_env: Environment,
}
```

`private_env` contains the file's full internal world.

`public_env` contains only what is exported from that file
or implicitly global from `app.marreta`.

#### 4. `ProjectRuntime`

Built once at startup and shared by the server:

```rust
struct ProjectRuntime {
    global_public_env: Environment,
    modules: HashMap<ModuleId, ModuleRuntime>,
    public_schemas: HashMap<String, SchemaDefinition>,
    module_private_schemas: HashMap<ModuleId, HashMap<String, SchemaDefinition>>,
}
```

---

## Core Semantics

### 1. Private declarations survive

In non-entrypoint files, top-level declarations are no longer discarded.
They are loaded into the owning module's `private_env`.

So this becomes valid:

```marreta
# routes/iteration.marreta
task factorial(n)
    match n <= 1
        true -> 1
        fallback -> n * factorial(n - 1)

route GET "/factorial/:n"
    reply 200, { result: factorial(params.n) }
```

without `export`.

### 2. `export` means cross-file visibility only

`export` publishes a symbol from the module's private runtime into the shared
public runtime.

That is, `export` answers only this question:

> can another file refer to this symbol?

It should not affect whether the symbol is available within its own file.

### 3. Exported tasks retain module context

This is the most important semantic rule.

If file `tasks/auth.marreta` contains:

```marreta
task hash_password(raw)
    raw.upper()

export task register_user(name)
    hash = hash_password(name)
    { name: name, hash: hash }
```

then `register_user()` must remain able to call `hash_password()` even when
invoked from another file.

That means exported tasks cannot be represented as naked bodies detached from
their defining module. They must carry their **owner module** as part of the
runtime value.

Proposed shape:

```rust
Value::Task {
    name: String,
    params: Vec<ParamDef>,
    body: TaskBody,
    owner_module: Option<ModuleId>,
}
```

`owner_module = None` is allowed for legacy/simple contexts such as REPL or
single-file mode.

### 4. Routes and consumers execute with module context

`RouteDefinition` and `ConsumerDefinition` already carry `source_file`.
This should be upgraded from "OpenAPI tag metadata" into true runtime identity.

When a route or consumer runs:

- request-local scope is created as today
- the owning module's private environment is available underneath
- shared public/global environment is available underneath that

Conceptually:

```text
RequestScope
  -> ModulePrivateScope
    -> GlobalPublicScope
```

### 5. Private schemas are resolved within the module

Schemas should follow the same rule as tasks/constants:

- non-exported schema: usable only inside that file
- exported schema: usable across files

For route payload validation and response serialization:

1. look in the route's module-private schema registry
2. if not found, look in public schemas

This allows:

```marreta
schema internal_payload
    name: string

route POST "/internal" take payload as internal_payload
    reply 200, payload
```

without exporting the schema.

---

## Loader Strategy

The existing two-pass model is directionally correct, but needs a new output.

### Phase 1 — Parse all files into `ModuleDefinition`

Instead of flattening directly into one merged registry, the loader first
produces one `ModuleDefinition` per file.

For non-entrypoint files:

- exported declarations go to `startup_public`
- non-exported declarations go to `startup_private`
- routes/consumers are stored with `module_id`
- schemas are split into public/private registries

For `app.marreta`:

- all declarations behave as public/global
- `startup_private` is empty

### Phase 2 — Build module runtimes

For each module:

1. create an empty `private_env`
2. execute private + public startup declarations into that environment
3. derive `public_env` from the exported subset (or full env for entrypoint)

Important:

> declarations must be executed in the module's own environment first, so
> exported tasks/constants can resolve private helpers from the same file.

### Phase 3 — Build shared public runtime

Merge all module `public_env`s into one `global_public_env`, with startup-time
conflict detection for duplicate exported names.

This keeps the existing "fail fast on ambiguous export" property.

---

## Environment & Interpreter Changes

### Option A — Layered Environment (recommended)

Evolve `Environment` from a single stack of scopes into a stack plus optional
parents/layers.

Conceptually:

```rust
struct Environment {
    scopes: Vec<HashMap<String, Value>>,
    module_parent: Option<Arc<Environment>>,
    global_parent: Option<Arc<Environment>>,
}
```

Lookup walks:

1. current local scopes
2. module parent
3. global parent

Advantages:

- clean lexical semantics
- routes/tasks/consumers can share the same resolution model
- exported tasks can run in their owner module context naturally

### Option B — Manual environment merging

At route/task start, clone and merge module-private + global-public symbols into
a fresh environment.

This is simpler to implement initially, but weaker semantically:

- more copying
- trickier shadowing behavior
- more error-prone for nested task calls and future features

**Recommendation:** use Option A.

---

## Compatibility Rules

### Source compatibility

Existing code that currently uses `export` only for cross-file visibility keeps
working unchanged.

Existing code that unnecessarily added `export` just to make same-file tasks
work also keeps working, but becomes redundant.

### Behavioral compatibility

The only intentional semantic shift is:

> non-exported top-level declarations in non-entrypoint files become available
> within that file instead of being discarded.

That is a bug fix / semantic correction, not a new language concept.

### Conflict behavior

No change:

- duplicate exported names across files remain startup errors
- duplicate private names across different files remain isolated and valid

---

## Acceptance Criteria

1. A task declared without `export` in `routes/iteration.marreta` is callable by a route in the same file.
2. The same non-exported task is **not** callable from another file.
3. An exported task is callable from another file.
4. An exported task can reference a private helper task/constant/schema from its own file.
5. A non-exported schema can be used by routes in the same file for request validation and response serialization.
6. Two files may each declare `task helper()` privately without conflict.
7. Two files exporting `task helper()` still fail at startup with a descriptive export conflict.
8. `app.marreta` remains implicitly global; no `export` needed there.
9. Existing single-file mode behavior remains unchanged.

---

## Implementation Phases

### Phase 1 — Model the file as a module

Files:

- `src/file_loader.rs`
- `src/route_loader.rs`
- `src/ast.rs`

Deliverables:

- introduce `ModuleId`
- loader returns module-aware structures instead of flattening private symbols away
- routes/consumers keep stable module ownership

### Phase 2 — Persistent module runtimes

Files:

- `src/file_loader.rs`
- `src/server.rs`
- `src/interpreter.rs`
- `src/environment.rs`

Deliverables:

- build `ModuleRuntime` per file
- build shared `ProjectRuntime`
- route execution starts with module-aware scope chain

### Phase 3 — Task lexical ownership

Files:

- `src/value.rs`
- `src/interpreter.rs`

Deliverables:

- `Value::Task` stores owning module
- task calls resolve against owner module, not accidental caller context

### Phase 4 — Schema visibility parity

Files:

- `src/server.rs`
- `src/response_serializer.rs`
- `src/validator.rs`
- `src/file_loader.rs`

Deliverables:

- private schema lookup by module
- public schema lookup by export/global visibility

### Phase 5 — Tests

Required coverage:

- unit tests for loader module partitioning
- route/runtime tests for same-file private task access
- cross-file failure tests for non-exported access
- exported-task-uses-private-helper test
- private schema route validation test
- regression test reproducing the original `iteration.marreta` problem

### Phase 6 — Functional verification

Add a dedicated functional scenario showing:

- same-file private task used by a route
- exported task imported across files
- private helper inaccessible from another file

---

## Design Notes

### Why not just "keep all startup statements globally"?

Because that would solve availability but destroy encapsulation.

It would make every top-level declaration in every file global, which is exactly
what `export` was meant to avoid.

### Why not add imports?

Because the current philosophy is still valid:

- zero import boilerplate
- project tree scanning by convention
- `export` as the only visibility modifier

The problem is not lack of imports. The problem is that private runtime state is
currently thrown away.

### Why lexical task ownership matters

Without owner-module semantics, "private helper" remains unreliable. The task
would resolve names from wherever it is called, not where it was defined. That
would create dynamic-scope-like surprises and make encapsulation partial.

Real encapsulation requires definition-site context.

---

## Recommendation

Implement this plan before adding any richer module or import system.

As long as file-private declarations in non-entrypoint files are discarded, the
multi-file story remains semantically incomplete. Fixing this now strengthens:

- correctness
- developer intuition
- future modularity work
- trust in `export` as a real visibility keyword

This is not feature creep. It is finishing the semantics that v0.3.2 already
claimed to provide.
