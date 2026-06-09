# 047 — Repository Split

> Status: Approved
> Type: Repository organization / release hygiene
> Scope: Split non-runtime assets out of `marreta-lang` into dedicated sibling repositories

---

## 1. Purpose

`marreta-lang` should become the runtime/CLI repository only.

The current repository also contains specs, the VS Code extension, brand assets,
examples, and performance material. These assets have different release cycles,
validation needs, and audiences.

This spec defines the split so each concern can evolve independently.

---

## 2. Target Repositories

| Repository | Source path today | Target scope |
|---|---|---|
| `marreta-lang` | current repo | Runtime, CLI, parser, tests, `CHANGELOG.md` |
| `marreta-lang-spec` | `docs/spec` | Official language specs and spec history |
| `marreta-lang-vscode` | `docs/vscode-marreta` | VS Code extension source and package metadata |
| `marreta-lang-brand` | `docs/logo` | Logo, visual assets, and brand material |
| `marreta-lang-examples` | `examples` | Official runnable examples |
| `marreta-lang-performance` | `docs/performance`, `tests/load` | Load tests, performance methodology, and results |

---

## 3. Decisions

1. `marreta-lang` keeps no copy of moved content.
2. `marreta-lang` does not need placeholder links in this first cut.
3. `CHANGELOG.md` remains in `marreta-lang` for now.
4. Specs live only in `marreta-lang-spec`.
5. Examples live only in `marreta-lang-examples`.
6. VS Code extension lives only in `marreta-lang-vscode`.
7. Brand assets live only in `marreta-lang-brand`.
8. Performance assets live only in `marreta-lang-performance`.

---

## 4. Examples Runtime Contract

Official examples assume:

- Docker Compose is the default execution path.
- A Marreta runtime image exists in the local Docker registry/image cache.
- If an example needs host execution, `marreta` is available in `PATH`.
- Examples should not depend on source paths inside `marreta-lang`.

---

## 5. Non-Goals

This spec does not define:

- GitHub remote creation.
- CI/CD for the new repositories.
- Website publishing.
- Binary distribution.
- Docker image publishing.
- `marreta-lang-dist`.
- `marreta-lang-website`.
- Cross-repo release automation.

Those will be handled by future specs.

---

## 6. Migration Plan

### Phase 1 — Create sibling local repositories

Create directories next to `marreta-lang`:

- `../marreta-lang-spec`
- `../marreta-lang-vscode`
- `../marreta-lang-brand`
- `../marreta-lang-examples`
- `../marreta-lang-performance`

Copy the corresponding source content into each target.

### Phase 2 — Validate copied content exists

Verify file counts and key files in each new directory.

### Phase 3 — Remove moved content from runtime repo

Remove from `marreta-lang`:

- `docs/spec`
- `docs/vscode-marreta`
- `docs/logo`
- `docs/performance`
- `tests/load`
- `examples`

Keep `CHANGELOG.md`.

### Phase 4 — Validate runtime repo

Run:

```bash
cargo test --lib
cargo test --test integration_tests
cargo test --bin marreta
```

If integration tests still depend on removed examples, update or split those
tests deliberately instead of silently deleting coverage.

Preferred runtime-repo strategy:

- full functional examples and end-to-end example validation move to
  `marreta-lang-examples`;
- `marreta-lang` must not keep copied functional test suites under
  `tests/fixtures`;
- runtime tests may keep only small, purpose-built fixtures when a test cannot
  generate input dynamically;
- `init`, `fmt`, `lint`, and editor tooling tests should prefer temporary
  generated projects;
- runtime CI must not depend on sibling repositories.

---

## 7. Risks

### Runtime tests may depend on `examples`

Some integration tests currently validate generated projects and functional
examples. Removing `examples` requires explicit handling:

- full functional validation moves to `marreta-lang-examples`;
- runtime fixtures, if needed, must be minimal and purpose-built;
- tests that can generate temporary projects should do so instead of reading
  extracted example folders.

This must be handled deliberately. The split must not silently delete coverage.

### Docs references may break

Some repository documentation may reference moved paths. This first cut does not
add replacement links, but broken internal references should not remain inside
runtime tests or CLI help.

---

## 8. Validation

The split is complete when:

- each target directory exists with copied content;
- `marreta-lang` no longer contains moved directories;
- runtime build/tests pass or failures are explicitly classified as expected
  follow-up for extracted example tests;
- no `.vsix` artifacts are reintroduced;
- `git status` clearly shows only the intended removals/updates.
