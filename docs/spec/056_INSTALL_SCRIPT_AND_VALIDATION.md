# 056 - Install Script and Cross-Platform Install Validation

> Status: Delivered
> Type: Distribution / developer experience
> Scope: Add a POSIX `install.sh` that detects the host, downloads a published
> release binary, installs it on the user's `PATH`, and verifies it. Point the
> README install instructions at a one-line `curl ... | sh`. Add a manual
> workflow that validates the installer across the full OS matrix against a
> published release tag. Companion to the Release Smoke Test (Spec 053 area) and
> the Release E2E suite (Spec 055).

---

## 1. Purpose

Installing Marreta Lang today is a manual sequence: open GitHub Releases, pick the
right asset for your OS and architecture, `chmod`, `mv` it onto your `PATH`, then
run `--version`. That is friction for a language that sells "zero ceremony", and it
is easy to pick the wrong asset.

This spec adds a one-line installer that does exactly what the smoke and e2e
workflows already do to obtain a binary, but on the user's machine:

1. Detect the operating system and architecture.
2. Resolve the release to install (latest by default, or a pinned version).
3. Download the matching asset from GitHub Releases.
4. Install it into a `PATH` directory without requiring `sudo`.
5. Verify the install by running `marreta --version`.

It also adds a manual **Install Validation** workflow so the installer itself is
covered on every supported platform, the same way the smoke and e2e suites cover
the binary. The installer is a public entry point: if it breaks on one OS, that is
a broken first impression, so it deserves the same OS matrix as the release.

## 2. The installer (`install.sh`)

A single POSIX `sh` script at the repository root, so the README can serve it
directly from the raw URL and a user can read it before running it.

### 2.1 Host detection

Map `uname -s` and `uname -m` to the published asset names, which are fixed by the
release workflow:

| `uname -s` | `uname -m`            | asset                   |
| ---------- | --------------------- | ----------------------- |
| `Linux`    | `x86_64`              | `marreta-linux-x86_64`  |
| `Linux`    | `aarch64` / `arm64`   | `marreta-linux-arm64`   |
| `Darwin`   | `x86_64`              | `marreta-macos-x86_64`  |
| `Darwin`   | `arm64`               | `marreta-macos-arm64`   |

Native Windows is out of scope, consistent with the rest of the project: Windows
users run the script inside WSL, where it resolves to the Linux x86_64 asset. An
unrecognized OS or architecture is a clear error that points at the manual download
instructions and the WSL note.

### 2.2 Resolving the version

- Default: the latest release, via GitHub's stable redirect
  `https://github.com/<owner>/<repo>/releases/latest/download/<asset>`, followed
  with `curl -fL`. This keeps the install path off the GitHub API: no `jq`
  dependency, no hand-rolled JSON parsing in POSIX `sh`, and no exposure to API
  rate limits.
- Override: a pinned version through the first positional argument or the
  `MARRETA_VERSION` environment variable (for example `v0.2.0`), downloaded from
  `releases/download/<tag>/<asset>`. A pin lets users reproduce an exact install
  and lets the validation workflow target the dispatched tag deterministically.
- Printing the resolved tag is best-effort (for example derived from the redirect
  target), never a requirement of the install path.

### 2.3 Target override (testing and edge setups)

`MARRETA_TARGET` overrides the detected asset name. This serves two needs: it lets
a user on an atypical setup force a specific build, and it lets the validation
workflow exercise the macOS x86_64 (Rosetta) branch on an arm64 runner, where
`uname -m` would otherwise resolve to `arm64`.

### 2.4 Install location

- Default install directory: `~/.local/bin`, created if missing. This needs no
  `sudo` and matches the directory the README and the project already use.
- Override: `MARRETA_INSTALL_DIR`.
- After installing, if the chosen directory is not on `PATH`, print a short note
  telling the user to add it to their shell profile. The script does not edit shell
  profiles for the user.

### 2.5 Download and install steps

1. Build the download URL: the `releases/latest/download/<asset>` redirect for the
   default, or `releases/download/<tag>/<asset>` for a pinned version.
2. Download with `curl -fL` (following redirects, with `wget` as a fallback) to a
   temporary file.
3. `chmod +x` and move it to `<install-dir>/marreta`.
4. Run `<install-dir>/marreta --version` by its absolute path and print the result,
   so the script's own verification never depends on `<install-dir>` being on
   `PATH`.

The script uses `set -eu`, cleans up its temporary file on exit, and fails with a
readable message (not a stack of shell errors) when a dependency is missing, a
download fails, or the asset does not exist for the requested tag and target.

### 2.6 Usage

```sh
# latest release
curl -fsSL https://raw.githubusercontent.com/tm-dev-lab/marreta-lang/main/install.sh | sh

# pinned version
curl -fsSL https://raw.githubusercontent.com/tm-dev-lab/marreta-lang/main/install.sh | sh -s -- v0.2.0
```

## 3. README change

Replace the manual download block in the Installation section with the one-line
installer as the primary path, and keep the manual download as an alternative for
users who prefer it or are on an unsupported target. Keep the existing supported
environments note (Linux, macOS, Windows via WSL) and the `PATH` guidance.

## 4. Install Validation workflow (`install.yml`)

A manual `workflow_dispatch` workflow that mirrors the smoke and e2e matrix, so the
installer is proven on the same platforms as the binary.

- Input: `tag`, the release tag to install (for example `v0.2.0`).
- Matrix (same five legs as Release Smoke Test and Release E2E):
  Linux x86_64, Linux arm64, macOS x86_64 (Rosetta), macOS arm64, Windows (WSL).
- Each leg installs into a job-scoped directory so the assertion never depends on
  the runner's `PATH`: it sets `MARRETA_INSTALL_DIR="$RUNNER_TEMP/marreta-bin"` and
  runs the checked-out `install.sh` with the dispatched tag pinned, validating the
  script at that ref against the real published assets:
  - Native legs run `MARRETA_VERSION=<tag> MARRETA_INSTALL_DIR=... sh install.sh`
    (the macOS Rosetta leg also sets `MARRETA_TARGET=marreta-macos-x86_64` and
    ensures Rosetta 2).
  - The WSL leg runs the script inside Ubuntu, resolving to the Linux x86_64 asset.
- Each leg then asserts `"$MARRETA_INSTALL_DIR/marreta" --version` exits zero and
  reports a version that matches the dispatched tag (compared without a leading
  `v`). Asserting the absolute path tests the installer without relying on
  `~/.local/bin` being on `PATH`, which is not guaranteed on macOS or WSL.

The workflow validates the script logic and the per-OS download and install path.
It complements, and does not replace, the smoke and e2e suites that validate the
binary's behavior.

## 5. Out of scope and future hardening

- Native Windows installer (`.ps1`). WSL covers Windows for the first distribution
  target, consistent with the rest of the project.
- Checksum or signature verification of the downloaded asset. This is worthwhile
  once the release publishes a checksums asset, and can be added without changing
  the user-facing one-liner.
- A hosted short URL (for example `https://marreta.dev/install.sh`). The raw GitHub
  URL is the initial source and can be fronted by a stable URL later.

## 6. Acceptance criteria

1. `install.sh` exists at the repository root, passes `shellcheck`, and:
   - installs the latest release with no arguments, via the
     `releases/latest/download` redirect (no GitHub API, no `jq`),
   - installs a pinned version via argument or `MARRETA_VERSION`,
   - honors `MARRETA_INSTALL_DIR` and `MARRETA_TARGET`,
   - fails with a readable message on an unknown target or a missing asset,
   - ends by running `<install-dir>/marreta --version` by its absolute path.
2. The README Installation section leads with the one-line installer and keeps a
   manual alternative.
3. `install.yml` runs on manual dispatch with a `tag` input and passes on all five
   matrix legs (Linux x86_64, Linux arm64, macOS x86_64 via Rosetta, macOS arm64,
   Windows via WSL), each installing into `$RUNNER_TEMP/marreta-bin` and asserting
   `"$MARRETA_INSTALL_DIR/marreta" --version` matches the tag, without relying on
   the runner's `PATH`.
4. The `.marreta` files are unaffected. This spec touches distribution only, not
   the runtime, so the standard runtime gates do not apply beyond keeping the suite
   green.

## 7. Delivery notes

- `install.sh` at the repository root: host detection with `MARRETA_TARGET`
  override, latest via the `releases/latest/download` redirect, pinned via
  argument or `MARRETA_VERSION`, install to `~/.local/bin` (override
  `MARRETA_INSTALL_DIR`), absolute-path `--version` verification, and a PATH hint
  that never edits a shell profile.
- `.github/workflows/install.yml`: manual `tag` input, the five-leg matrix, each
  leg running the checked-out script and asserting `"$MARRETA_INSTALL_DIR/marreta"
  --version` matches the tag, without a separate download step (the script
  downloads).
- README Installation section leads with the one-line installer and keeps the
  manual download as an alternative.
- Local validation: `sh -n` syntax check, `shellcheck` clean (one intentional
  SC2016 literal documented with a disable directive), host detection, and the
  missing-asset error path (readable message, non-zero exit). The full
  download-install-verify happy path is validated by `install.yml` across the OS
  matrix against a published release, the same model as the Release Smoke Test and
  Release E2E (Spec 055).

---

## P.S. Do not forget the docs of record

On delivery, update both `CHANGELOG.md` (the Current Status note, and a delivery
entry when warranted) and `docs/spec/SPEC.md` (the Active Follow-Ups section, and
any user-facing syntax or behavior that changed). A spec is not done until these
two are in sync with it. See the general convention in SPEC.md section 1.3.
