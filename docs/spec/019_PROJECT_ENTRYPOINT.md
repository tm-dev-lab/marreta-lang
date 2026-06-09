# v0.13 — Project Entrypoint Convention

> Status: Delivered.

## Motivation

Today the Marreta CLI still thinks primarily in terms of an explicit `.marreta`
file path for commands that conceptually operate on a project:

```bash
marreta serve
marreta migrate apply
marreta migrate status
```

This is appropriate for `run`, which is intentionally file-oriented, but
unnecessarily file-oriented for `serve` and project lifecycle commands such as
`migrate`.

After `018_DB_MIGRATIONS.md`, this tension became clearer:

- `migrate diff|generate|status|list|discard|apply|rollback` operate on a project
- `018b_MIGRATION_HYGIENE.md` continues that same project-oriented migration flow
- `serve` also operates on a project
- the runtime already uses the entrypoint directory as a meaningful project root
- `marreta.env` is now loaded from the entrypoint project root

MarretaLang should adopt a first-class project convention.

## Goal

Define a canonical project entrypoint so that Marreta applications are loaded by
project convention, not by constantly passing explicit file paths.

## Proposal

Every Marreta project must have a root file named:

```text
app.marreta
```

This file becomes the canonical project entrypoint.

It must declare at least:

```marreta
project_name = "my-api"
project_version = "0.1.0"
```

`project_name` and `project_version` become required metadata for every valid
project.

## Consequences

With this convention, project-oriented CLI commands can default to the current
directory project:

```bash
marreta serve
marreta migrate diff
marreta migrate generate
marreta migrate status
marreta migrate apply
marreta migrate rollback
```

The runtime will resolve:

```text
./app.marreta
```

as the default entrypoint.

## Recommended CLI behavior

### Default resolution

When a command operates on a project and no explicit path is provided:

1. look for `./app.marreta`
2. if found, use it
3. if not found, fail with a clear project-level error

Suggested error:

```text
No Marreta project found in the current directory.
Expected ./app.marreta
```

### Explicit override

Even with the convention, the CLI should still allow explicit override.

Supported forms:

```bash
marreta serve /path/to/app.marreta
marreta migrate apply /path/to/app.marreta
```

The important rule is:

- convention-first by default
- explicit override still possible for advanced scenarios

This keeps monorepo, fixture, and non-standard layouts possible without making
them the default UX.

## Required project metadata

`project_name` and `project_version` should be mandatory in `app.marreta`.

Example:

```marreta
project_name = "ecommerce-api"
project_version = "0.1.0"
```

This metadata is useful immediately for:

- project identity in CLI output
- project version in CLI output and tooling
- future OpenAPI defaults
- migration metadata and operational logs
- future tooling and scaffolding

Optional future metadata may include:

- `project_description`
- `openapi_title`
- `openapi_version`

## Semantics

`app.marreta` is:

- the canonical project entrypoint
- the root used by the project loader
- the place for project-wide metadata
- a valid place for startup/global declarations

`app.marreta` is not intended to force all project code into one file.

The existing multi-file project model remains valid:

```text
my-api/
  app.marreta
  marreta.env
  routes/
    users.marreta
    orders.marreta
  schemas/
    models.marreta
  migrations/
```

## Relationship with `marreta.env`

This proposal does not introduce project-root configuration loading.

That behavior was already established in `018_DB_MIGRATIONS.md`, where
project-oriented commands load `marreta.env` from the entrypoint directory.

`019` builds on that by making the entrypoint itself conventional and
discoverable by default.

## Language identity impact

This proposal strengthens the language identity in three ways:

1. It makes Marreta feel project-oriented instead of file-oriented.
2. It reinforces convention over configuration.
3. It reduces repeated CLI ceremony without adding syntax to the language
   itself.

## Commands affected

### Project commands

These resolve `./app.marreta` by convention today:

- `serve`
- `migrate diff`
- `migrate generate`
- `migrate status`
- `migrate list`
- `migrate discard`
- `migrate apply`
- `migrate rollback`
- `migrate explain`

Potentially also:

- future scaffolding commands
- future validation/lint commands
- future packaging/deploy commands

### File/script commands

These should remain usable without a project entrypoint:

- `run`
- `repl`
- `tokenize`
- `parse`

Rationale:

- `run` is useful for simple standalone Marreta scripts
- `repl` is an exploratory language tool, not an application lifecycle command
- parser/tokenizer workflows are file-oriented by nature

## Backward compatibility

Recommended rollout:

### Phase 1

- `app.marreta` becomes the recommended convention
- project commands accept zero args and resolve `./app.marreta`
- explicit file path still works

### Phase 2

- docs/examples standardize on `app.marreta`
- error messages guide users toward the convention

### Phase 3

- decide whether some commands should reject non-project invocation entirely

This avoids breaking existing users abruptly.

## Validation

When implemented, validation should cover:

1. `marreta serve` in a project root resolves `./app.marreta`
2. `marreta migrate apply` in a project root resolves `./app.marreta`
3. `marreta migrate status` in a project root resolves `./app.marreta`
4. explicit path still works
5. missing `app.marreta` fails clearly
6. missing `project_name` fails clearly
7. missing `project_version` fails clearly
8. `marreta run script.marreta` still works without a project
9. examples and functional suites run without passing the entrypoint path when
   invoked from project root

## Open questions

To be decided in implementation planning:

- Should `project_name` and `project_version` remain plain top-level
  assignments, or become reserved metadata fields?
- Should commands search parent directories for `app.marreta`, or only the
  current directory?
- Should future scaffolding generate `app.marreta` automatically?
