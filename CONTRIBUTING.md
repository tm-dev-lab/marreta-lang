# Contributing to Marreta

Thanks for considering a contribution. Marreta is a focused language for building REST APIs, and it
is developed spec-first. This guide explains how a change gets accepted, with the barrier
proportional to the size of the change.

## How changes are accepted

There are three paths. Pick the one that matches your change.

### Trivial changes

A typo, a wording fix in the docs, or an obvious small bug fix with no design decision. Just fork
and open a pull request. There is no barrier beyond the usual review. The
[pull request checklist](.github/PULL_REQUEST_TEMPLATE.md) still applies where relevant.

### Substantial changes (proposal first)

Anything user-visible by design: language semantics or syntax, the CLI surface, a new provider or
namespace, or a change to an error or event-log contract. These go **proposal first**, so design is
agreed before code is written.

1. Open a [proposal issue](https://github.com/tm-dev-lab/marreta-lang/issues/new?template=proposal.yml)
   describing the problem, the motivation, the proposed change, how it fits the language's focused
   scope, and the alternatives you considered.
2. Maintainers review the proposal. An accepted proposal becomes a spec under
   [`docs/spec/`](docs/spec/), written from [`docs/spec/TEMPLATE.md`](docs/spec/TEMPLATE.md).
3. Once the spec is reviewed, implementation proceeds by fork and pull request.

This is the same spec-driven flow the project uses internally. It follows the precedent of the Rust
RFC and Python PEP processes: not every change needs a proposal, but the ones that shape the
language do.

### Bugs

Open a [bug report](https://github.com/tm-dev-lab/marreta-lang/issues/new?template=bug_report.yml) with your `marreta --version` output,
your platform, a minimal `.marreta` reproduction, the expected versus the actual behavior, and any
relevant logs. Triage decides the path: a plain fix goes straight to a pull request, while a fix
that implies a design decision is escalated to a proposal.

## Scope: focused by design

Marreta is deliberately narrow (see the "Philosophy" and scope framing in the
[README](README.md)). A proposal can be declined for scope alone, even when it is well made. The
proposal stage exists precisely so that conversation happens before you write code, not after.

## Development

Prerequisites: Rust 1.85 or newer, Docker and Docker Compose (only for containerized example
validation), and Node.js (only for editor tooling). The language and tooling are developed and
tested on Linux, macOS, and Windows via WSL.

Repository layout:

| Path | Purpose |
| --- | --- |
| `src/` | Runtime, CLI, parser, interpreter, server, providers, tooling commands |
| `tests/` | Rust integration tests and fixtures |
| `e2e/` | In-memory feature suite exercised over localhost (see `e2e/README.md`) |
| `docs/examples/` | Example projects and functional validation suites |
| `docs/editors/` | The published VS Code extension (a thin client over the CLI) |
| `docs/spec/`, `docs/assets/` | Language specs and brand assets |
| `.github/workflows/` | Manual build, release, extension release, e2e, and smoke workflows |

Before opening a pull request:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

If your change affects runtime behavior for generated projects, functional examples, migrations, or
provider integration, also run the relevant example suites under `docs/examples/` before requesting
review.

Guidelines:

- Keep changes scoped to one problem.
- Add or update tests for behavior changes.
- Do not weaken tests to make a change pass.
- Avoid broad refactors unless the refactor is the point.
- Prefer explicit design notes for language semantics and public CLI behavior.

The house style for the language and for prose lives in
[`docs/guide/reference/conventions.md`](docs/guide/reference/conventions.md). Documentation under
`docs/guide` is authored by hand, so any change that adds or alters behavior updates the docs in the
same pull request. The [pull request checklist](.github/PULL_REQUEST_TEMPLATE.md) is the source of
the per-PR requirements.

## License

Contributions are licensed under the MIT license, inbound equals outbound. By opening a pull
request you agree your contribution is provided under the [LICENSE](LICENSE). There is no separate
contributor agreement.

## What to expect from review

This is a small maintainer team. Review latency is measured in days. Silence means limited
bandwidth, not rejection. A polite nudge on the issue or pull request after a week is welcome.

## Code of conduct

This project follows the [Contributor Covenant](CODE_OF_CONDUCT.md). By participating you agree to
uphold it.

## Security

Do not report security vulnerabilities through public issues. See [SECURITY.md](SECURITY.md) for the
private reporting path.
