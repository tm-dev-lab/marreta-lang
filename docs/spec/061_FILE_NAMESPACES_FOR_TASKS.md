# 061 - File-Name Namespaces for Exported Tasks

> Status: Delivered
> Type: Language semantics / modules
> Scope: Make an exported task's origin visible at the call site by accessing it
> through a namespace inferred from its file name — `file.task()` — consistent with
> the built-in namespaces (`db.find`, `cache.get`). No imports. Pre-release breaking
> change: cross-file exported tasks are reached only via `file.task()`; a bare name
> resolves only within the file that declares it (and from `app.marreta`).

---

## 1. Purpose

`export task` publishes a task into a shared global space, callable by **bare name**
from any file. That is convenient but opaque: reading `charge(order)` you cannot tell
which file `charge` lives in, so you have to hunt for it. As projects grow this hurts
readability.

The language deliberately has **no imports**, so a declared-module or `use`-style
import is off the table. But it already exposes built-in capabilities through
namespaces — `db.find(1)`, `cache.get(k)`, `topic.publish(...)` — and a `.marreta`
file is already a runtime module internally (`ModuleRuntime` keyed by a path-derived
id).

This spec turns each file into an **inferred namespace named after the file**, so an
exported task is called `file.task()` — the same shape as the built-in namespaces,
with the origin visible at the call site, and zero import ceremony.

## 2. Model

### 2.1 The namespace is the file stem

A file `greetings.marreta` (anywhere in the project) exposes its exported tasks under
the namespace `greetings`. `tasks/billing.marreta` exposes `billing`. The namespace
is the file **stem** (name without `.marreta` and without directory), so calls stay
short: `billing.charge(order)`.

### 2.2 Visibility (only exported tasks, only via the namespace)

- **Within the declaring file**, any task (exported or not) is callable **bare**:
  `charge(order)`. Unchanged.
- **From another file**, only **exported** tasks are reachable, and **only** through
  the namespace: `billing.charge(order)`. A non-exported task stays file-private.
- A **bare cross-file** call no longer resolves — it is an "undefined task" error.
  This is the breaking change that delivers the readability goal: cross-file calls
  must name their origin.
- `app.marreta` stays the implicitly-global entrypoint: everything in it is callable
  bare from anywhere (it is the glue), and `app` is **not** a namespace.

### 2.3 Reuses the built-in-namespace call shape (no parser change)

`billing.charge(x)` parses today exactly like `db.find(x)` —
`MethodCall { object: Identifier("billing"), method: "charge", arguments }`. So the
**parser does not change**. Resolution happens at eval time, where the interpreter
already routes `X.foo(args)` by inspecting `X`. File-namespaces are a new branch in
that routing.

## 3. Resolution and precedence

For `X.foo(args)` the interpreter resolves `X` in this order:

1. **Built-in namespace** (`db`, `cache`, `topic`, … — see 4.2): dispatch the
   built-in. Built-ins are reserved and always win.
2. **A value bound in scope** named `X`: it is a method call on that value (today's
   behavior). A local variable shadows a file-namespace, predictably.
3. **A file-namespace** `X`: call the exported task `foo` from that file; error if
   the file has no exported task `foo`.
4. Otherwise: undefined.

## 4. Collisions and reserved names (load-time)

All of these are detected when the project loads (`serve` / `test` / `doctor`),
failing uniformly like other project load errors, never silently appended or
overwritten.

### 4.1 Two files with the same stem

`a/util.marreta` and `b/util.marreta` both infer the namespace `util`. If **both
declare exported tasks**, this is a **load error** naming both files (rename one).
Files that export nothing do not register a namespace, so they never collide. (Merging
the two into one namespace is rejected: it is ambiguous and can itself collide on task
names.)

### 4.2 A file stem that matches a built-in namespace

A file `db.marreta` (stem `db`) collides with the reserved built-in `db`. This is
**forbidden — a load error**, never an append/override of the native namespace (that
would let a file silently shadow `db.find`). The reserved set is the built-in
namespaces, which the runtime derives from the catalog so it stays in sync:
`base64, cache, db, doc, feature, fs, http_client, json, log, math, queue, time,
topic, uuid`. **`app` is also reserved** (see 4.4). (A file that declares no exported
task is not registered as a namespace, so `db.marreta` with no exports is allowed but
useless.)

### 4.3 A file stem that is not a valid identifier

`my-tasks.marreta` would yield the namespace `my-tasks`, but `my-tasks.greet()` lexes
as subtraction. So a file that **exports tasks** must have an identifier-safe stem
(`[A-Za-z_][A-Za-z0-9_]*`). A file exporting tasks with a non-identifier stem is a
**load error** with a rename hint (`my_tasks.marreta`). Files with no exports are
unaffected.

### 4.4 The `app` stem is reserved (entrypoint only)

The project entrypoint `app.marreta` (at the project root) is the implicitly-global
scope, not a namespace — there is no `app` namespace. So `app` is **reserved** for
file-namespaces: any *other* file with stem `app` (e.g. `routes/app.marreta`) that
exports tasks is a **load error**. This removes the ambiguity between the stem rule
(which would mint an `app` namespace) and the entrypoint rule (which says `app` is not
a namespace).

### 4.5 Same task name across different namespaces is allowed

The model naturally allows `billing.charge()` and `payments.charge()` to coexist:
exported task names are scoped to their file-namespace, so the **same name in two
different files is fine**. Two exported tasks with the same name in the **same file
(same namespace)** remain an error (a duplicate declaration). Today the loader dedups
exported names *globally* (`exported_names` → `ExportConflict` on any cross-file
duplicate); for **tasks** this becomes per-namespace. Exported schemas and variables
keep the current global-by-name dedup while they are out of scope (§6).

## 5. Implementation outline

- **No parser change** (3.3).
- **Registry** (`ProjectRuntime`): build `namespace -> { exported task name -> task }`
  from the modules, keyed by file stem; enforce the load-time rules in §4 (stem
  collisions, reserved built-ins + `app`, non-identifier stems).
- **Loader**: relax the global `exported_names` dedup (today any cross-file duplicate
  is an `ExportConflict`) so exported **tasks** dedup per-namespace — same name in
  different files is allowed, same name within one file is still an error. Exported
  schemas/variables keep the global dedup while out of scope.
- **Interpreter**: extend the `MethodCall` routing with the file-namespace branch
  (§3), reading the registry from the attached project runtime. **Resolution order
  matters:** `MethodCall` today evaluates `object` first, so `billing.charge()` would
  turn `Identifier("billing")` into an `UndefinedVariable` before any dispatch. The
  file-namespace branch must intercept an `Identifier(ns)` that names a known
  file-namespace (and is not a bound variable) **before** that value-evaluation path —
  mirroring how built-in namespace identifiers are already special-cased.
- **Bare resolution**: a bare task call resolves only against the current file's tasks
  plus `app.marreta`'s globals; cross-file bare no longer resolves.
- **Tooling**: completions after `file.` suggest that file's exported tasks;
  go-to-definition resolves `file.task` via the file-namespace; hover shows the
  origin; the `tooling` symbol/definition work from Spec 059 is extended for this.
- **Lint**: task-reference collection counts both a bare call (in the declaring file)
  and a `file.task` call (anywhere) as a use. `unused_private_task` keeps working, and
  a new `unused_exported_task` warns when an exported task is never referenced at all
  (no bare in its file, no `file.task` anywhere) — the cross-file analogue of the
  private-task check.
- **Doctor**: an informational "Modules" section lists each file-namespace and its
  exported tasks (the project's public cross-file surface), following the
  non-failing, informational style of the Spec 054 coverage summary. The hard rules in
  §4 are already load errors that `doctor` inherits, so they are not re-checked there.
- **Project sources**: update cross-file bare calls in `functional_tests`, `e2e`,
  and the example/benchmark projects to `file.task()`.

## 6. Out of scope (this spec)

- **Exported schemas.** `take payload as NewUser` references an exported schema by
  bare name and has the same "where is it" question, but schema references appear in
  several positions (`take … as`, `reply … as`, nested field types, constructors).
  Namespacing schemas (`core.NewUser`) is deferred to a follow-up so this spec stays
  focused on tasks. Exported schemas keep their current global-by-name behavior for
  now.
- Exported variables (`export set …`) — keep current behavior; revisit with schemas.
- Any import/`use` statement — explicitly excluded (no imports is a language tenet).

## 7. Resolved decisions

- **Self-qualified calls allowed.** An exported task is callable as `file.task()` from
  within its own file too (the namespace is valid everywhere), in addition to the bare
  form.
- **Stem-only namespaces.** The namespace is the file stem, never path-qualified;
  same-stem collisions among exporting files are a load error (§4.1).
- **Bare cross-file is a hard error** (no transitional warning) — pre-release, and it
  is what enforces the readability goal.

## 8. Acceptance criteria

1. An exported task in `billing.marreta` is callable as `billing.charge(...)` from
   another file, and bare `charge(...)` within `billing.marreta`.
2. A bare cross-file call to an exported task no longer resolves (undefined task);
   non-exported tasks remain file-private; `app.marreta` tasks stay bare-global.
3. Resolution precedence (built-in namespace > bound value method > file-namespace)
   holds; a local variable shadows a same-named file-namespace.
4. Load errors, with clear messages, for: two exporting files with the same stem; a
   file stem equal to a built-in namespace; a non-entrypoint exporting file with stem
   `app`; an exporting file whose stem is not a valid identifier.
5. The same exported task name in two different files coexists (`billing.charge`,
   `payments.charge`); the same exported task name twice in one file is an error; the
   global export-name dedup is relaxed to per-namespace for tasks (schemas/variables
   keep global dedup).
6. Completions after `file.` list that file's exported tasks; go-to-definition and
   hover resolve `file.task`.
7. `doctor` shows an informational "Modules" section listing each file-namespace and
   its exported tasks; it never fails the command.
8. Lint: `unused_exported_task` warns for an exported task never referenced (bare in
   its file or via `file.task` anywhere); `unused_private_task` still applies to
   non-exported tasks; both count `file.task` calls as uses.
9. All repo `.marreta` sources use the new cross-file form; `functional_tests`,
   `migrations_functional`, and `e2e` pass.
10. Standard gates: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
    the full test suite, `functional_tests`, and `migrations_functional` green.

---

## Delivery notes

Delivered 2026-06-04.

- **Loader registry.** `ProjectRuntime.task_namespaces` (`stem -> { task -> Task }`).
  Non-entrypoint exported tasks register here instead of `global_env`; exported
  variables and entrypoint tasks stay global/bare. Load errors (§4) are enforced while
  building the registry: same-stem collision among exporters, stem equal to a built-in
  namespace, the reserved `app` stem, a non-identifier stem, and a duplicate exported
  task within one file. The global `exported_names` dedup is relaxed to per-namespace
  for tasks; schemas/variables keep the global dedup (§6).
- **Interpreter.** `MethodCall` intercepts `Identifier(ns).method()` before evaluating
  the object (mirroring built-in namespaces); precedence is built-in > bound value >
  file-namespace. A known namespace with no matching exported task reports
  `task 'ns.method' is not defined` rather than a misleading "variable not defined".
  Namespaced tasks also work as pipeline stages (`>> file.task`, with list iteration)
  and broadcast targets (`-> file.task`, including the pure fast path) — these parse as
  `PropertyAccess`, resolved through the registry.
- **Doctor / lint.** Informational `Modules` section; `unused_exported_task` lint. The
  reference collector was fixed to also count bare pipeline/broadcast targets as uses
  (a pre-existing gap that the new lint would otherwise have surfaced as false
  positives).
- **Tooling / extension.** `ToolingSymbol` carries `exported` + `namespace`;
  completions after `file.` list only exported tasks; go-to-definition disambiguates
  `file.task` across same-named tasks; a semantic-tokens provider colors
  file-namespaces and their exported-task methods with the built-in-namespace scopes.
- **Scope.** Spec 060's `topic` surface needed no extension change (already complete).
  The e2e suite is the in-memory guardian, so topic/queue (infra) stays in
  `functional_tests`; e2e gained file-namespace coverage instead.

## P.S. Do not forget the docs of record

On delivery, update both `CHANGELOG.md` and `docs/spec/SPEC.md`. See SPEC.md
section 1.3.
