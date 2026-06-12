# 073 - Migrate Roundness Pass

> Status: Delivered
> Type: Tooling (migrations) + docs
> Scope: Two surgical fixes to `marreta migrate`, both "the tool must not surprise", neither a
> new feature. (1) The replay that derives schema state for `diff`/`generate` rejects any SQL
> it did not generate, which traps a developer who hand-writes a legitimate migration
> (applied in production, no non-destructive exit). Fix: tolerate provably schema-neutral
> statement classes, improve the error for the rest, add an explicit per-statement escape
> marker. (2) `diff`/`generate` are silent about schema changes the additive-only planner
> does not support (a column type change reports "up to date"). Fix: report the drift, never
> act on it. Everything below was verified live against a real Postgres (probe transcript in
> section 6) on 2026-06-12; reproduce before implementing in case the code moved.

---

## 1. Purpose

The migrate core is solid and was proven live: `apply` runs each migration's SQL plus its
bookkeeping INSERT in one transaction (`src/db/postgres.rs`, `apply_migration`), a
checksum guard refuses `apply` when local files were tampered with or are missing
(`Changed`/`MissingLocal` states), `status`/`list`/`discard`/`explain` give full state
visibility, and `rollback` cleanly reverts the last applied migration. None of that changes
here.

Two gaps undermine it in day-to-day relational work:

**Gap 1, the production trap.** `apply` executes raw SQL (`sqlx::raw_sql`), so a hand-written
migration (an index, a data backfill, a CHECK constraint) applies fine. But the replay that
derives the current schema for `diff` and `generate`
(`apply_local_migration_to_schema`, `src/migrations.rs:803`) accepts only `CREATE TABLE` and
`ALTER TABLE ...` statements and hard-errors on anything else. Live-proven sequence:

1. A migration containing `CREATE INDEX idx_accounts_owner ON accounts (owner);` applies
   cleanly and is recorded.
2. `marreta migrate generate` then fails forever: `unsupported migration statement 'CREATE
   INDEX ...'`.
3. The exits: keeping the file leaves the toolchain broken; deleting the file flips the state
   to `MissingLocal`, which the checksum guard (correctly) blocks; the only clean exit is
   `rollback` plus delete, which is **destructive**: regaining a working `generate` requires
   undoing the change in the database. For an applied production backfill that is not an
   option.

This is sharpened by the inferred-index decision (067): relational indexes have no language
path, and their natural home, a migration file, is exactly what the replay rejects.

**Gap 2, the silent drift.** The planner (`plan_migration`, `src/migrations.rs:232`) is
additive-only by design (correct: destructive auto-migrations are dangerous). But unsupported
changes are silently ignored: changing a field `integer` to `string`, or deleting a field,
makes `migrate diff` print "Database schema is up to date." (live-proven). The developer
ships believing migrations and schemas agree.

## 2. The change

### 2.1 Replay tolerance by safe class (fixes the trap)

The replay tracks a table/column model. A statement that cannot affect tables or columns is
**provably neutral** for that model and safe to skip during derivation. Three tiers:

- **Auto-tolerated (skipped for derivation, by statement prefix):** `CREATE INDEX`,
  `CREATE UNIQUE INDEX`, `DROP INDEX`, `INSERT`, `UPDATE`, `DELETE`, and `WITH` (a CTE
  cannot contain DDL in Postgres, so a `WITH ...` statement is data-only). Case-insensitive
  prefix match on the trimmed statement.
- **Marked escape valve (skipped explicitly):** a statement preceded by a line containing
  exactly `-- marreta: skip-replay` is skipped regardless of its shape. This is the general
  exit for the long tail (`CREATE EXTENSION`, `GRANT`, `SET`, anything schema-neutral the
  prefix list does not know). The marker is per-statement, auditable in the file, and
  documented.
- **Still rejected (correctly):** column-mutating DDL the model cannot derive (`ALTER TABLE
  ... DROP COLUMN`, `ALTER COLUMN ... TYPE`, `DROP TABLE`, renames). Tolerating these would
  make `generate` lie (it would re-add a dropped column). The error message improves to name
  the file, the offending statement, the rule ("the replay derives schema state from
  generated DDL; this statement changes columns in a way it cannot derive"), and the two
  options (rewrite as schema + generated migration, or `skip-replay` if the statement is
  actually schema-neutral).

**The statement splitter must stop being naive.** Today the replay splits `up_sql` on `;`
(`src/migrations.rs:807`), which mis-splits a semicolon inside a string literal. With hand
SQL now first-class, replace it with a minimal splitter aware of single-quoted strings
(`'...'` with `''` escape) and Postgres dollar-quoting (`$tag$ ... $tag$`). It does not need
to understand SQL, only to find statement boundaries safely.

**Scope note:** `apply` and `rollback` are untouched (they already execute raw SQL); the
checksum guard is untouched (it is correct and proved itself in the live probes). Only the
derivation in `apply_local_migration_to_schema` changes.

### 2.2 Drift report in diff and generate (fixes the silence)

When the planner walks a table that exists in both the migration-derived state and the
desired schemas, it currently only looks for missing columns and missing foreign keys. Add a
**report-only** comparison for what it deliberately does not support:

- column present in both but **type differs** (compare the rendered Postgres type),
- column present in both but **nullability differs**,
- column present in history but **absent from the schema** (deleted or renamed field),
- table present in history but **no schema declares it** anymore.

**Correction to the parked spec (the history side carries no type today).** The first two
cases need the migration-derived ("history") column to carry its type and nullability, and it
does not: `DatabaseColumn` is `{ name: String }` only, and the replay
(`apply_create_table_stmt`) keeps just the column name and discards the rendered type and
`NOT NULL`. The rendered DDL does carry them (`balance BIGINT NOT NULL`), so the fix is to
capture them during replay. The implementation outline in section 3 is amended with a fifth
touch point: `DatabaseColumn` gains `rendered_type: Option<String>` and
`nullable: Option<bool>` (Option for the precision-over-recall rule in (d) below), and the
replay learns to populate them in both `CREATE TABLE` and `ADD COLUMN`. The last two drift
cases (removed field, removed table) need only the column name and work without this.

**Capture and comparison rules.** The history type is the token span between the column name
and the first recognized modifier keyword (`PRIMARY KEY`, `GENERATED`, `NOT NULL`, `DEFAULT`,
`UNIQUE`, `REFERENCES`), normalized by whitespace and compared case-insensitively against the
desired column's rendered Postgres type. Four pins keep this best-effort capture from
producing noise (the report's whole value is that it is trustworthy):

- **(a) Paren-aware column-def split.** The replay parses the `CREATE TABLE` body into column
  definitions; with hand-written `CREATE TABLE` now first-class (2.1), a single-line body with
  a `NUMERIC(10, 2)` breaks any naive comma split. The column-def split must be parenthesis
  aware, the same pattern as the string/dollar-quote-safe statement splitter built in 2.1
  (build them together). Documenting "one column per line" as the only supported shape is
  rejected: a non-obvious limitation in hand SQL is exactly the surprise this spec exists to
  remove.
- **(b) Alias normalization, capped to our rendered set.** Pure textual comparison would flag
  `INT8` vs `BIGINT` or `TIMESTAMPTZ` vs `TIMESTAMP WITH TIME ZONE` as drift, a false positive
  that corrodes trust in the report. Normalize through a small Postgres-alias table covering
  only the types `postgres_type()` emits (`int8` to `bigint`, `float8` to `double precision`,
  `bool` to `boolean`, `timestamptz` to `timestamp with time zone`, and the rest of the
  Marreta set). A type outside that set compares textually, best-effort. Nothing more, this is
  not a SQL parser.
- **(c) `PRIMARY KEY` implies `NOT NULL`.** In Postgres a hand-written `id BIGINT PRIMARY KEY`
  is non-null in fact, but a presence scan for `NOT NULL` would miss it and flag nullability
  drift against our generated DDL (which states `NOT NULL` explicitly). Rule: presence of
  `PRIMARY KEY` implies `nullable = false`.
- **(d) The declared bias: precision over recall.** When type capture fails (odd DDL the span
  does not resolve), record `None` and silently skip the type-drift check for that column. A
  missed drift is the status quo (today every drift is missed); a false drift is new noise
  that kills the feature. In a report-only surface, erring quietly beats erring loudly. This is
  a decision, recorded so the implementation is not "improved" the wrong way later.

These never produce operations. They produce a drift report printed by `migrate diff` and
`migrate generate` (and `generate` still writes only the supported ops), in the doctor
report style:

```
Unsupported changes detected (migrations are additive-only, handle manually):
  accounts.balance: type differs (history BIGINT, schema TEXT)
  accounts.owner: present in history, no longer in any schema
```

Implementation shape: `plan_migration` gains a sibling (or a return struct) that carries the
drift entries alongside the ops, so `run_migrate_diff` (`src/main.rs:1545`) and
`run_migrate_generate` (`src/main.rs:1586`) print them. Exit code stays success (it is a
report, not a failure); `--strict`-style behavior can be a follow-up if ever asked for.

### 2.3 Small roundness items (same pass)

- **Same-second generate collision:** the version is a second-resolution timestamp; two
  `generate` runs inside one second must not silently collide. Add a collision check in
  `write_migration_files` (`src/migrations.rs:317`) (error or bump-a-second, decide in
  implementation) plus a test.
- **Docs, file-precise** (verified against the guide on 2026-06-12; two earlier analysis
  claims corrected: `migrate explain` and the rollback scope are **already documented** in
  the how-to, sections "Preview and explain a state" and "Undo the last migration", so
  neither needs new coverage):
  - **`docs/guide/how-to/migrations.md`**: the "Review before you apply" section gains the
    hand-written-SQL contract (the safe classes that derivation tolerates, the
    `-- marreta: skip-replay` marker, and what stays rejected and why); the "Evolve a
    schema" section gains the drift report (what `diff`/`generate` print for an unsupported
    change, with the additive-only rationale); "Troubleshooting" gains the improved
    rejected-DDL error and the recovery guidance for the trap; and the "Discard a pending
    migration" wording ("rather than editing it by hand") is aligned so hand-written SQL
    reads as a documented capability with rules, not a discouraged act.
  - **`docs/guide/reference/cli.md`** (the `marreta migrate` subcommand table, line ~30):
    the `diff`/`generate` row descriptions mention the drift report.
  - **Verify-only, expected no change**: `tutorials/relational-api-with-migrations.md`,
    `how-to/use-local-services.md`, and `reference/namespaces/db.md` show happy-path
    `migrate` output, which this spec does not alter (the drift block prints only when
    drift exists); confirm at implementation that no shown output changed.
  - No new page, so no `SUMMARY.md` change.

## 3. Implementation outline

- `src/migrations.rs`: the three-tier replay in `apply_local_migration_to_schema` (:803),
  the safe statement splitter (replacing the `split(';')` at :807) plus the parenthesis-aware
  column-def split it shares (pin (a)), the drift comparison beside `plan_migration` (:232),
  the collision check in `write_migration_files` (:317).
- `src/migrations.rs` (fifth touch point, per the 2.2 correction): `DatabaseColumn` gains
  `rendered_type: Option<String>` and `nullable: Option<bool>`; `apply_create_table_stmt` and
  the `ADD COLUMN` path in `apply_alter_table_stmt` populate them with the captured type (span
  to first modifier keyword) and nullability (presence of `NOT NULL`, or `PRIMARY KEY` per pin
  (c)); the drift comparison normalizes through the Postgres-alias table (pin (b)) and skips a
  column whose `rendered_type` is `None` (pin (d)).
- `src/main.rs`: print the drift report in `run_migrate_diff` (:1545) and
  `run_migrate_generate` (:1586).
- `src/db/postgres.rs`: untouched (apply/rollback/guard already correct).
- Docs: `docs/guide` migrations how-to/reference updates per 2.3 (docs DoD, same change).

### Test requirements (house standards)

- **Unit tests, positive and negative, per tier and per drift case:** each auto-tolerated
  prefix is skipped and derivation still matches (index + insert + update + delete + with);
  the `skip-replay` marker skips an arbitrary statement; each rejected statement
  (`DROP COLUMN`, `ALTER COLUMN TYPE`, `DROP TABLE`) still errors and the message names file,
  statement, rule, and options; the splitter handles a semicolon inside `'...'` and inside
  `$tag$...$tag$`; drift report cases: type change, nullability change, removed field,
  removed table, and the clean case reports nothing; generate-collision test.
- **Unit tests for the four 2.2 pins:** (a) a single-line hand-written `CREATE TABLE` with a
  `NUMERIC(10, 2)` column splits into the right defs and captures `numeric(10, 2)` (the
  paren-aware split does not break on the inner comma); (b) an `int8` / `timestamptz` history
  column does not drift against a `bigint` / `timestamp with time zone` desired column (alias
  normalization), while a genuine `bigint` vs `text` still drifts; (c) a hand-written
  `id BIGINT PRIMARY KEY` history column reports no nullability drift against our generated
  `NOT NULL` (PRIMARY KEY implies non-null); (d) a column whose type span does not resolve
  records `None` and produces no type-drift entry (silent skip, precision over recall).
- **Functional coverage of the new behavior, end-to-end** (house rule: exercise the new
  behavior through the real surface): extend `migrations_functional` with the live probe
  script from section 6 turned into assertions: a hand-written `CREATE INDEX` migration
  applies against real Postgres **and `generate` keeps working afterwards** (the trap, made
  green); a tampered applied migration is still refused (no regression on the checksum
  guard); a schema type change makes `diff` print the drift block while still writing no
  migration. `migrations_functional` is the end-to-end tier for this spec; record that in
  the delivery notes.

### Coverage analysis (spec protocol)

- **VS Code extension: no change, verified.** Migrate has no editor surface (the extension's
  palette commands are Serve/Test/Doctor/Init/Format; no migrate command, no diagnostics
  sourced from migrate). Nothing to update.
- **e2e:** none (no language behavior changes; e2e has no relational provider).
- **Documentation:** the migrations guide updates in 2.3.

## 4. Out of scope

- Generating destructive operations (`DROP COLUMN`, type changes, renames). Additive-only is
  the right design; this spec makes its boundary visible, not wider.
- Migration squashing, multi-step or targeted rollback, out-of-order apply policies.
- Any second relational provider concern (the renderer stays Postgres).
- A `--strict` mode that turns the drift report into a failure (follow-up if asked).

## 5. Acceptance criteria

1. A migration containing hand-written safe-class SQL (`CREATE INDEX`, `INSERT`, `UPDATE`,
   `DELETE`, `WITH ...`) applies AND leaves `diff`/`generate` fully working (the live trap
   sequence from section 6 reproduces green end-to-end in `migrations_functional`).
2. `-- marreta: skip-replay` skips the following statement in derivation, documented and
   tested.
3. Column-mutating DDL in a migration file still fails derivation, with the improved error
   (file, statement, rule, options).
4. The splitter is string-safe and dollar-quote-safe (unit-tested).
5. A type change, nullability change, removed field, or removed table against migration
   history makes `diff` and `generate` print the drift block; no operation is generated for
   them; a clean project prints no drift block. The probe "balance integer to string reports
   up to date" now reports drift instead.
6. Two `generate` runs in the same second cannot silently collide.
7. Docs updated per 2.3: `how-to/migrations.md` carries the safe-class contract and marker
   (Review section), the drift report (Evolve section), the rejected-DDL error and trap
   recovery (Troubleshooting), and the aligned Discard wording; `reference/cli.md`'s
   `diff`/`generate` rows mention the drift report; the three happy-path pages confirmed
   unchanged; no `SUMMARY.md` change (no new page).
8. Standard gates: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, full
   test suite, plus `migrations_functional` (the end-to-end tier for this change) and
   `functional_tests` (the change touches the loader-adjacent migrate path consumed by the
   CLI).

## 6. Live probe transcript (reproduce before implementing)

Validated 2026-06-12 against real Postgres (`marreta init miglive --with db`,
`docker compose up -d --wait`):

```bash
# foundation: generate -> apply -> status (clean cycle)
printf 'export schema Account\n    db: accounts\n    id: integer\n    owner: string\n    balance: integer\n' > schemas/account.marreta
marreta migrate generate     # Generated migration: ..._create_accounts
marreta migrate apply        # Applied ..._create_accounts
marreta migrate status       # Applied: 1, Pending: none

# checksum guard (must keep working after this spec)
echo "-- tampered" >> migrations/..._create_accounts.up.sql
marreta migrate apply        # refused: "migration state is inconsistent", status shows Changed
# restore file -> apply clean again

# the trap (gap 1)
printf 'CREATE INDEX idx_accounts_owner ON accounts (owner);\n' > migrations/..._hand_index.up.sql
printf 'DROP INDEX idx_accounts_owner;\n' > migrations/..._hand_index.down.sql
marreta migrate apply        # Applied ..._hand_index  (raw SQL applies fine)
marreta migrate generate     # FAILS: unsupported migration statement 'CREATE INDEX ...'
rm migrations/..._hand_index.*
marreta migrate status       # MissingLocal -> apply blocked; only exit is rollback + delete

# the silence (gap 2, works offline too)
# change balance: integer -> string in the schema
marreta migrate diff         # "Database schema is up to date."  (silent drift)
```

---

## Delivery notes

Delivered. All gates green (`cargo fmt --check`, `clippy -D warnings`, full suite, and the runtime
tier: `functional_tests`, `migrations_functional`, e2e), validated live against real Postgres.

What landed:

- **2.1 replay tolerance** (`apply_local_migration_to_schema`, `src/migrations.rs`): three tiers
  (auto-tolerate `CREATE/DROP INDEX`, `INSERT/UPDATE/DELETE`, `WITH`; the `-- marreta: skip-replay`
  marker; still-rejected column-mutating DDL with an actionable error naming the file, statement,
  rule, and two options). The statement splitter is string-, dollar-quote-, and comment-safe, and a
  parenthesis-aware column-def splitter replaced the per-line parse.
- **2.2 drift report**: `DatabaseColumn` gained `rendered_type` and `nullable` (both `Option`, the
  precision-over-recall bias); `detect_schema_drift` (a sibling of `plan_migration`, no signature
  change) reports type changes, nullability changes, removed fields, and removed tables without ever
  producing an operation; `diff`/`generate` print the doctor-style block. The four pins are in:
  parenthesis-aware capture, alias normalization capped to the `postgres_type()` set, `PRIMARY KEY`
  implies `NOT NULL` (mirrored on the desired side), and an unresolved type is skipped silently.
- **2.3 collision guard**: `write_migration_files` refuses a same-second version collision rather
  than clobbering.
- **Docs**: the migrations how-to (hand-written SQL contract, the drift report, troubleshooting, the
  aligned discard wording) and `cli.md`'s `diff`/`generate` rows.

The review caught a third day-to-day gap the original live probes missed: the probes used clean SQL
(`CREATE INDEX idx ON t (c)`) with no comments or apostrophes, so two compounded bugs were
uncovered. An apostrophe inside a `--` comment opened the string scanner and swallowed the next `;`,
and a trailing comment leaked into the column-def parse and corrupted the type capture. Both bite
the headline case (a hand-written `CREATE TABLE` with a comment). Fixed by making the splitter and
the comment strip string-safe and comment-aware. The lesson for the next spec: a feature whose
headline is "hand-written SQL is first-class" needs tests with what real hand-written SQL contains
(comments, apostrophes, type aliases), not only the clean path.

---

## P.S. Do not forget the docs of record

On delivery in marreta-lang, update both `CHANGELOG.md` and `docs/spec/SPEC.md` (see SPEC.md
section 1.3), and renumber this file into `docs/spec/` with the next free number.
