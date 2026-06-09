# v0.12b — Migration Hygiene and State Guidance

> Status: Delivered.

## Motivation

`018_DB_MIGRATIONS.md` delivered the migration engine:

- persistent `schema` metadata
- SQL diffing and generation
- `_marreta_migrations` tracking
- `diff`, `generate`, `status`, `apply`, `rollback`

But one important operational gap remains:

- what to do with a migration that is still `pending`
- what to do after a `rollback`
- how to recover from `changed`
- how to recover from `missing_local`

Today the system exposes these states correctly, but the developer still has to
infer the right workflow manually.

Marreta should not only execute migrations; it should guide the developer
through the migration state machine safely.

## Goal

Add explicit migration hygiene tooling and guidance so that developers can:

- understand every migration state
- safely discard pending migrations they no longer want
- recover from inconsistent local/applied history
- learn the intended migration workflow directly from the CLI

## Scope

This plan adds four things:

1. `marreta migrate discard`
2. `marreta migrate explain`
3. `marreta migrate list`
4. richer migration workflow guidance in `status` and docs

## Core concepts

### Migration states

The current migration system already exposes four important states:

- `applied`
- `pending`
- `changed`
- `missing_local`

`018b` makes these states first-class in the CLI UX.

### State meanings

#### Applied

The migration exists locally and is recorded in `_marreta_migrations`.

This is the healthy steady state.

#### Pending

The migration exists locally in `migrations/`, but has not been applied to the
current database.

This commonly happens when:

- a new migration was generated
- the latest migration was rolled back

#### Changed

The migration was applied to the current database, but the local file checksum
does not match the checksum stored in `_marreta_migrations`.

This means the applied history and local history diverged.

#### Missing local

The migration was applied to the current database, but the local file is no
longer present.

This means the project checkout is missing part of the migration history.

## Proposed commands

### `marreta migrate explain`

Show the migration state machine and explain what each state means.

Examples:

```bash
marreta migrate explain
marreta migrate explain workflow
marreta migrate explain pending
marreta migrate explain applied
marreta migrate explain changed
marreta migrate explain missing_local
```

### `marreta migrate discard <version>`

Discard a local migration that has not been applied.

Example:

```bash
marreta migrate discard 20260410_215746
```

`<version>` means the numeric migration version only.

Valid:

```text
20260410_215746
```

Invalid:

```text
20260410_215746_alter_addresses
```

This removes:

- `<version>_name.up.sql`
- `<version>_name.down.sql`

Only for migrations that are still `pending`.

### `marreta migrate list`

List project migrations in a table, showing version, name, and current state.

Example:

```bash
marreta migrate list
```

Example output:

```text
VERSION          NAME              STATE
20260410_215007  create_users      applied
20260410_215049  update_addresses  applied
20260410_215746  alter_addresses   pending
```

Another example:

```text
VERSION          NAME              STATE
20260410_215007  create_users      applied
20260410_215049  update_addresses  changed
20260410_215746  alter_addresses   missing_local
```

`list` is intended as the inventory view of the migration system.

It complements, but does not replace, `status`.

## Command semantics

### `migrate discard`

#### Allowed

`discard` is allowed only when the target migration is:

- present locally
- not applied
- not changed
- not missing_local

#### Rejected cases

`discard` must fail when:

- the migration is already applied
- the migration is in `changed`
- the migration is in `missing_local`
- the version does not exist locally

#### Important behavior

Discarding a pending migration does not change the schema model.

If the `.marreta` schema still declares the same structural change, a future:

```bash
marreta migrate diff ...
marreta migrate generate ...
```

will produce a new pending migration again.

So the intended discard workflow is:

1. revert the schema change if you no longer want it
2. discard the pending migration file

### `migrate list`

`list` should:

- combine local migration files with applied migration records from `_marreta_migrations`
- sort rows by version ascending
- compute a single state per migration
- print a stable, human-readable table

Suggested states:

- `applied`
- `pending`
- `changed`
- `missing_local`

Suggested state resolution:

- local exists, applied exists, checksum matches => `applied`
- local exists, applied does not exist => `pending`
- local exists, applied exists, checksum differs => `changed`
- local does not exist, applied exists => `missing_local`

When `missing_local` occurs, `name` should still be shown from the applied
record in `_marreta_migrations`.

Optional future expansion:

- `applied_at` column
- machine-readable output such as `--json`

## CLI help design

### `marreta migrate explain`

Base output should describe the state machine:

```text
Migration states:

Applied:
  Migration exists locally and is recorded in _marreta_migrations.

Pending:
  Migration exists locally but has not been applied.
  Typical actions:
    - apply it
    - discard it
    - or revert the schema change and discard it

Changed:
  Migration was applied, but the local file checksum differs.
  Typical action:
    - restore the original migration file

Missing local:
  Migration was applied, but the local file is missing.
  Typical action:
    - restore the missing migration file from version control
```

### `marreta migrate explain pending`

Example output:

```text
State: pending

Meaning:
  The migration exists locally in migrations/, but has not been applied to this database.

Common causes:
  - you just generated it
  - you rolled it back

Recommended actions:
  - apply it:
      marreta migrate apply
  - discard it:
      marreta migrate discard <version>
  - or revert the schema change and then discard it
```

### `marreta migrate explain applied`

Example output:

```text
State: applied

Meaning:
  The migration exists locally and matches the applied record in _marreta_migrations.

Recommended actions:
  - continue normally
  - rollback the latest applied migration if you need to revert it
```

### `marreta migrate explain changed`

Example output:

```text
State: changed

Meaning:
  The migration was already applied, but the local file content no longer matches
  the checksum stored in _marreta_migrations.

Common causes:
  - someone edited an applied migration file
  - the local branch diverged from the applied history

Recommended actions:
  - restore the original migration file from version control
  - do not edit applied migrations
```

### `marreta migrate explain missing_local`

Example output:

```text
State: missing_local

Meaning:
  The migration was applied to this database, but the local file is missing.

Common causes:
  - a migration file was deleted locally
  - the local checkout is incomplete

Recommended actions:
  - restore the missing migration files from version control
  - do not apply or rollback further until the history is complete again
```

## Status output improvements

`marreta migrate status` should remain concise, but when it detects non-empty:

- `pending`
- `changed`
- `missing_local`

it should also print short suggested actions.

### Example: pending

```text
Pending:
  20260410_215746_alter_addresses

Suggested actions:
  - apply:   marreta migrate apply
  - discard: marreta migrate discard 20260410_215746
```

### Example: changed

```text
Changed:
  20260410_215049_update_addresses

Suggested actions:
  - restore the original migration file from version control
  - do not edit applied migrations
  - run: marreta migrate explain changed
```

### Example: missing_local

```text
Missing local:
  20260410_215049_update_addresses

Suggested actions:
  - restore the missing migration files from version control
  - run: marreta migrate explain missing_local
```

## `list` vs `status`

These commands should serve different purposes:

### `migrate list`

Use when the developer wants a complete inventory of migrations and their
current state.

It should be tabular and descriptive.

### `migrate status`

Use when the developer wants actionable information about what to do next.

It should remain grouped by state and may include suggested actions.

Suggested actions are human guidance, not a stable machine-readable output
contract.

Recommended mental model:

- `list` = "show me all migrations"
- `status` = "tell me what needs attention"

## Proposed workflow

### Normal forward workflow

1. change `.marreta` schema
2. run `migrate diff`
3. run `migrate generate`
4. inspect generated SQL
5. run `migrate apply`
6. verify with `migrate status`

State transition:

```text
no migration -> pending -> applied
```

### Rollback workflow

1. run `migrate rollback`
2. migration moves from `applied` back to `pending`
3. choose one of:
   - reapply it
   - discard it
   - modify schema and generate a replacement migration

State transition:

```text
applied -> pending
```

### Discard workflow

1. revert the schema change if it is no longer desired
2. run `migrate discard <version> app.marreta`
3. verify with `migrate status`

State transition:

```text
pending -> discarded
```

### Recovery workflow: changed

1. detect with `migrate status`
2. run `migrate explain changed`
3. restore the original file from version control
4. rerun `migrate status`

State transition:

```text
applied -> changed -> applied
```

### Recovery workflow: missing_local

1. detect with `migrate status`
2. run `migrate explain missing_local`
3. restore the missing files from version control
4. rerun `migrate status`

State transition:

```text
applied -> missing_local -> applied
```

## Operational rules

These rules should be explicit in both docs and CLI help:

- applied migrations are immutable
- applied migrations must not be deleted from the project
- pending migrations may be discarded
- rollback does not delete local migration files
- if a discarded change is still present in the schema model, it will be generated again
- `changed` and `missing_local` are inconsistent states and should block unsafe operations

## Implementation scope

### Included

- `migrate discard <version> <file.marreta>`
- `migrate explain`
- `migrate explain <state>`
- `migrate list <file.marreta>`
- better `status` guidance text
- documentation for the migration state machine
- tests for discard/explain/state guidance

### Explicitly out of scope

- editing applied migrations
- auto-repair for `changed`
- auto-repair for `missing_local`
- squashing or rebasing migration history
- destructive pruning of applied migration history

## Validation

Implementation should validate at least:

1. `discard` removes only pending migrations
2. `discard` fails for applied migrations
3. `discard` fails for unknown versions
4. `list` prints a complete migration table with version, name, and state
5. `explain` prints workflow guidance
6. `explain pending|applied|changed|missing_local` prints state-specific help
7. `status` shows suggested actions for non-empty problematic groups
8. rollback followed by discard behaves as expected for the latest migration

## Functional validation

As with `018_DB_MIGRATIONS.md`, validation should happen at two levels:

- unit-level verification of state computation and command semantics
- functional validation against a real project and real Postgres state

The existing example project remains the right validation workspace:

```text
examples/migrations_functional/
  app.marreta
  marreta.env
  docker-compose.yml
  migrations/
  routes/
  schemas/
  test.sh
```

The `018b` validation should extend this workspace rather than inventing a new
fixture hierarchy.

### Unit validation

The implementation should add focused tests for:

1. state resolution for `list`
   - local + applied + matching checksum => `applied`
   - local only => `pending`
   - local + applied + checksum mismatch => `changed`
   - applied only => `missing_local`
2. `discard`
   - removes a valid pending pair
   - fails if version is not found
   - fails if only one of the local pair files exists
   - fails if the target is already applied
   - fails if the target is `changed`
   - fails if the target is `missing_local`
3. `explain`
   - base help renders successfully
   - `workflow` renders successfully
   - each state-specific explain target renders successfully
   - unknown state returns a clear usage or error message
4. `status`
   - suggested actions appear only for non-empty groups
   - suggested actions match the relevant state

### Functional validation phases

The functional runner for `018b` should prove four workflows:

#### Phase A — Inventory view

Starting from the existing two-migration project:

1. `marreta migrate list`
2. validate that both migrations appear in a table
3. validate that both are `applied` after `migrate apply`

Expected evidence:

```text
VERSION          NAME              STATE
20260410_215007  create_users      applied
20260410_215049  update_addresses  applied
```

#### Phase B — Pending after generate

After changing the schema and generating a new migration:

1. `marreta migrate list`
2. validate that the new migration appears as `pending`
3. `marreta migrate explain pending`
4. validate that the output recommends `apply` and `discard`

Expected evidence:

```text
VERSION          NAME               STATE
20260410_215007  create_users       applied
20260410_215049  update_addresses   applied
20260410_215746  alter_addresses    pending
```

#### Phase C — Pending after rollback

After applying the latest migration and then rolling it back:

1. `marreta migrate rollback`
2. `marreta migrate list`
3. validate that the rolled-back migration returns to `pending`
4. `marreta migrate explain workflow`
5. validate that rollback -> pending is described

Expected evidence:

```text
Rolled back 20260410_215746_alter_addresses
```

and then:

```text
VERSION          NAME               STATE
20260410_215007  create_users       applied
20260410_215049  update_addresses   applied
20260410_215746  alter_addresses    pending
```

#### Phase D — Discard pending migration

After a migration is `pending`:

1. validate that the target migration is `pending`
2. `marreta migrate discard 20260410_215746`
3. validate that the `.up.sql` and `.down.sql` files are removed together
4. `marreta migrate list`
5. validate that the discarded migration no longer appears locally
6. rerun `migrate diff`
7. confirm the schema change is still proposed if the schema was not reverted

Expected evidence:

```text
Discarded 20260410_215746_alter_addresses
```

and then:

```text
VERSION          NAME              STATE
20260410_215007  create_users      applied
20260410_215049  update_addresses  applied
```

and later:

```text
ALTER TABLE addresses ADD COLUMN street TEXT NOT NULL;
```

This final diff is important. It proves the intended rule:

- discard removes the pending migration file
- discard does not alter the schema model
- if the schema still asks for the change, the migration can be regenerated

### Optional inconsistency validation

If included in the functional runner, the suite may also exercise:

- `changed` by editing an applied migration file
- `missing_local` by temporarily moving an applied migration file away

The minimal validation for those states is:

1. `marreta migrate list`
2. `marreta migrate status`
3. `marreta migrate explain changed|missing_local`

The goal is not to auto-repair them, only to prove that the CLI explains them
correctly and surfaces the right state.

### Documentation evidence

As in `018`, the plan should include example outputs directly in the spec, not
just references to artifacts on disk.

At minimum, the final implementation should document:

- `list` output in a clean state
- `list` output with a `pending` migration
- `explain pending`
- `explain changed`
- `explain missing_local`
- `discard` success output
- `status` with suggested actions

## Acceptance criteria

`018b` is complete when:

1. developers can discard unwanted pending migrations safely
2. developers can list all migrations and their current state in one table
3. every migration state has first-class CLI help
4. `status` becomes actionable, not just descriptive
5. the migration workflow is documented as an explicit state machine
6. recovery guidance for `changed` and `missing_local` is built into the tool

## Notes

This plan does not redesign `rollback`.

`rollback` remains part of the migration flow introduced in `018`. `018b`
extends the operational guidance around the states that `rollback` and other
commands can produce.
