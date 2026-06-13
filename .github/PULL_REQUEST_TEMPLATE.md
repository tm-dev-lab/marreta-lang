# Summary

<!-- What changes, and why. Link the spec under docs/spec/ when this implements one. -->

## Type of change

<!-- Trivial change (typo, doc, small fix with no design decision)? Fill Summary + Type, delete the rest. -->

- [ ] Trivial (typo, doc wording, a small fix with no design decision)
- [ ] Substantial (language or CLI surface, a provider or namespace, an error or event-log contract)
- [ ] Bug fix

<!-- Everything below is for substantial changes and non-trivial bug fixes. -->

---

## Linked issue, proposal, or spec

<!-- Substantial changes need an approved spec first (see CONTRIBUTING.md). Bug fixes link their bug issue. -->

Linked: #

## Tests

- [ ] The new behavior is covered end to end (a test that exercises the change through the real surface), not only no-regression on existing suites.
- [ ] Language features add or update `e2e` scenarios (the in-memory guardian tracks the language).
- [ ] No existing test was weakened, skipped, or removed to make this pass.

## Coverage analysis

<!-- One line per axis. On the negative case, give a reason ("not affected because ..."). -->

- Extension: updated in `<file>`, or not affected because `<reason>`
- e2e: updated in `<file>`, or not affected because `<reason>`
- Documentation: updated in `<file>`, or not affected because `<reason>`

## Documentation

- [ ] Docs under `docs/guide` are updated for every added or changed surface (namespace, keyword, builtin, method, env var, CLI command, error code, schema type, or runtime semantics), following `docs/STYLE.md`.
- [ ] Every new doc code example is lifted from, or verified against, a tested project under `docs/examples`. A documented example that does not run is a defect.
- [ ] Docs prose uses no em-dashes, no semicolons, and no emojis.

## Quality gates

- [ ] `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` pass.
- [ ] Runtime changes (`src/**`): `functional_tests`, `migrations_functional`, and the `e2e` suite pass.
- [ ] Extension changes (`docs/editors/vscode/**`): `node --check` and `vsce package` pass.

## Docs of record

- [ ] `CHANGELOG.md` and `docs/spec/SPEC.md` reflect the change.

<!-- If this change only proves live (a published page, a rendered form, a release artifact), note how and when it is verified post-merge. -->
