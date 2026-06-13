# NNN - Title

> Status: Proposed
> Type: The area this touches (for example Language syntax, CLI surface, Editor tooling, Migrations, Governance).
> Scope: One paragraph. What changes, why, and any pre-launch note. Keep it to the essentials a reader needs before the details.

---

## 1. Purpose

Why this change exists: the problem in the current behavior, grounded in what the code or the repo
actually does today (cite the file or the observed behavior). State the cost of leaving it as is.

## 2. The change

The proposed behavior in concrete terms. Use as many numbered subsections (2.1, 2.2, ...) as the
change needs, each describing one part. Prefer before/after or a short example where it clarifies.

## 3. Implementation outline

The contained surfaces this touches (parser, interpreter, CLI, tooling, grammar, docs, and so on).
Call out anything non-obvious: operational strings, scenario mocks, a catalog, an invariant.

### Test requirements

What proves the change: unit tests (one per rule or case, positive and negative), and functional
coverage that exercises the new behavior end to end through the real surface, not only a
no-regression run of existing suites.

### Coverage analysis

Answer all three axes out loud, even when the answer is "no change":

- **VS Code extension**: does this change the language surface the editor should know (a namespace,
  keyword, builtin, grammar token, snippet, or a completion/hover/definition behavior)? If yes,
  update the thin client.
- **e2e**: does this add or change a language feature the in-memory guardian should track? If yes,
  add `routes/*.marreta` plus `tests/*.marreta` scenarios (and a `run.sh` smoke assertion when
  execution or resolution semantics change).
- **Documentation**: which authored pages under `docs/guide` must reflect the added or changed
  surface (namespace, keyword, builtin, method, env var, CLI command, error code, schema type, or
  runtime semantics)? Every example must be lifted from or verified against a tested project under
  `docs/examples`.

## 4. Out of scope

- What this spec deliberately does not do, and why or what follows up.

## 5. Acceptance criteria

1. Observable, testable outcomes, one per line.
2. ...
N. Standard gates: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, the full test
   suite, and for runtime changes the `functional_tests` and `migrations_functional` suites, and for
   extension changes `node --check` plus a VSIX package.

---

## P.S. Do not forget the docs of record

On delivery, update both `CHANGELOG.md` and `docs/spec/SPEC.md`. See SPEC.md section 1.3.
