# Summary

<!-- What changes, and why. Link the spec under docs/spec when there is one. -->

## Documentation

The docs under `docs/guide` are authored by hand. There is no generator, so any
change that adds or alters behavior must update the docs in the same PR.

- [ ] Documentation updated for every added or changed surface (namespace, keyword,
      builtin, method, env var, CLI command, error code, schema type, or runtime
      semantics), following `docs/STYLE.md`.
- [ ] Every new doc code example is lifted from, or verified against, a tested
      project under `docs/examples`. A documented example that does not run is a
      defect. (A `db.` example must include the `marreta migrate generate` / `apply`
      step.)
- [ ] Docs prose uses no em-dashes, no semicolons, and no emojis.

## Quality gates

- [ ] `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`
      all pass.
- [ ] For runtime changes (`src/**`): `functional_tests`, `migrations_functional`,
      and the `e2e` suite pass.
- [ ] For VS Code extension changes (`docs/editors/vscode/**`): `node --check` and
      `vsce package` pass.

## Records of record

- [ ] `CHANGELOG.md` and `docs/spec/SPEC.md` reflect the change.
