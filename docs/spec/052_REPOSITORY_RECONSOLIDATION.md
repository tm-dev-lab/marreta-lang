# 052 - Repository Re-consolidation

> Status: Delivered (engineering monorepo in place: spec/, examples/, benchmarks/, performance/, editors/ consolidated under marreta-lang; brand assets in marreta-lang-assets, with a README-referenced subset under assets/)
> Type: Repository organization / developer workflow
> Scope: Collapse the split sibling repositories back into two repositories — an
> engineering monorepo (`marreta-lang`) and a public assets repository
> (`marreta-lang-assets`). Supersedes Spec 047.

---

## 1. Purpose

Spec 047 split `marreta-lang` into many sibling repositories (spec, vscode,
brand, examples, performance) so each concern could evolve independently.

In practice the split went too far. Most of those concerns **version and are
validated together with the runtime**: a single logical change (e.g. a runtime
feature) routinely touches the runtime, its spec, an example, and a benchmark at
once. Under the split this means coordinated commits across several repositories
and constant risk of version drift between the runtime and the material that
validates it.

This spec re-consolidates by a sharper criterion:

> **Versions and is validated against the runtime → engineering monorepo.
> Public/marketing material with an independent cadence → assets repository.**

The result is two repositories instead of seven.

---

## 2. Target Repositories

### 2.1 `marreta-lang` (engineering monorepo)

Runtime at the root (unchanged); the rest as subdirectories:

```
marreta-lang/
├── Cargo.toml, src/, tests/, Dockerfile, CHANGELOG.md   (runtime — unchanged)
├── spec/            (was marreta-lang-spec)
├── examples/        (was marreta-lang-examples)
├── benchmarks/      (was marreta-lang-bench; results/ git-ignored)
├── performance/     (was marreta-lang-performance)
└── editors/vscode/  (was marreta-lang-vscode)
```

### 2.2 `marreta-lang-assets` (public assets)

```
marreta-lang-assets/
├── brand/   (was marreta-lang-brand)
└── site/    (new, empty scaffold)
```

---

## 3. Migration Decisions

1. **Only `marreta-lang` history is preserved.** The runtime repository keeps its
   full git history. Everything merged in arrives as a **snapshot of the source
   repository's `main`** — no history carried over. The other repositories' git
   history is intentionally not preserved (docs/harness/assets; the runtime
   history is the one that matters).
2. **Snapshots exclude generated/vendored content:** `.git`, `target/`,
   `node_modules/`, and benchmark `results/`.
3. **The old `marreta-lang-site` content is discarded.** The site is recreated as
   an empty scaffold under `marreta-lang-assets/site/`. `brand` is migrated as-is.
4. **The VS Code extension joins the monorepo** under `editors/vscode/`, so its
   grammar/syntax stays in sync with the runtime in the same change. Its
   marketplace publish runs from an isolated, tag-scoped workflow (CI detail,
   §6).
5. **Runtime stays at the repository root** — no `/runtime` subdir — to avoid
   churning every runtime path and CI reference for no benefit.
6. **Content is moved verbatim.** This spec does not edit the migrated files
   (path/link fixups are follow-up); it only relocates them.

---

## 4. `.gitignore`

The monorepo `.gitignore` must cover the absorbed subtrees:

- `benchmarks/**/results/` (raw benchmark output is never committed — see Spec
  049/050);
- `editors/vscode/node_modules/` and VS Code build artifacts (`*.vsix`);
- any `node_modules/` and `target/` under the new subdirectories.

---

## 5. Validation

The consolidation is behavior-neutral; it must not change runtime, examples, or
benchmark behavior. After the move:

- `cargo fmt --check`, `cargo build`, `cargo test` at the monorepo root pass
  (runtime unaffected by sibling subdirectories);
- `examples/functional_tests` and `examples/migrations_functional` pass against a
  freshly built `marreta-lang:dev` image (the suites use script-relative paths,
  so relocation must not break them — verify);
- the benchmark harness still launches against `marreta-lang:dev`.

If any example/benchmark script depended on the old sibling layout, fix it as part
of this spec rather than leaving it broken.

---

## 6. Non-Goals

- **No remote/GitHub changes here.** Creating the new remote, pushing, and
  archiving the old repositories are deliberate, owner-driven steps done after the
  local consolidation is validated.
- **No CI path-filtering yet.** Per-component CI (don't run `cargo` on docs-only
  changes; tag-scoped VS Code publish) is a follow-up; it does not block the move.
- **No content edits.** Fixing internal path references/links inside migrated docs
  is a follow-up.
- **No deletion of the old local source repositories** as part of execution; they
  are removed deliberately by the owner once the consolidation is confirmed.

---

## 7. Relationship To Other Specs

- **Supersedes Spec 047 (Repository Split).** The criterion changes from "split
  every non-runtime concern out" to "keep what versions with the runtime in the
  monorepo; only public assets live apart."
- Specs themselves now live in `marreta-lang/spec/`; this file is the first to be
  authored under the consolidated layout's intent.
