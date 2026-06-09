# v0.13b — Project Metadata Unification

> Status: Delivered.

## Motivation

`019_PROJECT_ENTRYPOINT.md` established `app.marreta` as the canonical project
entrypoint and made these assignments mandatory:

```marreta
project_name = "my-api"
project_version = "0.1.0"
```

However, parts of the runtime and examples still rely on older metadata names:

```marreta
api_name = "My API"
api_version = "1.0"
```

That leaves the language with two overlapping concepts for the same thing:

- project identity
- API/OpenAPI identity

This duplication weakens the entrypoint convention introduced in `019` and
keeps `app.marreta` semantically inconsistent.

## Goal

Make `project_name` and `project_version` the only valid metadata fields for
project identity across MarretaLang.

After this change:

- `project_name` is the canonical human-facing project title
- `project_version` is the canonical version string
- OpenAPI and `/docs` use those values directly
- older `api_name` and `api_version` conventions are removed

## Proposal

`app.marreta` remains the required project entrypoint.

The canonical minimal project becomes:

```marreta
project_name = "ecommerce-api"
project_version = "1.0.0"
```

No fallback or compatibility layer is introduced.

The language is still under construction, so it is better to remove duplicate
metadata now rather than support two naming systems.

## Runtime semantics

### Canonical project metadata

These top-level string assignments in `app.marreta` are mandatory:

- `project_name`
- `project_version`

They represent:

- project identity for CLI/runtime surfaces
- OpenAPI `info.title`
- OpenAPI `info.version`
- Swagger UI visible metadata
- startup banner metadata for `marreta serve`
- built-in health metadata where exposed

### Removed metadata

The following conventions are removed:

- `api_name`
- `api_version`

They should no longer be required, documented, or used by the runtime.

Project entrypoints should stop using them entirely.

## OpenAPI and docs behavior

After `019b`, the OpenAPI document should use:

- `info.title = project_name`
- `info.version = project_version`

Swagger UI should reflect the same values through the generated OpenAPI
document.

This makes `/docs` and `/openapi.json` align with the canonical project
entrypoint.

## Health behavior

If the health response includes project metadata, it should also use:

- `project_name`
- `project_version`

No parallel `api_*` naming should remain in runtime responses.

If the current health payload shape already exposes stable keys, the preferred
implementation is to keep the response shape stable and change the value source
to `project_name` / `project_version`, instead of renaming fields without need.

## Example

```marreta
project_name = "shop-api"
project_version = "2.1.0"

route GET "/health"
    reply 200, { ok: true }
```

Expected OpenAPI metadata:

```json
{
  "info": {
    "title": "shop-api",
    "version": "2.1.0"
  }
}
```

## Scope

Included in `019b`:

- use `project_name` / `project_version` as the only canonical metadata
- remove `api_name` / `api_version` from examples and docs
- update OpenAPI generation to use `project_*`
- update any health/docs/runtime metadata surfaces to use `project_*`
- update tests and fixtures accordingly
- remove `api_name` / `api_version` usage from project entrypoints so the old
  vocabulary no longer appears in committed project code

Not included in `019b`:

- secret/config work from `020_SECRET_AWARE_CONFIG.md`
- new optional metadata such as `project_description`
- project scaffolding/generation

## Implementation plan

### Phase 1 — Runtime metadata source

Update the runtime/OpenAPI path so that project metadata comes from
`project_name` and `project_version`.

Likely files:

- `src/openapi.rs`
- `src/server.rs`
- any helper that currently reads `api_name` / `api_version`

### Phase 2 — Example and fixture migration

Update committed examples and test fixtures so they stop declaring or depending
on `api_name` / `api_version`.

Likely files:

- `examples/functional_tests/app.marreta`
- `examples/ecommerce/app.marreta`
- `examples/migrations_functional/app.marreta`
- `tests/http_integration_tests.rs`
- any other project fixture using old metadata names

This phase should fully remove the old vocabulary from committed project
fixtures, not merely leave it unused.

### Phase 3 — Docs cleanup

Update specs and user-facing docs so project metadata is described only in
terms of `project_name` / `project_version`.

Likely files:

- `docs/spec/019_PROJECT_ENTRYPOINT.md`
- `docs/spec/SPEC.md`
- `docs/spec/002_HTTP.md`
- `CHANGELOG.md`

## Validation

Implementation is complete when:

1. `project_name` and `project_version` are sufficient for `/docs`
2. `/openapi.json` uses `project_name` / `project_version`
3. no committed example requires `api_name` / `api_version`
4. no runtime surface still depends on `api_name` / `api_version`
5. functional suites still pass

Suggested validation:

- `cargo test --lib`
- `cargo test --test http_integration_tests`
- `./examples/functional_tests/test.sh --docker`
- `bash examples/migrations_functional/test.sh`
- inspect `/openapi.json` and confirm:
  - `info.title == project_name`
  - `info.version == project_version`

## Risk

This change is intentionally breaking for any fixture, example, or project that
still uses `api_name` / `api_version`.

That is acceptable at the current maturity stage of the language, but it means
all committed project fixtures must be migrated in the same implementation pass.

## Language identity impact

This change strengthens the language in three ways:

1. one concept now has one name
2. the project entrypoint becomes semantically meaningful, not just structural
3. OpenAPI and runtime metadata become aligned with the same project contract

## Open questions

- Should future metadata such as description/contact/license also live as plain
  top-level assignments in `app.marreta`?
- Should health always expose `project_name` / `project_version`, or only when
  docs/OpenAPI are enabled?
