# 063 - Runtime Compatibility Manifest (`requires_marreta`)

> Status: Delivered
> Type: Project manifest / loader
> Scope: Let a project declare the minimum Marreta Lang runtime it needs, in the
> committed manifest (`app.marreta`), and have the runtime enforce it at load with a
> clear error. Establishes the language/runtime compatibility contract before there are
> multiple released versions. Pairs with a language-versioning (semver) policy. No new
> language semantics for routes/tasks/schemas.

---

## 1. Purpose

A Marreta application is **source written against a version of the language** run by a
**runtime binary**. Today everything is `0.2.0`, so the two always match. Once there are
several released versions and breaking changes (we already produce pre-release breaking
changes — Spec 060 removed `queue.publish`, Spec 061 retired bare cross-file calls), an
app created against a newer language can be run on an **older runtime that does not
understand it**, failing in obscure ways.

This spec adds one manifest field — **`requires_marreta`** — so an app states the minimum
runtime it needs, and the runtime refuses to run on an older one with a clear,
load-time error (the same pattern as other config errors). It is the equivalent of
`package.json` `engines.node`, `go.mod` `go 1.21`, or `Cargo.toml` `rust-version`.

## 2. The two version axes (do not conflate)

These are **orthogonal** and must never be confused:

| Property | What it is | Set by | Example |
|---|---|---|---|
| `project_version` (existing) | the version of the **developer's product/app** | the app developer | `"2.3.1"` (the "shop" release) |
| `requires_marreta` (new) | the minimum **Marreta Lang runtime** the app needs | the Marreta ecosystem (`marreta init` / language semver) | `">=0.2.0"` |

Bumping the Marreta runtime does not change the product version, and releasing v3 of the
product does not change the Marreta required. `project_version` keeps its current meaning,
untouched. The new field always carries `marreta` in its name so the two read distinctly.

## 3. Model

```marreta
project_name     = "shop"
project_version  = "2.3.1"      # (existing) the product's own version — unchanged
requires_marreta = ">=0.2.0"    # (new) minimum Marreta Lang runtime required
```

- **Format (v1), frozen:** the value is a **string** `>=` immediately followed by exactly
  three dot-separated non-negative integers — `>=MAJOR.MINOR.PATCH`. Surrounding
  whitespace on the whole value is trimmed (`" >=0.2.0 "` is fine); **exactly zero or one
  space** is allowed between `>=` and the version (`">=0.2.0"` and `">= 0.2.0"`).
  **Rejected** (malformed → load error): a non-string value (`requires_marreta = 123`),
  two-or-more spaces or a tab after `>=`, a `v` prefix (`>=v0.2.0`), prerelease or build
  metadata (`-rc1`, `+build`), missing components (`>=0.2`), other operators (`^`, `~`,
  `>`, `=`), and ranges. Richer ranges/caret are out of scope; a single minimum covers
  the "app needs at least X" axis.
- **Rule:** the running runtime's version must be **>= the declared minimum**, by numeric
  semver comparison. If it is lower, project load fails (`serve` / `test` / `doctor` /
  `migrate`) with `IncompatibleRuntime` naming both versions.
- **Enforcement is real, not deferred:** e.g. `requires_marreta = ">=0.3.0"` on runtime
  `0.2.0` fails today. For a project freshly stamped by the current runtime the check is
  initially satisfied (it stamps the current compatibility floor — §3.2); it becomes
  meaningful as the runtime advances past that floor and as 0.x minor bumps land.
- **Optional / absent:** a project without `requires_marreta` loads as today (no check) —
  backward compatible. `marreta init` stamps it for new projects (§3.2).

### 3.1 Why the manifest (`app.marreta`), not `marreta.env`

The requirement is **intrinsic to the source**, not to a deployment, so it lives in the
committed manifest beside `project_name`/`project_version`:

- `marreta init` puts `marreta.env` in `.gitignore` (it holds deployment config/secrets;
  only `marreta.env.example` is committed). A requirement there would be **absent on a
  fresh clone** and unreviewed in PRs.
- `marreta.env` is **per-deployment** — it could differ between dev and prod or be
  forgotten in prod, defeating the guarantee. The requirement must be identical
  everywhere and travel with the code in git.
- Every ecosystem declares the runtime requirement in the **manifest**, never in an
  env/secrets file (`package.json engines`, `go.mod`, `Cargo.toml rust-version`).

### 3.2 How the value is filled — automatic vs manual

`marreta init` should **not** stamp `>=<current runtime version>`. That over-restricts:
if the language has not had a breaking change since `0.5.0` and the current runtime is
`0.9.0`, a freshly-scaffolded project runs fine on any runtime in `0.5.0 ..= 0.9.0`, so
requiring `>=0.9.0` would needlessly reject `0.7.0`.

The value `init` stamps is the **compatibility floor**: the most recent version that
introduced a **breaking** language/runtime change. A scaffold uses only stable,
long-available features, so it needs at least that floor and nothing newer.

Two runtime-known numbers (do not confuse them):

- **runtime version** (`CARGO_PKG_VERSION`, e.g. `0.7.0`) — the binary's own version; used
  by the load check and `marreta --version`.
- **`COMPAT_FLOOR`** (e.g. `0.5.0`) — the last version with a breaking change; the only
  value `init` stamps. Today both are `0.2.0`.

Who fills what:

| Actor | Action | Automatic or manual |
|---|---|---|
| Marreta maintainers | maintain `COMPAT_FLOOR` (a controlled constant — `version::COMPAT_FLOOR` or `[package.metadata]`) | **manual but disciplined** — changes only on a breaking release; that PR bumps the version **and** advances the floor together (§4) |
| `marreta init` | writes `requires_marreta = ">=<COMPAT_FLOOR>"` into the new `app.marreta` | **automatic** — the developer does nothing |
| App developer | normally never touches it | **manual and rare** — only raises it when adopting a feature newer than the floor (see below) |

So the *stamping in `app.marreta` is automatic*; what is manual is the *maintenance of
`COMPAT_FLOOR`*, which happens only at breaking releases, not every release.

Timeline example:

```
0.2.0  COMPAT_FLOOR = 0.2.0   (today)
0.3.0  additive release    → floor stays 0.2.0
0.4.0  additive            → floor stays 0.2.0
0.5.0  BREAKING CHANGE      → bump version to 0.5.0 AND set COMPAT_FLOOR = 0.5.0
0.6.0  additive            → floor stays 0.5.0
0.7.0  additive            → floor stays 0.5.0
```

`marreta init` on runtime `0.7.0` stamps `requires_marreta = ">=0.5.0"` (the floor, not
`0.7.0`). That app then loads on any runtime `>= 0.5.0` and is rejected on `0.4.0`.

**The one manual case for a developer (accepted limitation):** if they later adopt a
feature introduced *after* the floor (additive, e.g. added in `0.6.0`), the true minimum
for their app rises to `0.6.0`, but `init` could not know that — they raise
`requires_marreta` to `">=0.6.0"` themselves, exactly like bumping `engines.node` when you
start using a newer API. Auto-deriving the minimum from feature usage is out of scope.

## 4. Dependency: a language-versioning policy

`requires_marreta` only means something if version bumps are disciplined. This spec
records the policy in `SPEC.md` §1.5 (Language Versioning Policy):

- **Every breaking language/runtime change bumps a defined component** — the minor while
  `0.x` (pre-1.0), the major once `1.0` is reached.
- **The same breaking-change release advances the compatibility floor** (§3.2): the
  controlled `COMPAT_FLOOR` constant is set to the new version, in the same PR as the
  break. Non-breaking (additive) releases bump the version but leave `COMPAT_FLOOR`
  unchanged.
- This gives two runtime-known values: its own version (`CARGO_PKG_VERSION`, used by the
  load check and `--version`) and `COMPAT_FLOOR` (the last breaking version, used by
  `init` to stamp). Today both are `0.2.0`.

Without this discipline the version number — and therefore `requires_marreta` — is
decorative.

## 5. Implementation outline

- **Parse + validate at load** (`file_loader`): read `requires_marreta` from the
  entrypoint (alongside the existing `project_name`/`project_version` validation); if
  present, compare the runtime version (`CARGO_PKG_VERSION` via the existing
  `version` module) against the declared minimum and, on a lower runtime, return a new
  config error `IncompatibleRuntime { required, actual }` (so `serve`/`test`/`doctor`
  fail uniformly, like `CircularSchemaReference`). A malformed `requires_marreta` value
  fails load with a clear error in the same family as the other entrypoint-metadata
  validation errors (missing `project_name`/`project_version`).
- **`marreta init`** stamps `requires_marreta = ">=<COMPAT_FLOOR>"` (§3.2) in the
  generated `app.marreta` — the last breaking version, not the current runtime.
- **`doctor`**: the compatibility check is a hard load error, so when the runtime is
  **incompatible** `doctor` fails at load with `IncompatibleRuntime` like any other
  config error (it never reaches report rendering — no special pre-read of the
  manifest). When the project **loads** (compatible), the Project section shows the
  `requires_marreta` value as an informational `OK` line. So `doctor` only ever *displays*
  the field in the satisfied case; the unsatisfied case is the load error itself.
- **Lint**: add `requires_marreta` to the project-metadata allowlist
  (`allow_project_metadata`) so it is not flagged as an unknown/unused assignment.
- **Version comparison**: a small semver compare (or a lightweight existing dependency)
  for `>=`. No need for a full range grammar in v1.

### 5.1 CLI surface (which commands change)

The compatibility check lives at **project load**, so every command that loads a project
inherits it; only `init` and `doctor` need command-specific work.

| Command | Change |
|---|---|
| `marreta init` | **Generates** `requires_marreta = ">=<COMPAT_FLOOR>"` (§3.2) in the scaffolded `app.marreta`, beside `project_name`/`project_version`. (The only command that *writes* the field.) |
| `marreta serve` | Inherits the load check — refuses to start on an incompatible runtime with the config error. |
| `marreta test` | Inherits the load check (fails before running scenarios). |
| `marreta doctor` | Inherits the load check (fails at load when incompatible). When the project loads, **displays** the `requires_marreta` value as an informational Project-section line — it never renders a "failed" line, since incompatibility is the load error itself. |
| `marreta migrate` | Inherits the load check (loads the project first). |
| `marreta lint` | Allowlists `requires_marreta` as project metadata so it is not flagged; lint itself does not enforce the version (it is a load/config concern). |
| `marreta fmt` | No behavior change — `requires_marreta` is a normal top-level assignment; verify it round-trips through the formatter. |
| `marreta --version` | Unchanged; it already reports the runtime version that the check compares against (the reference value). |

No new command is added. There is intentionally **no** command that auto-bumps
`requires_marreta` (the floor is the developer's decision — see §6).

## 6. Out of scope

- The reverse axis — an **old app on a newer runtime** that removed/changed a feature.
  That is handled by a **deprecation policy** and, if it becomes painful post-1.0, by
  **language editions** (the app declares an edition the runtime can run in compat mode).
  `requires_marreta` is forward-extensible toward that, but editions are not built here.
- Rich version ranges (caret, multiple constraints) — start with a single `>=` minimum.
- `created_with_marreta` (provenance) — not enforced, not worth its surface yet; can be
  added later if migration tooling needs it.
- Auto-bumping `requires_marreta` on `marreta` upgrade — the floor is the developer's
  choice; tooling may suggest, not rewrite.

## 7. Acceptance criteria

1. A project with `requires_marreta = ">=X.Y.Z"` higher than the running runtime fails
   `serve` / `test` / `doctor` at load with a clear config error naming both the required
   and the actual version; one with a satisfied (`<=` current) minimum loads normally.
2. A project without `requires_marreta` loads exactly as today (no check; backward
   compatible).
3. A malformed `requires_marreta` value fails load with a clear error (the same family as
   the other entrypoint-metadata validation errors).
4. `marreta init` generates `app.marreta` with `requires_marreta = ">=<COMPAT_FLOOR>"`
   (the last breaking version, §3.2), distinct from `project_version`; a breaking-change
   release advances `COMPAT_FLOOR`.
5. When the project loads (compatible), `doctor` displays `requires_marreta` as an
   informational line; when incompatible, `doctor` fails at load with `IncompatibleRuntime`
   (no rendered report). `lint` does not flag the field.
6. `project_version` semantics are unchanged and never conflated with `requires_marreta`
   (covered by the doc and by tests asserting the two are independent).
7. `SPEC.md` records the language-versioning (semver) policy that gives the field meaning.
8. Standard gates: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, the
   full suite, `functional_tests`, `migrations_functional`, and `e2e` green.

## 8. Coverage analysis (per spec definition-of-done)

- **VS Code extension:** no change. `requires_marreta` is a manifest assignment, not new
  route/task/schema surface, and the tooling catalog has **no metadata-key entries today**
  (`project_name`/`project_version` are not cataloged; `CatalogKind` is only
  Keyword/Namespace/Function/Method). Adding completion/hover for it would mean building a
  new metadata-completion mechanism — disproportionate; out of scope.
- **e2e / functional_tests / examples:** the incompatible-runtime case is a **load
  failure** (the app would not serve), so it is covered by loader unit tests, not a
  served-process scenario. The satisfied case is dogfooded — **every example, benchmark,
  and e2e `app.marreta` declares `requires_marreta = ">=0.2.0"`**, so the whole suite
  (functional_tests, migrations_functional, e2e, init_functional) loads and runs with the
  field present, and `init_functional` asserts that `marreta init` stamps it.

---

## Delivery notes

Delivered 2026-06-05.

- **Manifest field + load check.** `app.marreta` may declare `requires_marreta = ">=X.Y.Z"`.
  `file_loader::validate_entrypoint_metadata` parses it: absent → no check; a non-string
  value or a malformed string → load error (`io_error`, like the sibling
  `project_name`/`project_version` checks); a runtime older than the minimum →
  `MarretaError::IncompatibleRuntime { required, actual }` (config_error). The check is at
  load, so `serve`/`test`/`doctor`/`migrate` all inherit it.
- **Format** (`version::parse_requires_marreta`, frozen): `>=MAJOR.MINOR.PATCH`, outer
  whitespace trimmed, zero or one space after `>=`; rejects non-string, two+ spaces/tab,
  `v` prefix, prerelease/build, missing components, other operators/ranges.
- **`COMPAT_FLOOR`** (`version.rs`, today `0.2.0`) is the last breaking version; `marreta
  init` stamps `requires_marreta = ">=<COMPAT_FLOOR>"` (not the current runtime). The
  Language Versioning Policy (SPEC.md §1.5) advances the floor on breaking releases.
- **doctor** shows `requires_marreta` (with the running runtime) when the project loads;
  incompatibility is the load error itself. **lint** allowlists the field.
- **Dogfood:** every example/benchmark/e2e `app.marreta` declares `requires_marreta =
  ">=0.2.0"`; `init_functional` asserts `init` stamps it.
- **Out of scope, as planned:** the reverse axis (old app on a newer runtime) →
  deprecation policy / future editions; `created_with_marreta` provenance; rich ranges.
- Gates: fmt + clippy(`-D warnings`); suite 1515 + 3 + 38 + 37; `functional_tests`
  566/566; `migrations_functional` PASS; `e2e` 60 + 18; `init_functional` PASS;
  doctor/lint clean on every example.

## P.S. Do not forget the docs of record

On delivery, update both `CHANGELOG.md` and `docs/spec/SPEC.md` (including §1.5 with the
versioning policy). See SPEC.md section 1.3.
