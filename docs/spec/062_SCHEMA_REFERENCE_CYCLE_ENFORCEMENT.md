# 062 - Schema Reference Cycle Enforcement (Validator + Loader)

> Status: Delivered
> Type: Validator / loader
> Scope: Make the payload validator relation-aware so a persistent (`db:`) schema works
> as an API contract even inside a relation cycle (the relation edge is let through, not
> recursed), and re-establish circular schema-reference detection on the live load path
> (currently dead) for the only remaining loop — an all-value-schema cycle. One shared
> relation-aware cycle rule governs the loader and `marreta lint`. No new syntax.

---

## 1. Purpose

Spec 006 (`006_ADVANCED_SCHEMAS_040`) introduced circular schema-reference detection:
a **value/validation** schema that references itself transitively (A references B, B
references A) makes the recursive validator loop forever, so the loader rejects it at
startup with a config error (`007_ERROR_HANDLING` lists `CircularSchemaReference`).

Two things have drifted since:

1. **The runtime check is dead on the live path.** The detection lives in
   `route_loader::load` (`detect_circular_references`), but the universal multi-file
   loader `file_loader::load_project` (used by `serve`, `test`, `doctor`) builds the
   `RouteRegistry` itself and never calls `route_loader::load`. That function now has
   **zero production callers** — only `#[cfg(test)]` tests in `route_loader.rs` exercise
   it. Demonstrated: a single-file project with a value cycle `A -> B -> A` passes
   `marreta doctor` with no error. So the Spec 006 rule is no longer enforced at load.

2. **The only surviving enforcer is `marreta lint`**, which has its own independent
   cycle check (`lint_schema_cycles`). Spec 061 made that check relation-aware by
   allowing all-persistent cycles (Spec 025 infers foreign keys from singular references
   and inverse collections from `list of <persistent schema>`). That was a first step,
   but its rule ("a cycle touching a value schema errors") is refined below — once the
   validator lets persistent references pass, only an all-value cycle can loop.

The result is an inconsistency: the lint enforces the rule (relation-aware), the runtime
does not enforce it at all, and the rule is implemented twice.

There is also a deeper gap the load check alone cannot fix: **persistent schemas are
valid API contracts** (a deliberate language design — one schema serves storage and
contract), and the payload validator recurses into **every** reference, persistent or
not. So `take payload as User` where `User <-> Order` is a relation cycle drives the
validator into that cycle (bounded only by a depth guard). Rejecting such schemas at
load is not acceptable — it would block a supported feature. The fix must make the
*validator* terminate on relations, not ban the schema.

## 2. Background / evidence

- `src/route_loader.rs` — `pub fn load(...)` (the only caller of
  `detect_circular_references`) is called only from tests in the same file.
- `src/file_loader.rs` — `load_project` assembles `all_project_schemas:
  HashMap<String, SchemaDefinition>` (public + private across all files) and runs the
  other load validations on it (`validate_schema_naming`,
  `validate_persistent_schema_references`) right before building the runtime, but never
  runs a circular check.
- `src/lint.rs` — `lint_schema_cycles` is the live enforcement, relation-aware after
  Spec 061 (persistent-only cycles allowed; value/mixed cycles error).
- `SchemaDefinition { db_table: Option<String>, fields }` — a schema is persistent iff
  `db_table.is_some()`.
- The payload validator (`validator.rs`) recurses into every `SchemaType::Reference`,
  guarded only by a fixed `MAX_DEPTH`. Spec 025 says a reference to a **persistent**
  schema is a **foreign-key relation** (singular ref) or an **inverse collection**
  (`list of <persistent>`), not an embedded value — so it should not be recursively
  embed-validated at all.

Two consequences define the fix:

- **The validator should not recurse into persistent references.** A reference whose
  target is a `db:` schema is a relation: let the value pass (it is an FK, not a nested
  object to validate). This makes a persistent schema usable as a contract even in a
  relation cycle — the relation edge is let through and validation terminates.
- **Once persistent references are let through, the only cycle the validator can loop on
  is one whose every node is a value schema** (every edge targets a value schema). So the
  load-time / lint rule is: **a cycle is disallowed iff it lies entirely within value
  schemas.** Any cycle passing through a persistent schema is broken at that relation
  edge and is safe — including a value schema that *references into* a relation cycle
  (`Profile -> DbUser <-> DbOrder`): validating `Profile` lets the `DbUser` relation pass.

## 3. Decision

1. **Make the validator relation-aware** (cycle-aware as a backstop). A reference to a
   persistent schema is let through without recursing; a value-schema reference is
   validated recursively, and a value reference already on the validation stack is
   reported as an infinite cycle. This keeps persistent schemas — even cyclic relational
   ones — working as API contracts (a deliberate language design; not negotiable).
2. **Re-wire the load-time check** for the only remaining loop — an all-value-schema
   cycle — failing `serve`/`test`/`doctor` with `CircularSchemaReference`, the early,
   clear config error (Spec 006). One shared relation-aware helper backs both the loader
   and `marreta lint`.

## 4. Implementation outline

- **Validator** (`src/validator.rs`): thread the set of schema names on the validation
  stack through `coerce_recursive`/`coerce_field_type`. In the `Reference` arm: if the
  target is persistent (`db_table.is_some()`) return the value as-is (relation, let
  pass); else if the target is already on the stack return an infinite-cycle error; else
  recurse as today. `TypedList(Reference(persistent))` elements are let through the same
  way. `MAX_DEPTH` stays as a final backstop.
- **Shared cycle helper** (`src/schema_cycle.rs`): one pure function
  `find_disallowed_cycle(refs, persistent) -> Option<Vec<String>>` that searches for a
  cycle lying entirely within value schemas — DFS from value schemas, cutting edges into
  persistent schemas (mirroring the validator's let-pass). Returns the cycle path or
  `None`.
- **Loader call site** (`file_loader::load_project`): after
  `validate_schema_naming(&all_project_schemas)`, build `refs`/`persistent` from
  `all_project_schemas` and call the helper; on a cycle return
  `MarretaError::CircularSchemaReference`.
- **Lint** (`lint_schema_cycles`): build `refs`/`persistent` from the parsed AST and call
  the same helper; its private DFS is removed.
- **Dead code**: `route_loader::detect_circular_references` (+ `dfs_schema` /
  `collect_schema_refs`) and its test-only call path are removed. Auditing the rest of
  `route_loader::load` is **out of scope**.

## 5. Risk / impact

- **No repo project regresses.** A sweep of every project (`docs/examples/*`,
  `docs/benchmarks/*`, `e2e/`) finds **no all-value cycle**, and no value schema embeds a
  persistent one in a way that changes behavior (no value schema references the `Db*`
  schemas). The all-persistent `DbUser <-> DbOrder` in `functional_tests` stays allowed.
- **Validator behavior change (intended):** a reference to a persistent schema is no
  longer embed-validated; it is accepted as a relation (FK). No repo schema relies on
  recursively validating a persistent reference, so nothing regresses; `take payload as
  <persistent>`, even cyclic, now terminates and validates the entity's own fields.
- **Load behavior change (intended):** an all-value cycle that silently loads today now
  fails at load (`CircularSchemaReference`) — the Spec 006 rule restored. A cycle through
  a persistent schema is allowed (it is a relation, broken at the relation edge).
- Self-referential **value** schemas stay unsupported (Spec 006); self-referential
  **persistent** schemas (a tree via `parent: DbCategory`) are a normal FK pattern, allowed.

## 6. Out of scope

- Auditing/removing the rest of `route_loader::load` beyond the circular check.
- Any change to the persistence model or query navigation (Spec 025).
- No new syntax.

## 7. Acceptance criteria

1. A persistent schema in a relation cycle works as an API contract: validating against
   `User` (`User.orders: list of Order`, `Order.user: User`, both `db:`) terminates — the
   relation edges are let through, the entity's own fields are validated.
2. An all-persistent cycle loads and runs (Spec 025; the `functional_tests` fixture).
3. A cycle that passes through a persistent schema (e.g. value `Profile -> DbUser <->
   DbOrder`, or `Profile <-> DbUser`) is allowed — the validator breaks it at the
   relation edge, so it never loops.
4. An all-value-schema cycle (`A -> B -> A`, no `db:`) fails `serve`/`test`/`doctor` at
   load with `CircularSchemaReference` (`config_error`), and the validator reports an
   infinite-cycle error as a runtime backstop.
5. The loader and `marreta lint` resolve cycles through **one** shared relation-aware
   helper; the dead `route_loader::detect_circular_references` is gone.
6. Unit tests cover: the persistent-cyclic contract validating, the value-cycle load
   rejection, persistent and through-persistent cycles loading, and the validator's
   relation let-pass vs value-cycle error.
7. Standard gates: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, the
   full suite, `functional_tests`, `migrations_functional`, and `e2e` green.

## 8. Coverage analysis (per spec definition-of-done)

- **VS Code extension:** no change — no new language surface (no namespace/keyword/
  snippet/grammar change); the rule is validator + load-time behavior.
- **functional_tests (HTTP):** the runtime "allowed" scenarios are covered live against
  the served process (section 37): a persistent schema used as a contract validates its
  own fields and 422s on a bad scalar while letting relation fields pass; a deeply nested
  cyclic payload terminates (no loop); a value schema referencing a persistent schema
  validates its own field and lets the relation pass; a persistent self-referential
  (tree) schema works as a contract.
- **e2e / load errors:** an all-value cycle is a load **failure** (the project would not
  serve), so it cannot be a served-process scenario; it is covered by `validator`,
  `schema_cycle`, and `file_loader` unit tests. No new e2e endpoint or `run.sh` smoke line.

---

## Delivery notes

Delivered 2026-06-05.

- **Validator** (`src/validator.rs`) is relation-aware: `coerce_recursive` /
  `coerce_field_type` thread the schema names on the validation stack; a `Reference`
  whose target is persistent (`db_table.is_some()`) is let through as a relation (the
  value is accepted as-is, not recursed), so a persistent schema works as an API contract
  even inside a relation cycle. A value reference already on the stack is reported as an
  infinite-cycle error; `MAX_DEPTH` stays as a final backstop.
- **Shared rule** in `src/schema_cycle.rs` (`find_disallowed_cycle`): a cycle is
  disallowed only when it lies entirely within value schemas (DFS from value schemas,
  cutting edges into persistent schemas, mirroring the validator). The loader
  (`file_loader::detect_schema_cycle`, on the live `serve`/`test`/`doctor` path) and
  `marreta lint` both call it; the dead `route_loader::detect_circular_references` (and
  helpers/tests) were removed.
- **Behavior:** all-value cycles (incl. value self-reference) fail at load with
  `CircularSchemaReference`; cycles through any persistent schema — all-persistent
  relations, a value schema referencing into a relation cycle, persistent self-reference
  trees — load and validate, the relation edge being let through.
- **Tests:** `schema_cycle` (8), `validator` (persistent-cyclic contract validates,
  value cycle errors), `file_loader` load tests, `lint` (mixed-through-persistent
  allowed), and `functional_tests` section 37 (7 live HTTP scenarios). Gates all green:
  unit suite 1506 + 3 + 38 + 37; `functional_tests` 566/566; `migrations_functional`
  PASS; `e2e` 60 + 18; lint/doctor clean on every example.
- This supersedes the relation-aware lint rule from Spec 061 (which flagged any
  value-touching cycle) and the two interim review rules; the validator change made the
  narrower all-value rule the correct one.

## P.S. Do not forget the docs of record

On delivery, update both `CHANGELOG.md` and `docs/spec/SPEC.md`. See SPEC.md
section 1.3.
