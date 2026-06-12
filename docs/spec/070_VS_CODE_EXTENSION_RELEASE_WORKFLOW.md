# 070 - VS Code Extension Release Workflow

> Status: Delivered
> Type: Editor tooling + CI (release automation)
> Scope: Give the VS Code extension a real release path in the monorepo: a versioning convention,
> VSIX generation, and publication as a GitHub Release under a `vscode-v*` tag namespace that coexists
> with the runtime's `v*` releases (published with `--latest=false` so it never steals the installer's
> `releases/latest`). The one workflow also publishes to Open VSX and the MS Marketplace, each gated
> on its secret, and the binary-first install how-to ships with it. All manual (`workflow_dispatch`),
> consistent with the repo's manual-workflow policy.

---

## 1. Purpose

The extension under `docs/editors/vscode` is marketplace-ready in its manifest (name `marretalang`,
publisher `MarretaTeam`, version `0.2.18`, icon, MIT, categories, keywords) and the gates already
package a VSIX locally (`vsce package`). But there is **no release path**: nothing produces a
downloadable artifact, nothing publishes it, and there is no installable build for a launch user.

The monorepo already releases the **runtime** through `.github/workflows/release.yml`: manual
(`workflow_dispatch`), takes a `tag` input (`v0.2.0`), creates the tag if it does not exist, builds
the binaries from it, and publishes a GitHub Release. The validator workflows (`e2e`, `install`,
`release-smoke`) then download assets by that tag. The extension needs its own release line that
**does not collide** with this one, in the same repo.

The open question this spec settles: how the extension is versioned, built, and published as a
GitHub artifact in a monorepo, and (a reviewer decision) whether the same pipeline also pushes to the
extension registries.

## 2. The change

### 2.1 Tag-namespaced release lines (the monorepo answer)

A GitHub Release is keyed by a git tag, and a repo can hold any number of releases. The two release
lines stay distinct by **tag namespace**:

| Artifact | Tag | Release title |
|---|---|---|
| Runtime (exists) | `v0.2.0` | Marreta CLI v0.2.0 |
| Extension (new) | `vscode-v0.2.18` | VS Code Extension v0.2.18 |

They coexist on the Releases page, told apart by the tag prefix and the title. The tag marks the
monorepo commit the VSIX was built from.

This is additive for the **workflows**: every existing workflow takes an exact tag input, so none of
them matches the new tags. (A precise note for the future: a glob `v*` would match `vscode-v0.2.18`,
since it starts with `v`. No workflow uses a glob today. If one is ever introduced for the runtime
line, it must be `v[0-9]*`, never `v*`.) The one consumer that does collide across the tag namespace
is not a workflow but the **installer's latest-release contract**, handled in §2.3.

### 2.2 Versioning (single source, anti-drift guard)

The extension version is owned by `docs/editors/vscode/package.json` `version` (already `0.2.18`,
independent of the runtime's `0.2.0` in `Cargo.toml`). The release tag must equal
`vscode-v<package.json version>`. The workflow **asserts** this and fails on a mismatch, so a tag can
never publish a VSIX whose manifest version disagrees with it.

### 2.3 The release workflow (`.github/workflows/release-vscode.yml`)

A new workflow, mirroring the shape of `release.yml` but scoped to the extension:

- **Trigger:** `workflow_dispatch` only (the repo's manual-workflow policy), input `tag`
  (e.g. `vscode-v0.2.18`).
- **Create the tag if missing**, the same way `release.yml` does (the workflow owns the tag rather
  than requiring a hand-pushed one).
- **Single `ubuntu-latest` runner, no OS matrix.** A VSIX is cross-platform, unlike the runtime's
  per-target binaries, so the build is one job.
- **Build:** checkout at the tag, set up Node, `cd docs/editors/vscode`, `npm ci`, `node --check` on
  the JS (the gate's extension checks), `vsce package` to produce the `.vsix`.
- **Publish the GitHub Release, never as repo `latest`:** `gh release create "vscode-v<version>"
  *.vsix --latest=false`, with a clear title, attaching the VSIX as the release asset (the stable
  artifact the how-to links).

  This `--latest=false` is **critical, not cosmetic**. The one-line installer (`install.sh:59`)
  resolves `releases/latest/download/<runtime-target>` whenever no version is pinned, and GitHub's
  `releases/latest` is the most recent non-prerelease release of the **whole repo**, regardless of
  tag namespace. A VSIX release left as `latest` would make the README's `curl | sh` look for runtime
  binaries in a release that holds only a `.vsix`, a 404. It is worse than hypothetical today: no
  runtime release exists yet, so the first VSIX release would certainly become `latest`. Every
  extension release is therefore published with `--latest=false`, so the runtime keeps ownership of
  `releases/latest`.

### 2.4 Marketplace channels: all three, gated on secrets

The workflow publishes to all three channels in the same run:

- **GitHub Release VSIX** (always): the stable artifact, the fallback for any editor or air-gapped
  setup, and what the how-to links.
- **Open VSX** (`ovsx publish`): reaches Cursor, Windsurf, VSCodium, Gitpod, Theia. This is the only
  channel those editors read, so it is required for the Cursor users named as the target.
- **MS VS Code Marketplace** (`vsce publish`): reaches stock VS Code, the largest audience, with
  search discoverability and auto-update.

The code decision is **all three, once**. Which channels actually publish on a given run is an
**operational** decision decoupled from the code: each registry publish is **gated on the presence of
its secret** (`OVSX_TOKEN`, `VSCE_PAT`, documented in the workflow header the way the site's
`deploy.yml` documents its Azure and Cloudflare secrets). Adding a channel later is then a matter of
adding a secret, no new PR per channel, incremental and reversible.

The skip must be **loud, not silent**: the job writes one line to the run summary per channel
(`published: github, openvsx; skipped: marketplace (no VSCE_PAT)`), so an expired or missing PAT in
the real repo surfaces in the run instead of quietly dropping a publication.

Securing the `MarretaTeam` namespace on **both** registries is a **delivery-checklist item of this
spec**: anti-squat costs minutes and losing the name at launch is irreversible.

### 2.5 The install how-to (in this spec)

A release pipeline creates a user-facing surface (the install path), so by the house rule since Spec
064 its documentation ships in the same change. The how-to lives under `docs/guide` and reaches
marreta.dev on the next `scripts/sync-docs.sh`.

The extension is a thin CLI client: it spawns the `marreta` binary and already surfaces an actionable
prompt when the binary is absent. So the page's spine is **binary first, extension second**: install
the binary (the `install.sh` one-liner), then the extension, then set `marreta.path` if the binary is
not on `PATH`.

The **guaranteed** section is the GitHub Release VSIX install (that artifact exists as soon as the
§2.3 dry run runs), with the per-editor sideload (`code --install-extension`, Cursor's Install-from-
VSIX). The **registry** sections (VS Code via the Marketplace, Cursor and forks via Open VSX) are
written per channel and switched on at delivery as each channel actually publishes, mirroring the
gating in §2.4. This keeps the page honest: it never points at a Marketplace listing that is not live
yet.

## 3. Implementation outline

- New `.github/workflows/release-vscode.yml` (`workflow_dispatch`, single runner). Header documents
  the required secrets, as `deploy.yml` does in the site repo.
- Version source is `docs/editors/vscode/package.json`; the tag-equals-version assertion runs
  **before packaging** (a small inline step or a tiny script under `.github/scripts/`).
- The GitHub Release is created with `--latest=false` (§2.3), so it never displaces the runtime's
  ownership of `releases/latest`, which `install.sh` resolves.
- No change to `release.yml`, `e2e.yml`, `install.yml`, `release-smoke.yml` (they stay on the exact
  `v*` tags they already take as input).
- `ovsx`/`vsce` publish steps are each gated on their secret, so a fork without secrets still builds
  and attaches the VSIX without failing the run, and the run summary names what published vs skipped.
- One `docs/guide/how-to` page plus its `SUMMARY.md` entry, following `docs/STYLE.md`, with the binary
  install lifted from the existing `install.sh` one-liner.
- Coverage analysis (per the spec protocol):
  - **VS Code extension:** no language-surface change. The extension code is untouched; this spec is
    its release pipeline, not its behavior.
  - **e2e:** not applicable. No language feature changes, so the in-memory guardian has nothing new
    to exercise.
  - **Documentation:** the install how-to (§2.5) ships in this spec.

## 4. Out of scope

- Changing the runtime release scheme or its `v*` tags.
- Any auto-trigger (tag-push or release-published). Workflows stay `workflow_dispatch`, per the
  repo's manual-workflow policy.
- An OS build matrix for the VSIX (it is a single cross-platform artifact).
- Changes to the extension's behavior or its thin-client contract.

## 5. Acceptance criteria

1. A `workflow_dispatch` `release-vscode.yml` builds the VSIX from a `vscode-v<semver>` tag (created
   if missing), asserts the tag matches `docs/editors/vscode/package.json` `version` before packaging,
   and publishes a GitHub Release with the VSIX attached, on a single runner.
2. The GitHub Release is created with `--latest=false`: it never becomes the repo's `releases/latest`,
   so `install.sh`'s unpinned `releases/latest/download/<runtime-target>` keeps resolving to a runtime
   release. The dry run (AC6) asserts the created release has `isLatest == false` via the API.
3. The `vscode-v*` tag namespace coexists with the runtime `v*` line: every existing workflow takes an
   exact tag input (none uses a glob), so the runtime release path is unchanged.
4. The workflow publishes to Open VSX and the MS Marketplace, each gated on its secret
   (`OVSX_TOKEN`, `VSCE_PAT`); a missing secret skips that channel without failing the run, and the
   run summary names what published vs skipped per channel. The `MarretaTeam` namespace is secured on
   both registries (delivery checklist).
5. A how-to page under `docs/guide` sequences binary-first install (the `install.sh` one-liner, then
   the extension per editor: VS Code and Cursor), reaches the site through the docs mirror, and gates
   its registry sections on the channels being live so it never links a listing that is not published.
6. Gates: CI plus docs, with no Rust or extension-behavior change. The core gates stay green and the
   extension gate (`node --check` + a VSIX package) passes. The workflow is proven by a dispatched
   **dry run** before Delivered, hardened to: download the published VSIX and validate its contents
   (unzip, `package.json` at the expected version, not just local packaging), and assert
   `isLatest == false` on the created release.

---

## P.S. Do not forget the docs of record

On delivery, update both `CHANGELOG.md` and `docs/spec/SPEC.md`. See SPEC.md
section 1.3.

---

## Delivery notes

The extension now has a manual release path, proven end to end.

- **Workflow** (`.github/workflows/release-vscode.yml`): `workflow_dispatch`, tag-equals-`package.json`
  guard before packaging, create-tag-if-missing, single runner, VSIX via `vsce package`, GitHub
  Release with `make_latest:false`, and Open VSX + MS Marketplace publishes each gated on their
  secret with a loud per-channel run summary. Self-verify step asserts the release is not the API
  `releases/latest` and re-downloads the published VSIX to check its version.
- **Dry-run proof (AC6):** dispatched against `vscode-v0.2.18`, green. The verify step confirmed the
  API `releases/latest` does not point at the extension tag (so `install.sh` stays safe) and the
  published VSIX carried the expected version. The UI "Latest" chip on the sole release is cosmetic
  and migrates to the first runtime release automatically (the extension release is `make_latest:false`).
  One fix landed during the proof: the verify step used the API `releases/latest` endpoint instead of
  a non-existent `gh release view --json isLatest` field.
- **Install how-to** (`docs/guide/how-to/install-the-editor-extension.md` + SUMMARY, mirrored to the
  site): binary-first, command-palette-first (`Extensions: Install from VSIX`, no CLI on PATH needed),
  with the settings-path detail and the per-channel registry sections framed as forthcoming until live.
- **Curated release bodies** (`.github/release-notes/{runtime,extension}.md`, shipped alongside): both
  releases get an authored body. The runtime dropped `generate_release_notes` (its "What's changed"
  plus "Full Changelog" commits link were noise on a first release, and `CHANGELOG.md` is internal by
  its own charter). **Decision (a):** the first release ships with no changelog section; the **second**
  release introduces hand-curated highlights in the body (not an internal-log link, not an auto dump).
- **Channels:** code path is all three; which publish is operational (secret presence).
  **Anti-squat checklist (do before enabling a registry):** create the `MarretaTeam` namespace on
  Open VSX (`npx ovsx create-namespace MarretaTeam -p <token>`) and the `MarretaTeam` publisher on the
  MS Marketplace, before setting the tokens.
- **Gates:** core + extension green (`node --check` + VSIX package). No runtime or extension-behavior
  change.
