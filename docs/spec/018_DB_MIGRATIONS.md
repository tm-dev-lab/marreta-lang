# MarretaLang — DB Migrations & Relational Schema Lifecycle

> Status: Delivered.
> Delivered baseline: Postgres migrations with
> `marreta migrate diff|generate|status|list|explain|discard|apply|rollback`.
> Later hygiene and persistence-by-convention refinements are tracked in
> `018b_MIGRATION_HYGIENE.md` and
> `025_PERSISTENCE_BY_CONVENTION_AND_QUERY_NAVIGATION.md`.

## Overview

MarretaLang already treats relational database access as a first-class part of
the language through `db.*`.

But there is still a structural gap:

- the language abstracts reads and writes
- the language does **not** yet abstract database creation and evolution

That means a user can write:

```marreta
saved = db.users.save(payload)
```

while still needing external SQL knowledge, migration tooling, and provider
details just to create or evolve the `users` table.

That breaks the language's own promise of zero ceremony.

This plan closes that gap by making relational structure part of the Marreta
tooling lifecycle:

- `schema` remains the modeling primitive
- schemas marked with `db:` become persistent relational schemas
- `marreta migrate` generates and applies reviewable migrations
- database evolution becomes explicit and versioned, not magical at server boot

The goal is not to turn MarretaLang into a DBA platform.
The goal is to let a Marreta project define, version, and evolve its own
relational structure without forcing users out of the ecosystem for normal
application work.

---

## Problem

Today, the `db.*` API is intentionally provider-agnostic:

- application code does not mention Postgres directly
- database operations are modeled in MarretaLang
- infrastructure is bound through config

But table lifecycle is still external:

- creating tables
- adding columns
- evolving constraints
- keeping environments in sync

This creates four problems:

1. The zero-ceremony story is incomplete.
   Users still need migration tools or hand-written SQL.

2. The source of truth is duplicated.
   The Marreta schema says one thing; the real DB may say another.

3. Onboarding is inconsistent.
   The language feels simple until the moment the database must be initialized.

4. `db.*` is only half first-class.
   Runtime access is in the language; structural lifecycle is not.

---

## Goal

Enable relational database schema lifecycle inside the Marreta ecosystem with:

- modeling in `schema`
- migration generation by CLI
- reviewable, versioned migration files
- explicit application and rollback
- provider-specific DDL hidden behind the engine

The intended workflow is:

1. developer changes a persistent `schema`
2. `marreta migrate diff` or `generate` computes the change
3. a migration file is written to `migrations/`
4. `marreta migrate apply` applies it and records the version

This keeps modeling simple while preserving control in real environments.

---

## Non-Goals

This plan does **not** introduce:

- automatic destructive sync on server startup
- silent schema drift correction
- arbitrary full-featured DBA automation
- implicit persistence of nested objects with ambiguous storage semantics
- automatic many-to-many / one-to-many relational inference in v1

The design favors correctness and clarity over magical convenience.

---

## Core Decision: One `schema`, Two Roles

MarretaLang should **not** introduce `DbSchema` or a second schema keyword.

There remains a single `schema` concept.

That single concept can operate in two modes:

- **contract schema**: validation / serialization / task contracts only
- **persistent schema**: participates in relational migrations

A schema becomes persistent when it declares `db:`.

Example:

```marreta
schema UserPayload
    name: string
    email: string

schema User
    db: users

    id: integer @primary @generated
    name: string
    email: string @unique
```

### Why this is the right design

- keeps the language surface small
- avoids duplicating concepts
- preserves existing schema semantics
- lets persistence be an opt-in capability, not a separate type family

---

## Persistent Schema Syntax

### Baseline shape

```marreta
schema User
    db: users

    id: integer @primary @generated
    name: string
    email: string @unique
    active: boolean @default(true)
    created_at: timestamp @default(now)
```

### Meaning

- `db: users` marks this schema as relationally persistent and maps it to table `users`
- fields remain normal schema fields
- annotations describe relational intent used by migration tooling

### Metadata vs. contract fields

`db:` is **schema metadata**, not a runtime field.

It must be invisible to:

- request validation
- response serialization
- task contracts
- OpenAPI generation
- user-visible payloads

That means the schema above does **not** gain an API field called `db`.

---

## Initial Relational Field Model

The initial migration-capable field model should support:

- primitive columns
- nullability from existing optional field syntax
- primary key
- generated key
- unique
- default values
- singular foreign keys

### Proposed annotations

Initial v1 set:

- `@primary`
- `@generated`
- `@unique`
- `@default(VALUE)`

Additional v1 limits:

- `@generated` is only supported on `integer` fields marked `@primary`
- `@unique` only creates single-column unique constraints in v1

Example:

```marreta
schema Product
    db: products

    id: integer @primary @generated
    name: string
    sku: string @unique
    price: float
    active: boolean @default(true)
```

### Nullability

Existing schema optional syntax stays meaningful:

```marreta
description?: string
```

For persistent schemas, this means the generated column is nullable.

Required fields mean `NOT NULL`.

---

## Foreign Keys

Foreign keys should be part of the first migration version.

MarretaLang already supports references between schemas.
When those references occur between persistent schemas, the migration system can
infer a simple relational FK.

### Supported v1 case

Only this case is inferred automatically:

- singular field
- target is another persistent schema
- target has a simple primary key

Example:

```marreta
schema Address
    db: addresses

    id: integer @primary @generated
    city: string

schema User
    db: users

    id: integer @primary @generated
    name: string
    address: Address
```

Expected relational interpretation:

- `users.address` does **not** become a nested JSON structure
- it becomes a column like `address_id`
- that column references `addresses(id)`

### v1 FK rules

- singular schema reference to persistent schema => inferred FK
- generated local column name defaults to `<field_name>_id`
- referenced table comes from target schema's `db:`
- referenced column defaults to the target primary key
- required reference field => generated FK column is `NOT NULL`
- optional reference field (`address?: Address`) => generated FK column is nullable
- no automatic cascade behavior in v1
- no composite key support in v1

### Out of scope in v1

These should **not** be inferred automatically yet:

- `list of User`
- many-to-many bridge tables
- one-to-many ownership inference
- polymorphic relations
- cascade strategies

Those need explicit future design.

---

## Invalid Combinations

To avoid silent persistence ambiguity, the first version should reject certain
model shapes at migration time.

### Persistent schema referencing non-persistent schema

This should be an error in v1.

Example:

```marreta
schema AddressPayload
    street: string
    city: string

schema User
    db: users

    id: integer @primary @generated
    name: string
    address: AddressPayload
```

This is ambiguous:

- should it become JSON?
- should it flatten into columns?
- should it become a FK?

None of those should happen silently.

### Rule

If a persistent schema field references a non-persistent schema, migration
generation fails with a descriptive modeling error.

This keeps the system honest and avoids accidental storage semantics.

### Future expansion

If embedded persistence is later desired, it should be explicit, for example via
future annotations such as:

- `@json`
- `@embedded`

But v1 should not guess.

---

## Reuse of Persistent Schemas in API Contracts

A persistent schema may still be used in:

- route payload validation
- response serialization
- task contracts

Example:

```marreta
schema User
    db: users

    id: integer @primary @generated
    name: string
    email: string @unique

route POST "/users" take payload as User
    saved = db.users.save(payload)
    reply 201 as User, saved
```

This should be allowed.

### But it is not always the best modeling choice

Using the same persistent schema for storage and external API contracts may:

- expose DB-shaped fields externally
- over-couple public API to persistence structure
- make future refactors harder

So the language should allow it, but documentation should recommend separate
contract schemas when external shape differs from persistence shape.

### Runtime persistence API remains a separate concern

This migration spec does **not** yet define the runtime payload contract for
relational fields in `db.*` operations such as:

- whether `db.users.save(...)` should accept nested `address`
- whether it should accept only `address_id`
- whether both forms may be accepted

That behavior should be decided explicitly in the runtime persistence API design,
not inferred silently from migration behavior alone.

---

## CLI

### Commands

```bash
marreta migrate diff <file.marreta>
marreta migrate generate <file.marreta>
marreta migrate apply <file.marreta>
marreta migrate status <file.marreta>
marreta migrate rollback <file.marreta>
```

### Intended semantics

- `diff`: show what would change, write nothing
- `generate`: create migration files from current schema vs applied state
- `apply`: apply pending generated migrations
- `status`: show applied and pending versions
- `rollback`: revert the last reversible migration

### Design principle

Generation and application should be separate concerns.

That means:

- no silent production sync
- no implicit destructive apply during normal server startup
- migrations remain explicit operational events

---

## Migration File Strategy

Migration files should be versioned and reviewable.

Suggested directory:

```text
migrations/
  20260412_153001_create_users.up.sql
  20260412_153001_create_users.down.sql
```

### Why SQL files first

For v1, plain generated SQL is the most practical format:

- easy to audit
- easy to diff in git
- easy to inspect when debugging
- aligns with DBA expectations without requiring the user to write SQL by hand

Driver-specific generation is acceptable in the file output.
The language stays provider-agnostic; the generated migration artifact does not
need to pretend all providers are identical.

---

## Migration Application Model

Applied versions should be recorded in a dedicated table:

```text
_marreta_migrations
```

Suggested fields:

- `version`
- `name`
- `applied_at`
- `checksum`

Suggested shape:

```sql
CREATE TABLE _marreta_migrations (
  version TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  checksum TEXT NOT NULL,
  applied_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

### Why both filesystem and DB are needed

Migration state should be tracked in two places with different responsibilities:

- `migrations/` = declared project history
- `_marreta_migrations` = applied history for a specific environment

The filesystem tells Marreta what the project intends.
The database tells Marreta what that environment has already executed.

The CLI compares both.

### Checksums are required

Checksums should not be optional in v1.

Reason:

- a migration may keep the same version and filename
- but its SQL may be edited after being applied somewhere
- that creates silent drift unless content is verified

So the migration engine should compute a checksum for the applied migration
content and store it in `_marreta_migrations`.

If a local file and applied record share the same version but not the same
checksum, this must be treated as a drift/error state.

### Apply flow

1. connect using `MARRETA_DB_*`
2. ensure `_marreta_migrations` exists
3. discover local migration files
4. compare with applied versions
5. apply pending migrations in order
6. record success in `_marreta_migrations`

### Status states

At minimum, `marreta migrate status` should classify migrations into:

- `applied`
- `pending`
- `changed`
- `missing_local`

#### `applied`

- exists in `migrations/`
- exists in `_marreta_migrations`
- checksum matches

#### `pending`

- exists in `migrations/`
- does not exist in `_marreta_migrations`

#### `changed`

- exists in both places
- version matches
- checksum differs

This indicates that an already-applied migration file was edited locally and
must be treated as a serious inconsistency.

#### `missing_local`

- exists in `_marreta_migrations`
- no corresponding local migration file exists anymore

This indicates the environment has history that the current project checkout no
longer contains.

### `status` output goal

Conceptually:

```text
Applied:
  20260410_153001_create_users
  20260411_101500_add_email_to_users

Pending:
  20260412_090000_create_orders

Changed:
  none

Missing local:
  none
```

Exact formatting is implementation-defined.
The important contract is that the system distinguishes clean state from drift.

### Rollback flow

`rollback` should:

1. inspect `_marreta_migrations`
2. find the latest applied reversible migration
3. locate its matching `down` file
4. execute the rollback
5. remove the corresponding row from `_marreta_migrations`

If no valid reversible migration exists, rollback must fail explicitly.

---

## Safety Rules

This feature should preserve explicit operational control.

### Mandatory safety properties

- destructive changes are never applied silently
- `generate` never mutates the database
- irreversible operations are flagged at generation time
- rollback is only offered when a valid down migration exists

### Destructive changes

Examples:

- `DROP TABLE`
- `DROP COLUMN`
- type narrowing
- incompatible type conversion

These should require explicit flags such as:

```bash
marreta migrate generate --allow-destructive
```

or an equivalent confirmation path.

The default path should be conservative.

---

## Driver Scope

### v1

Postgres only.

Reason:

- `db.*` currently supports Postgres only
- relational migrations are already a large feature
- introducing multi-driver DDL generation in the same first version adds too much surface area

### Future

The architecture should be written so additional providers can later implement:

- schema introspection
- type mapping
- SQL generation
- compatibility checks

But the spec should explicitly state:

> v1 migrations target Postgres first; other relational drivers are future work.

---

## Type Mapping

Initial Postgres-oriented mapping:

| Marreta | Postgres |
|---|---|
| `string` | `TEXT` |
| `integer` | `BIGINT` |
| `float` | `DOUBLE PRECISION` |
| `boolean` | `BOOLEAN` |
| `timestamp` | `TIMESTAMPTZ` |

Additional details:

- required field => `NOT NULL`
- optional field => nullable
- `@primary` => primary key
- `@generated` on integer primary key => generated identity / serial-equivalent
- `@unique` => unique constraint
- `@default(...)` => SQL default when representable

Exact SQL spelling may vary by generator implementation.

---

## Diff Semantics

The migration engine should compare:

- persistent schemas in source
- applied migration state / live DB structure

Initial v1 diff operations:

- create table
- add column
- add/remove not null
- add/remove default
- add/remove unique
- create foreign key
- alter/drop existing foreign keys only in future explicit phases
- rename support only if explicitly modeled in future

### Important constraint

Do not infer renames from similarity in v1.

If a field disappears and a new one appears, treat it conservatively as:

- drop + add

unless explicit rename support exists later.

---

## Example

```marreta
schema Address
    db: addresses

    id: integer @primary @generated
    city: string
    street: string

schema User
    db: users

    id: integer @primary @generated
    name: string
    email: string @unique
    active: boolean @default(true)
    address: Address
```

Conceptual generated SQL:

```sql
CREATE TABLE addresses (
  id BIGINT PRIMARY KEY GENERATED BY DEFAULT AS IDENTITY,
  city TEXT NOT NULL,
  street TEXT NOT NULL
);

CREATE TABLE users (
  id BIGINT PRIMARY KEY GENERATED BY DEFAULT AS IDENTITY,
  name TEXT NOT NULL,
  email TEXT NOT NULL UNIQUE,
  active BOOLEAN NOT NULL DEFAULT true,
  address_id BIGINT NOT NULL,
  CONSTRAINT fk_users_address_id
    FOREIGN KEY (address_id)
    REFERENCES addresses(id)
);
```

The exact SQL is implementation-defined.
The semantic contract is what matters.

---

## Open Questions Deferred Beyond v1

- embedded object persistence
- JSON/JSONB column annotations
- one-to-many and many-to-many modeling
- index declarations
- composite primary keys
- cascade options
- rename annotations
- provider-specific type overrides
- SQLite / MySQL / other relational backends

These should not block the first useful migration system.

---

## Acceptance Criteria

1. A schema with `db:` is recognized as persistent and participates in migration generation.
2. A schema without `db:` does not participate in relational migrations.
3. `db:` is treated as metadata only and never appears in payload validation, response serialization, task contracts, or OpenAPI.
4. A singular reference from one persistent schema to another generates a foreign key relation in migration output.
5. A persistent schema referencing a non-persistent schema fails migration generation with a descriptive error.
6. `marreta migrate diff` shows pending relational changes without mutating the DB.
7. `marreta migrate generate` writes versioned migration files.
8. `marreta migrate apply` applies pending migrations and records them in `_marreta_migrations`.
9. Destructive changes require explicit acknowledgment and are never applied silently.
10. v1 works for Postgres without requiring hand-written SQL for common table lifecycle.

---

## Functional Validation

Beyond unit and integration coverage, v0.12 should have a dedicated functional
validation flow for migrations.

This flow should be separate from the main
`examples/functional_tests/test.sh` suite.

Reason:

- the main functional suite validates language/runtime behavior of APIs
- DB migrations introduce an operational lifecycle of their own
- the migration lifecycle must be validated against a real Postgres instance
- the result must also be validated through subsequent `db.*` usage

### Validation workspace

Implemented example:

```text
examples/migrations_functional/
  app.marreta
  docker-compose.yml
  Dockerfile
  marreta.env
  migrations/
  routes/
    users.marreta
  schemas/
    models.marreta
  test.sh
```

This is a normal Marreta project layout. The functional runner mutates
`schemas/models.marreta` from a v1 model to a v2 model during execution, and
all migration commands are invoked with the real project entrypoint:

```bash
marreta migrate <command> examples/migrations_functional/app.marreta
```

### Infrastructure

The functional migration suite should:

- start a clean Postgres instance via `docker compose`
- start the Marreta service via `docker compose` for runtime validation
- use an isolated port/database from other suites
- load DB configuration from the project's `marreta.env`
- allow process environment variables to override `marreta.env` when needed
- create and inspect real migration files
- inspect real database state
- validate HTTP behavior against the compose-managed Marreta service

### Validation phases

#### Phase A — Empty DB -> v1 schema

Initial `schemas/models.marreta`:

```marreta
schema User
    db: users

    id: integer @primary @generated
    name: string
    email: string @unique
```

Checks:

1. `marreta migrate diff`
   - output contains `CREATE TABLE users`
   - output contains primary key and unique column SQL
2. `marreta migrate generate`
   - creates `migrations/*.up.sql`
   - creates `migrations/*.down.sql`
3. generated files contain expected SQL
   - `up.sql` contains `CREATE TABLE users`
   - `down.sql` contains `DROP TABLE users`
4. `marreta migrate status`
   - migration appears as `pending`
5. `marreta migrate apply`
   - succeeds
6. real Postgres verification
   - table `users` exists
   - table `_marreta_migrations` exists
   - one applied migration record exists
   - expected columns and unique constraint exist

Expected evidence:

```bash
$ cd examples/migrations_functional && ../../target/debug/marreta migrate diff
CREATE TABLE users (
  id BIGINT PRIMARY KEY GENERATED BY DEFAULT AS IDENTITY NOT NULL,
  name TEXT NOT NULL,
  email TEXT NOT NULL UNIQUE
);
```

```bash
$ cd examples/migrations_functional && ../../target/debug/marreta migrate generate
Generated migration 20260410_211545_create_users
examples/migrations_functional/migrations/20260410_211545_create_users.up.sql
examples/migrations_functional/migrations/20260410_211545_create_users.down.sql
```

```bash
$ cd examples/migrations_functional && ../../target/debug/marreta migrate status
Applied:
  none

Pending:
  20260410_211545_create_users

Changed:
  none

Missing local:
  none
```

```bash
$ cd examples/migrations_functional && ../../target/debug/marreta migrate apply
Applied 20260410_211545_create_users
```

```sql
SELECT tablename
FROM pg_tables
WHERE schemaname = 'public'
ORDER BY tablename;

-- expected rows include:
-- _marreta_migrations
-- users
```

```sql
SELECT version, name
FROM _marreta_migrations
ORDER BY version;

-- expected:
-- 20260410_211545 | create_users
```

Generated migration file:

```sql
CREATE TABLE users (
  id BIGINT PRIMARY KEY GENERATED BY DEFAULT AS IDENTITY NOT NULL,
  name TEXT NOT NULL,
  email TEXT NOT NULL UNIQUE
);
```

#### Phase B — Runtime usage after apply

After Phase A, start a small Marreta app that uses `db.*` against the migrated
table.

Suggested routes:

- `POST /users`
- `GET /users/:id`

Checks:

1. save a row through `db.users.save`
2. fetch the same row through `db.users.find`
3. confirm the migrated schema is usable by the runtime, not only by raw SQL
4. the Marreta service itself is started by `docker compose`, not manually

Expected evidence:

```bash
$ curl -s -X POST http://127.0.0.1:3738/users \
    -H 'Content-Type: application/json' \
    -d '{"name":"Ana","email":"ana@example.com"}'
{"id":1,"name":"Ana","email":"ana@example.com"}
```

```bash
$ curl -s http://127.0.0.1:3738/users/1
{"id":1,"name":"Ana","email":"ana@example.com"}
```

#### Phase C — v1 -> v2 evolution

Updated `schemas/models.marreta`:

```marreta
schema Address
    db: addresses

    id: integer @primary @generated
    city: string

schema User
    db: users

    id: integer @primary @generated
    name: string
    email: string @unique
    active: boolean @default(true)
    address?: Address
```

Checks:

1. `marreta migrate diff`
   - output contains `CREATE TABLE addresses`
   - output contains `ALTER TABLE users ADD COLUMN active`
   - output contains `ALTER TABLE users ADD COLUMN address_id`
   - output contains `ADD CONSTRAINT fk_users_address_id`
2. `marreta migrate generate`
   - writes a second migration pair
3. `marreta migrate apply`
   - succeeds
4. real Postgres verification
   - table `addresses` exists
   - `users.active` exists
   - `users.address_id` exists
   - foreign key exists
   - `_marreta_migrations` contains two applied records
5. `marreta migrate status`
   - `applied` contains both migrations
   - `pending`, `changed`, and `missing_local` are empty

Expected evidence:

```bash
$ cd examples/migrations_functional && ../../target/debug/marreta migrate diff
CREATE TABLE addresses (
  id BIGINT PRIMARY KEY GENERATED BY DEFAULT AS IDENTITY NOT NULL,
  city TEXT NOT NULL
);

ALTER TABLE users ADD COLUMN active BOOLEAN NOT NULL DEFAULT true;

ALTER TABLE users ADD COLUMN address_id BIGINT;

ALTER TABLE users ADD CONSTRAINT fk_users_address_id
FOREIGN KEY (address_id) REFERENCES addresses(id);
```

```bash
$ cd examples/migrations_functional && ../../target/debug/marreta migrate generate
Generated migration 20260410_211604_update_addresses
examples/migrations_functional/migrations/20260410_211604_update_addresses.up.sql
examples/migrations_functional/migrations/20260410_211604_update_addresses.down.sql
```

```bash
$ cd examples/migrations_functional && ../../target/debug/marreta migrate apply
Applied 20260410_211604_update_addresses
```

```bash
$ cd examples/migrations_functional && ../../target/debug/marreta migrate status
Applied:
  20260410_211545_create_users
  20260410_211604_update_addresses

Pending:
  none

Changed:
  none

Missing local:
  none
```

```sql
SELECT version, name
FROM _marreta_migrations
ORDER BY version;

-- expected:
-- 20260410_211545 | create_users
-- 20260410_211604 | update_addresses
```

```sql
SELECT column_name
FROM information_schema.columns
WHERE table_schema = 'public'
  AND table_name = 'users'
ORDER BY ordinal_position;

-- expected rows include:
-- id
-- name
-- email
-- active
-- address_id
```

Generated migration file:

```sql
CREATE TABLE addresses (
  id BIGINT PRIMARY KEY GENERATED BY DEFAULT AS IDENTITY NOT NULL,
  city TEXT NOT NULL
);

ALTER TABLE users ADD COLUMN active BOOLEAN NOT NULL DEFAULT true;

ALTER TABLE users ADD COLUMN address_id BIGINT;

ALTER TABLE users ADD CONSTRAINT fk_users_address_id FOREIGN KEY (address_id) REFERENCES addresses(id);
```

#### Phase D — Drift detection

After applying migrations, mutate a local migration file and rerun `status`.

Checks:

1. local file content changes while version stays the same
2. `marreta migrate status ...`
   - reports that migration under `changed`
3. `marreta migrate apply ...`
   - must refuse to proceed while drift exists

Expected evidence:

```bash
$ printf '\n-- edited locally\n' >> examples/migrations_functional/migrations/20260410_211604_update_addresses.up.sql
```

```bash
$ cd examples/migrations_functional && ../../target/debug/marreta migrate status
Applied:
  20260410_211545_create_users

Pending:
  none

Changed:
  20260410_211604_update_addresses

Missing local:
  none
```

```bash
$ cd examples/migrations_functional && ../../target/debug/marreta migrate apply
[marreta] runtime_error: migration state is inconsistent
[marreta] detail: resolve changed/missing_local entries before apply
```

#### Phase E — Rollback

Run:

```bash
cd examples/migrations_functional && ../../target/debug/marreta migrate rollback
```

Checks:

1. rollback succeeds
2. `_marreta_migrations` loses the latest applied record
3. objects from the latest migration are removed
   - `addresses` table no longer exists
   - `users.address_id` no longer exists
   - `users.active` no longer exists

Expected evidence:

```bash
$ cd examples/migrations_functional && ../../target/debug/marreta migrate rollback
Rolled back 20260410_211604_update_addresses
```

```sql
SELECT version, name
FROM _marreta_migrations
ORDER BY version;

-- expected:
-- 20260410_211545 | create_users
```

```sql
SELECT tablename
FROM pg_tables
WHERE schemaname = 'public'
ORDER BY tablename;

-- expected rows include:
-- _marreta_migrations
-- users
--
-- expected rows do not include:
-- addresses
```

### Tooling expectations for the functional runner

The dedicated migration runner may use:

- `docker compose`
- `curl`
- `jq`
- `grep`
- `find`
- `psql` directly or through the Postgres container

### What this functional suite must prove

The migration functional suite should prove all of the following in one real
environment:

- create-table generation
- add-column generation
- foreign-key generation
- migration file generation
- `_marreta_migrations` state tracking
- checksum drift detection
- rollback of the latest reversible migration
- compatibility between applied schema and runtime `db.*` operations

This is the minimum functional bar for calling the v0.12 migration baseline
production-shaped.

---

## Implementation Phases

### Phase 1 — Persistent schema modeling

Files:

- `src/ast.rs`
- `src/parser.rs`
- `src/lexer.rs`
- `src/token.rs`

Deliverables:

- `db:` schema metadata in AST
- relational annotations (`@primary`, `@generated`, `@unique`, `@default`)
- parser support and tests

### Phase 2 — Persistent schema extraction

Files:

- `src/file_loader.rs`
- `src/route_loader.rs`
- new migration metadata module

Deliverables:

- extract persistent schemas from project load
- preserve separation between contract-only and persistent schemas
- detect invalid persistent->non-persistent references

### Phase 3 — Postgres introspection + diff

Files:

- new migration engine module(s)
- `src/db/`

Deliverables:

- inspect live Postgres table structure
- compare source schema vs live DB
- produce change plan

### Phase 4 — SQL generation

Files:

- new migration generator module(s)

Deliverables:

- generate `.up.sql` and `.down.sql`
- support FK generation
- flag irreversible or destructive changes

### Phase 5 — CLI + apply/status/rollback

Files:

- `src/main.rs`
- new migration runner module(s)

Deliverables:

- `migrate diff`
- `migrate generate`
- `migrate apply`
- `migrate status`
- `migrate rollback`
- `_marreta_migrations` tracking

### Phase 6 — Tests, docs, examples

Required coverage:

- parser tests for persistent schema syntax
- unit tests for type mapping and diff planning
- tests for persistent/non-persistent reference validation
- FK generation tests
- CLI behavior tests
- functional example project covering generate/apply on Postgres
