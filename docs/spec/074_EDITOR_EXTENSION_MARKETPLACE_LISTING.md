# 074 - Editor Extension Marketplace Listing

> Status: Delivered
> Type: Editor tooling (extension metadata + docs), no runtime change
> Scope: Rewrite the VS Code extension's `README.md` (which both the VS Code Marketplace and Open
> VSX render as the extension's detail page) from an internal developer note into a user-facing
> listing, move the maintainer-only content out of the rendered page, and polish the `package.json`
> marketplace metadata (the short search blurb, keywords, categories). No change to extension
> behavior or the language surface. This spec carries open design questions for the reviewer
> brainstorm (visuals, house-style on a marketplace surface, maturity framing).

---

## 1. Purpose

The text a user sees when they find the extension on the VS Code Marketplace or Open VSX is the
extension's `README.md` (`docs/editors/vscode/README.md`), rendered verbatim as the detail page.
Today that file is written for maintainers, not users:

- it opens with "This folder contains the lightweight VS Code bundle for MarretaLang", which is a
  repository-internal sentence, not a value proposition,
- it lists features as an implementation inventory ("with full-token spans", "a single Serve lens
  on the project bootstrap `app.marreta`") rather than what the user gets,
- it includes a maintainer checklist ("If the language surface changes, update ...") and a full
  "Publishing to the Marketplace (maintainers)" section with PAT setup and `vsce` commands, all of
  which render on the public listing as noise,
- the single most important fact for a new user (the extension is a thin client and needs the
  `marreta` CLI installed, or nothing works) is buried in the middle, not surfaced up front.

The result is a listing that reads like a source-tree note and risks a bad first run (install,
open a `.marreta` file, see nothing work because the CLI is missing). The extension's actual
capability is strong; the listing undersells and misframes it.

## 2. The change

### 2.1 Rewrite `README.md` as a user-facing listing

Restructure to the shape a good marketplace listing uses, user-first:

1. **Title and one-line value proposition** (what the extension does for you).
2. **Requirements**, surfaced above the fold (before or immediately after the value prop): the
   extension is powered by the `marreta` CLI and needs it installed (or `marreta.path` set). It
   does not duplicate the install steps. It states the essentials (install the CLI, then the
   extension) and links the canonical how-to,
   `docs/guide/how-to/install-the-editor-extension.md` (Spec 070), as the single source of the
   per-editor steps (see F1).
3. **Features**, grouped and outcome-phrased (syntax highlighting and the file icon; completions,
   hover, diagnostics, and format-on-save; document and workspace symbols and cross-file
   go-to-definition; the unused-variable quick-fix, scenario CodeLens, palette commands, and the
   status bar health item; standalone `.marreta` files still get intelligence).
4. **Getting started**: install the CLI, install the extension, open a `.marreta` file (linking the
   canonical how-to for detail, not restating it).
5. **Settings**: `marreta.path`, the diagnostics toggle, the tooling debounce.
6. **How it works** (one short paragraph): the extension is a thin client over the `marreta` CLI,
   so editor intelligence always matches the installed runtime, framed as a property, not a caveat.
   The honest scope note (no language-server daemon, refactoring, or debugger yet) folds into this
   single paragraph as one line, not a standalone "what this is not" catalog (Q3).
7. **Links**: the documentation site, the language repository, the changelog.

The feature content is the same capability that exists today (no behavior change), only the framing
and ordering change.

### 2.2 One screenshot (visual floor), GIFs as a named follow-up

A marketplace listing with no image reads as abandoned to someone browsing, before they read a
word, so a text-only listing is below the floor. Include at least one static screenshot (a
completion popup or an inline diagnostic), which is a screen capture of minutes. Animated GIFs
(format-on-save, live completions), which need a capture/optimize/host pipeline, are a named
follow-up and must not block the listing (Q1).

The screenshot asset is committed under the extension directory, but the README references it by an
**absolute** `https://raw.githubusercontent.com/.../<screenshot>` URL (pinned to `main`), not a
relative path (F-new). The VS Code Marketplace rewrites relative image URLs to the repo's raw URL
using `package.json` `repository`, and Open VSX historically diverges on that rewrite, so a relative
path that previews fine locally can render broken on the live listing (the same class as Spec 070's
`--latest` trap, where the local check passes and the published page is wrong). An absolute URL
sidesteps the rewrite and resolves identically on both registries by construction. Pinning to `main`
is acceptable here (a listing image is not a versioned contract); the asset must be committed and
pushed before the publish so the raw URL resolves.

### 2.3 Move maintainer-only content out of the rendered listing

The "If the language surface changes, update ..." checklist, the grammar/scope token inventory (the
long "task, route, schema, ..." list, which is bad for a user but a useful coverage record), and
the entire "Publishing to the Marketplace (maintainers)" section move to a separate file in the
extension directory, `docs/editors/vscode/PUBLISHING.md` (Q4, F4). It is co-located with the
artifact it describes (release operations belong next to the extension, not in contributor
governance docs), and it **references** the `release-vscode.yml` workflow and the Open VSX / MS
Marketplace namespace prerequisite from Spec 070 rather than duplicating them (no drift). Excluded
from the packaged VSIX via `.vscodeignore`. Nothing maintainer-facing is lost, it just stops
rendering on the public listing.

### 2.4 Polish the `package.json` marketplace metadata

- **`description`** (the short blurb under the title in search results): today
  "Official VS Code support for the Marreta language", which states neither what you get nor the
  key fact. Replace with a blurb that packs the searchable capabilities, the file type, the domain,
  and the thin-client differentiator (Q5). Agreed wording:
  `Syntax, completion, diagnostics, and formatting for Marreta (.marreta) REST APIs, powered by the Marreta CLI.`
- **`keywords` / `categories`**: review for discoverability (confirm `Programming Languages`,
  `Snippets`, `Formatters`, `Linters` as fitting, and the keyword set covers how a user searches).
- `displayName` ("MarretaLang") and `icon` stay.

### 2.5 House style on the listing: strictly text, no emoji, no badges

The listing stays text-only, with no emoji and no badges, on principle, not as an exception (Q2):

- the marketplace renders its own chrome (install count, rating, version) above the README, so an
  install/version badge on the listing is a duplicate, and the root README's badges already point
  *to* the listing, so putting them *on* the listing is circular,
- the brand is restraint by identity, and emoji ages badly on a page that stays up for years.

Scannability comes from structure (short sections, requirements on top, features by outcome) and
the 2.2 screenshot, not from emoji or badges.

## 3. Implementation outline

- `docs/editors/vscode/README.md`: the rewrite (2.1, 2.5).
- `docs/editors/vscode/` screenshot asset plus the README reference (2.2).
- `docs/editors/vscode/PUBLISHING.md` (new) plus `.vscodeignore` adjustment: the maintainer content
  move, referencing Spec 070 rather than duplicating it (2.3, F4).
- `docs/editors/vscode/package.json`: metadata polish (2.4) and a `version` bump.
- **How the listing actually updates (F3):** the marketplace and Open VSX pages do not change by
  editing files or merging to `main`. They change on a publish, which runs through Spec 070's
  `release-vscode.yml` (whose guard requires the `vscode-v*` tag to equal `package.json` version,
  and which publishes per channel only when that channel's secret is set). So this spec's listing
  goes live via a 070 publish, not on merge. The spec records this so no one expects the page to
  change on its own.
- No `src/**` change, no grammar/snippet/provider change, no language-surface change.

### Coverage analysis (spec protocol)

- **VS Code extension**: this spec *is* the extension surface (metadata + listing), no code path
  changes; the extension tier gate (`node --check` + VSIX package) still applies because
  `package.json` changes.
- **e2e**: none (no language behavior).
- **Documentation**: the listing is the doc; it must not become a third source of install steps. It
  links the canonical how-to (Spec 070) and stays consistent with the root README's Editor Support
  section (F1).

## 4. Out of scope

- Any change to extension behavior, grammar, snippets, completions, or commands.
- New editor features (a language-server daemon, refactoring, debugging), unchanged non-goals.
- The marketplace "verified publisher" status (a domain/account verification step, separate from
  the listing content).
- Animated GIFs of the extension in action (named follow-up after the static screenshot ships).

## 5. Acceptance criteria

1. The rendered `README.md` opens with a user value proposition and presents features as outcomes;
   no maintainer checklist, grammar inventory, or publishing instructions render on the listing.
2. **(F2, the highest-risk decision)** The `marreta` CLI requirement, with a link to install,
   appears above the fold (before or immediately after the value prop), so a user cannot install
   the extension without learning it needs the CLI. (A dead extension from a missing CLI is the
   one-star "does not work" outcome, and it is fully preventable by content ordering.)
3. **(F1, single source)** The listing does not restate the per-editor install steps. It links the
   canonical how-to (`install-the-editor-extension.md`, Spec 070) and stays consistent with the
   root README's Editor Support section. No third copy of install instructions is created.
4. At least one static screenshot renders on the listing (the visual floor), referenced by an
   absolute `raw.githubusercontent.com` URL (not a relative path), so it resolves identically on
   both the VS Code Marketplace and Open VSX; verified by the URL opening directly in a browser, not
   by a local preview (which does not simulate the marketplace's relative-URL rewrite). GIFs are
   explicitly deferred (F-new).
5. Maintainer content (publishing, the language-surface checklist, the grammar/scope inventory)
   lives in `PUBLISHING.md`, references Spec 070 rather than duplicating it, and does not appear on
   the public detail page.
6. `package.json` `description` is the agreed blurb (2.4), `keywords`/`categories` are reviewed, and
   `version` is bumped. The spec and `PUBLISHING.md` record that the listing goes live via a Spec
   070 publish (tag == `package.json`), not on merge (F3).
7. The listing is text-only: no emoji, no badges (2.5).
8. The extension packages cleanly: `node --check` on the JS plus a `vsce package` succeed, and the
   rendered README is verified before publish. The Markdown structure can be checked locally, but
   image resolution is proven by opening the absolute screenshot URL directly (the local preview
   does not simulate the marketplace's relative-URL rewrite, so it cannot prove AC4).

---

## Delivery notes

Delivered. Extension tier gate green (`node --check` on the JS plus `vsce package`; the VSIX ships
only `readme.md`, with `PUBLISHING.md` and `images/` excluded). No runtime change.

What landed:

- `docs/editors/vscode/README.md` rewritten as a user-facing listing: value proposition, the
  `marreta` CLI requirement above the fold linking the canonical how-to
  (`marreta.dev/docs/how-to/install-the-editor-extension`, no second copy of install steps),
  features by outcome, settings, a one-paragraph "how it works" (thin client as a property, scope in
  one line), and links. Text-only, no emoji or badges. The "Ruby-inspired" framing was removed at
  the owner's correction as inaccurate.
- `docs/editors/vscode/images/completion.png`: the listing screenshot (a completion popup with
  inline documentation), referenced by an absolute `raw.githubusercontent.com` URL pinned to `main`
  so it survives the marketplace's relative-URL rewrite (the Spec 070 `--latest` class of bug).
- `docs/editors/vscode/PUBLISHING.md` (new): maintainer content moved off the rendered listing
  (publishing via Spec 070's `release-vscode.yml`, referenced not duplicated; the namespace
  prerequisite; the language-surface checklist; the grammar/scope inventory as a coverage record).
- `package.json`: the search blurb `description`, `version` 0.2.18 to 0.2.19, the `Formatters`
  category, and discoverability keywords. `.vscodeignore` excludes `PUBLISHING.md` and `images/`.

Two review findings shaped it. The design brainstorm's F1 (do not create a third source of install
instructions) is honored by linking the canonical how-to. The diff review caught a broken link
(Finding 1): the how-to URL omitted `/docs/`, diverging from the canonical path the root README and
the 070 release notes use, the exact link a user without the CLI clicks. Both classes (the
relative-image-URL rewrite and the link path) are invisible to a local Markdown preview, so they
were verified against the established canonical URLs, not the preview.

Note on going live: a merge does not change the marketplace or Open VSX page. The listing goes live
on the next publish through `release-vscode.yml` (its guard requires the `vscode-v*` tag to equal
`package.json` version, now 0.2.19). The absolute screenshot URL resolves once `main` is pushed; the
definitive proof of image resolution and the links happens on that publish, so the rendered page
should be eyeballed right after the first publish.

---

## P.S. Do not forget the docs of record

On delivery, update both `CHANGELOG.md` and `docs/spec/SPEC.md`. See SPEC.md section 1.3.
