---
title: "Lint codes"
category: reference
slug: "reference/lint"
summary: "Every diagnostic marreta lint can emit, what it flags, why it matters, and how to fix it."
---

# Lint codes

`marreta lint` reports a small, high-signal set of diagnostics. Each carries a code, shown in the CLI
output and in the editor. This page documents every code, grouped by what it protects.

Any warning can be silenced on a single line with an inline directive:

```ruby
# marreta: allow route_without_response
route GET "/health"
    log.info("alive")
```

A directive on its own line silences the next line. A trailing directive silences its own line:

```ruby
total = subtotal  # marreta: allow unused_variable
```

Run with `--strict` to make warnings fail the command, for CI.

## Load-time

### source_load_error

The file failed to load (a syntax error, a schema cycle, or an invalid configuration) and the project
cannot run until it is fixed. This is an error, not a warning, and `marreta doctor` reports it too.
Fix the underlying source problem.

## Routes and responses

### route_without_response

A route path can finish without `reply` or `fail`, and a route that returns no response sends a silent
`204 No Content`. Most often this means a branch forgot its `reply`. End every path with `reply` or
`fail`. The analysis is conservative and local to the route body: `if` needs both branches to respond,
`match` needs every arm plus a `fallback`, and a `reply`/`fail` inside a `rescue` recovery block does
not count toward the happy path. If an empty `204` is intended, suppress the line.

### duplicate_route

Two routes share the same verb and path pattern, so one shadows the other at registration. Give them
distinct paths or merge them.

## Correctness

### match_without_fallback

A `match` whose value is used (assigned, or passed somewhere) has no `fallback` arm. When no arm
matches, the runtime returns a silent `null` that surfaces as an error far from its origin. Add a
`fallback ->` arm. A bare `match` used only for its side effects, where the value is discarded, is not
flagged. The check works on the top-level statements of a body.

### unknown_schema_reference

A validation, response, field type, or constructor references a schema name that is not declared.
Declare the schema or fix the name.

### invalid_feature_flag_name

A feature flag name is not a valid `MARRETA_FEATURE_*` identifier, so it can never resolve. Use a name
that matches the flag convention.

### suspicious_self_recursive_task

A task calls itself with no visible base case, a likely infinite recursion. Add a guard, or rewrite it.

### unreachable_statement

A statement follows a terminating statement (`reply`, `fail`, an unconditional `raise`) and can never
run. Remove it, or move it before the terminator.

## Security

### non_literal_sql_identifier

A `db` `order_by` clause, a `select` computed alias, or a `like`/`in` field is built from a runtime
value instead of a literal. Filter values are parameterized, but the identifier is written into the
SQL string as-is, so a runtime value there (including an interpolated string) is an injection vector.
Use a literal for the identifier. An interpolated `order_by("created_at #{params.dir}")` is flagged
for the same reason. This lint warns, it does not sanitize: the runtime guard is a separate hardening
follow-up. The check applies to the relational provider only, not to `doc` pipelines.

### non_flat_input_schema

A schema bound to query or headers (`take query as Schema` / `take headers as Schema`) is not flat.
Query and header parameters are flat on the wire, so a schema bound there may only use scalar fields
and lists of scalars. A field that references another schema (a nested object), or a list of objects
(`list of SomeSchema`), is flagged. Keep query and header schemas flat, or use the raw `take query` /
`take headers` for arbitrary input. This is also a load-time error; the lint surfaces it at dev time.

## Unused declarations

### unused_variable

A local variable is assigned but never read. Remove the assignment, or use the value.

### unused_private_task

A file-private task is declared but never called. Remove it, export it, or call it.

### unused_exported_task

An exported task is never called from any file (by its `file.task` namespace, or within its own file).
Call it, use it in its own file, or drop the export.

### unused_schema

A non-persistent schema is declared but never referenced by a validation, a response, a field type, or
a constructor. Use it, reference it from another schema, or remove it. A persistent (`db:`) schema is
never flagged here, because it defines a table that can be in use without an explicit reference.

### unused_auth_provider

An auth provider is declared but no route requires it. Require it on a route with `require auth
<name>`, or remove it.

### shadows_injected_binding

A local in a route reuses the name of a runtime-injected binding (`params`, `query`, `headers`, `auth`,
or a `take` binding), hiding the injection for the rest of the scope. Rename the local. The check is
scope-aware: a name is only flagged where that binding is actually live in the route. It analyzes the
top-level statements of a body and its `while`/`transaction` blocks, not assignments buried inside an
`if` or `match` expression branch.
