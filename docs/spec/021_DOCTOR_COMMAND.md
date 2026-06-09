# v0.13 — Project Doctor Command

> Status: Delivered.

## Motivation

Marreta now has a stronger project model:

- `app.marreta` as project entrypoint
- `project_name` and `project_version` as required metadata
- structured infrastructure config for `db`, `doc`, `cache`, and `queue`
- migrations as part of the normal project lifecycle

As this surface grows, the CLI needs a simple way to validate that a project is
correctly configured before startup, migration, or deployment.

A `marreta doctor` command should provide that validation in one place.

## Goal

Add a `marreta doctor` command that validates project structure, runtime
configuration, and optional service connectivity in a way that is useful for:

- local development
- CI/CD
- deployment diagnostics

The command should be clear, fast, and oriented to actionable failures.

## Design principles

- validate only what the project intends to use
- avoid noisy warnings for modules that are irrelevant to the project
- distinguish structure validation from connectivity validation
- make failures explicit and actionable
- keep output human-readable first

## Project intent

The doctor command should not warn about every possible Marreta module by
default.

It should validate only the surfaces that the project actually intends to use.

Examples:

- if the project uses `db.*`, validate DB config
- if the project uses `doc.*`, validate document DB config
- if the project uses `cache.*`, validate cache config
- if the project declares queue consumers or uses queue publishing, validate
  queue config
- if the project does not use `queue`, do not emit `WARN queue not configured`

This keeps the output aligned with the actual project rather than the total
feature surface of the language.

## Command shape

Primary form:

```bash
marreta doctor
```

Connectivity form:

```bash
marreta doctor --connect
```

Project commands follow the same convention as `serve` and `migrate`: run from
the root of a Marreta project and resolve `./app.marreta`.

Optional explicit entrypoint override may still be supported:

```bash
marreta doctor app.marreta
```

The default command should validate:

- project structure
- project intent
- config completeness
- migration awareness

Live service connectivity should be opt-in via `--connect`.

## Validation layers

### 1. Project structure

Validate:

- `./app.marreta` exists
- `project_name` exists
- `project_version` exists
- project loads successfully

### 2. Intent discovery

Detect which subsystems the project actually uses.

Intent discovery should come from the loaded project model, not from heuristics
over raw text or the presence of env vars.

Sources of truth:

- `db.*` usage in the loaded AST / runtime model
- `doc.*` usage in the loaded AST / runtime model
- `cache.*` usage in the loaded AST / runtime model
- `queue.*`, `on queue`, and `on topic` usage in the loaded AST / runtime model
- `persistent_schemas` and `migrations/` for migration awareness

Examples:

- `db.*`
- `doc.*`
- `cache.*`
- queue consumers
- queue publishing
- presence of persistent schemas and `migrations/`

### 3. Config completeness

For each intended subsystem, validate that structured config is complete enough
for the selected provider.

Examples:

- missing `MARRETA_DB_PASSWORD` for `postgres`
- incomplete cache config for `redis`
- queue provider configured without required credentials

### 4. Connectivity

Attempt live connectivity only for intended and sufficiently configured
subsystems, and only when `--connect` is passed.

Examples:

- ping DB
- ping document DB
- ping cache
- connect to queue broker

### 5. Migration awareness

If the project uses persistent schemas and DB migrations:

- validate that DB config is present
- optionally show a short migration state summary

This should be informative, not a full replacement for `marreta migrate status`.

## Output model

The output should be grouped and readable.

Example:

```text
Project:
  OK  app.marreta found
  OK  project_name = ecommerce-api
  OK  project_version = 1.0.0

Intent:
  OK  db
  OK  cache
  OK  migrations

Config:
  OK  db provider = postgres
  OK  db host = localhost
  OK  cache provider = redis

Connectivity:
  OK  db connection
  OK  cache connection

Migrations:
  OK  2 applied
  OK  no pending migrations
```

If the project does not use `queue`, `doc`, or `cache`, those sections should
not appear just to say they are absent.

The command should use a small, predictable status vocabulary:

- `OK`
- `ERROR`
- `SKIP`

`SKIP` is only for explicitly skipped connectivity checks or intentionally
inapplicable checks. It should not be used to warn about subsystems the project
does not use.

## Error model

Doctor failures should be concrete.

Examples:

- `project entrypoint not found: ./app.marreta`
- `missing required project metadata: project_version`
- `incomplete structured db config for provider postgres: missing MARRETA_DB_PASSWORD`
- `db connectivity failed: connection refused`

The goal is that the user can act immediately without guessing what was missing.

## Exit codes

- `0` when all validated checks pass
- `1` when any structural, config, migration, or connectivity error is found
  for an intended subsystem

The command should be usable directly in CI/CD gates.

## Scope

This draft covers:

- project validation
- intent-aware config validation
- intent-aware connectivity validation
- migration summary when applicable

This draft does not cover:

- automatic config repair
- secret discovery from external secret managers
- machine-readable JSON output in v1

## Read-only semantics

`marreta doctor` must be read-only with respect to:

- project files
- migration files
- `_marreta_migrations`
- queue state
- cache contents
- document data
- relational data

`--connect` may open live connections and perform lightweight health-style
operations, but it must not mutate infrastructure state.

## Relationship to 020

This command should build on the configuration model defined in
`020_SECRET_AWARE_CONFIG.md`.

It should not re-implement configuration resolution. It should consume the same
resolved `MarretaConfig` and the same project/intention discovery used by the
runtime.

## Implementation outline

1. add `doctor` command to the CLI
2. resolve project entrypoint the same way as `serve`
3. load the project and required metadata
4. derive project intent from the loaded project model
5. validate effective structured config for intended subsystems
6. add optional live connectivity checks behind `--connect`
7. add migration summary when persistent schemas are present
8. update examples and docs

## Validation plan

### Unit validation

- project with valid metadata passes structure validation
- project missing `project_name` fails
- project missing `project_version` fails
- doctor only reports intended subsystems
- incomplete DB config produces clear DB-specific error
- incomplete cache config produces clear cache-specific error
- project with no queue usage does not warn about queue
- invalid `--connect` target fails with subsystem-specific connectivity error
- exit code is `1` when any intended subsystem fails validation

### Functional validation

#### Phase A: migrations project

In `examples/migrations_functional/`:

- run `marreta doctor`
- confirm DB and migrations are validated
- confirm unrelated subsystems are omitted
- run `marreta doctor --connect`
- confirm DB connectivity passes without mutating migration state

#### Phase B: functional HTTP project

In `examples/functional_tests/`:

- run `marreta doctor`
- confirm DB, doc, cache, and queue appear only if the project actually uses
  them
- confirm the default run does not require live containers beyond project load
- run `marreta doctor --connect`
- confirm connectivity checks pass in the docker environment

#### Phase C: config failure

For at least one project:

- remove a required variable
- run `marreta doctor`
- confirm the output points to the exact missing variable
- confirm the command exits with code `1`

## Acceptance criteria

This plan is complete when:

- `marreta doctor` works from the project root
- it validates project metadata and project loadability
- it reports only subsystems the project intends to use
- it produces clear, subsystem-specific config errors
- `marreta doctor --connect` can validate live connectivity for intended subsystems
- it surfaces migration context when the project uses persistent schemas
- it remains read-only with respect to project and infrastructure state
