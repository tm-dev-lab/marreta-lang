# 048 — Runtime Versioning

> Status: Delivered
> Type: Release hygiene / CLI contract
> Scope: Define a single source of truth for the Marreta runtime and CLI version

---

## 1. Purpose

The runtime currently exposes different versions depending on the code path:

- `Cargo.toml` declares the package version.
- `marreta --version` prints a hardcoded version.
- `marreta --help` prints a hardcoded version.
- `marreta repl` prints a different hardcoded version.
- `marreta serve` reads `env!("CARGO_PKG_VERSION")` for the runtime version.
- Tests assert hardcoded version strings.

This creates drift and makes releases ambiguous.

This spec defines a single runtime version source and the release process around
it.

---

## 2. Version Domains

Marreta has three distinct version domains. They must not be mixed.

| Domain | Source of truth | Meaning |
|---|---|---|
| Runtime/CLI version | `Cargo.toml` package `version` | Version of the distributed `marreta` binary |
| Application version | `project_version` in `app.marreta` | Version of the user application served by Marreta |
| Spec/history labels | Spec files and `CHANGELOG.md` headings | Historical implementation phases, not binary release versions |

Only the runtime/CLI version is shown by:

- `marreta --version`
- `marreta --help`
- `marreta repl`
- runtime startup output

Application metadata remains separate:

```text
Application smart-inventory (version 0.1.0) started ... (runtime MarretaLang v0.2.0)
```

In that example:

- `0.1.0` is the application version.
- `0.2.0` is the runtime/CLI version.

---

## 3. Decisions

1. `Cargo.toml` package `version` is the only source of truth for the runtime
   version.
2. Rust code must read the runtime version through `env!("CARGO_PKG_VERSION")`
   or a shared helper that wraps it.
3. Runtime code must not hardcode strings like `MarretaLang v0.2.0`.
4. Tests must not assert fixed runtime version strings unless the string is
   derived from `env!("CARGO_PKG_VERSION")`.
5. `project_version` is application metadata only and must never affect
   `marreta --version`.
6. Spec numbers and changelog phase labels are historical planning labels only.
   They must never drive runtime version output.
7. Git release tags should match the Cargo package version using the format
   `vX.Y.Z`.

---

## 4. Runtime API

Introduce a small version module:

```rust
pub const MARRETA_NAME: &str = "MarretaLang";
pub const MARRETA_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn runtime_version_label() -> String {
    format!("{MARRETA_NAME} v{MARRETA_VERSION}")
}
```

All runtime/CLI display sites use this module.

Expected replacements:

| Location | Current behavior | New behavior |
|---|---|---|
| `marreta --version` | hardcoded string | `runtime_version_label()` |
| `marreta --help` | hardcoded string | `runtime_version_label()` in header |
| `marreta repl` | hardcoded string | `runtime_version_label()` |
| `marreta serve` startup | direct `env!("CARGO_PKG_VERSION")` | shared version helper |
| tests | hardcoded `vX.Y.Z` | `env!("CARGO_PKG_VERSION")` |

---

## 5. Release Process

For a runtime release:

1. Update `Cargo.toml` package `version`.
2. Regenerate/update `Cargo.lock` through Cargo.
3. Update `CHANGELOG.md` with the release summary.
4. Run:

```bash
cargo check
cargo test
cargo build --release
```

5. Create a Git tag matching the package version:

```bash
git tag vX.Y.Z
```

6. Publish or distribute artifacts from that tagged commit.

The release process does not require editing source files just to change version
strings.

---

## 6. Non-Goals

This spec does not define:

- package publishing to crates.io
- Docker image tagging
- binary distribution repository layout
- website release notes
- cross-repository release automation
- compatibility matrix between runtime and examples/spec repos

Those belong to future distribution/release specs.

---

## 7. Implementation Plan

### Phase 1 — Add runtime version helper

Create `src/version.rs` and expose it from `src/lib.rs`.

### Phase 2 — Replace CLI hardcodes

Update:

- `marreta --version`
- `marreta --help`
- `marreta repl`

### Phase 3 — Normalize server startup

Make server startup use the shared helper while preserving the distinction
between application version and runtime version.

### Phase 4 — Update tests

Replace hardcoded runtime version expectations with assertions derived from
`env!("CARGO_PKG_VERSION")`.

### Phase 5 — Set current runtime version deliberately

Choose the current runtime version and set it in `Cargo.toml`.

The current `0.1.0` value is stale relative to the delivered feature surface.
The exact replacement version should be chosen explicitly before implementation.

---

## 8. Test Plan

Required coverage:

- `marreta --version` prints `MarretaLang v{CARGO_PKG_VERSION}`.
- `marreta --help` prints the same runtime version in the header.
- `marreta repl` prints the same runtime version in the banner.
- `marreta serve` startup output includes the same runtime version.
- `project_version` continues to appear as application metadata and does not
  affect runtime version output.
- No runtime source file contains hardcoded `MarretaLang v0.` strings.

Validation commands:

```bash
cargo check
cargo test
cargo build
```

---

## 9. Open Question

What should the next runtime version be?

Options:

- `0.2.0`: conservative, aligns with the current hardcoded CLI string.
- `0.15.0`: aligns with the current changelog phase label but risks confusing
  planning labels with binary releases.
- `1.0.0-alpha.1`: starts public-release semantics, but should only be used if
  the project is ready to commit to pre-1.0 artifact distribution.

Recommendation: use `0.2.0` now, then move to normal SemVer increments from
that point forward.

---

## 10. Delivery Notes

Delivered with runtime version `0.2.0`.

Implementation summary:

- `Cargo.toml` is the runtime/CLI version source of truth.
- `Cargo.lock` records the same package version.
- `src/version.rs` centralizes runtime name and version formatting.
- `marreta --version`, `marreta --help`, and `marreta repl` use the shared
  helper.
- `marreta serve` startup output uses the same Cargo-derived runtime version
  while preserving `project_version` as application metadata.
- Tests derive expectations from `env!("CARGO_PKG_VERSION")`.

Validation:

```bash
cargo fmt
cargo check
cargo test
cargo build
cargo build --release
target/debug/marreta --version
target/debug/marreta --help
printf '.exit\n' | target/debug/marreta repl
```
