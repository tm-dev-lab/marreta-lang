# 075 - Contribution Flow (Tiered SDD)

> Status: Delivered
> Type: Governance (repo docs + issue templates), no code
> Scope: Give the repo a public contribution flow before launch: a tiered model (trivial
> changes go straight to PR; substantial changes go proposal-first, spec-driven; bugs go
> through a structured bug issue), carried by a root `CONTRIBUTING.md`, GitHub issue forms, a
> public canonical spec template, and a private security-reporting path. Tier model follows
> the Rust RFC / Python PEP precedent: not every change is an RFC. Verified against the repo
> on 2026-06-12: no `CONTRIBUTING.md`, no `SECURITY.md`, no `.github/ISSUE_TEMPLATE/` (blank
> issues are enabled), `PULL_REQUEST_TEMPLATE.md` exists, conventions live at
> `docs/guide/reference/conventions.md` (not at the root).

---

## 1. Purpose

The README carries build gates and contribution guidelines, but there is no flow: nothing
tells a stranger how a change gets accepted, issues are free-form, and the spec discipline
that drives the project is invisible to an outside contributor. At launch, the first
contributors arrive with no rails, and the most painful failure mode is predictable: a large
PR, written in good faith, refused for scope or for skipping design review.

The fix is the tiered, spec-driven flow the project already practices internally, made
public, with the barrier proportional to the change.

## 2. The change

### 2.1 The tiered model

- **Trivial** (typo, doc wording, an obvious small bug fix with no design decision): direct
  fork-and-PR. Zero barrier. The PR template's checklist still applies where relevant.
- **Substantial** (language semantics or syntax, CLI surface, a new provider or namespace,
  error or event-log contract changes, anything user-visible by design): **proposal first**.
  Open a proposal issue (problem, motivation, proposed change, how it fits the language's
  focused scope, alternatives considered). Maintainers review it; an accepted proposal
  becomes a spec under `docs/spec/` following the public template (2.4), and only then does
  implementation proceed via fork-and-PR. Internal gate names and tooling are not exposed:
  the public wording is "maintainer review" at both points (proposal/spec, and PR).
- **Bug**: a structured bug issue (version from `marreta --version`, platform, minimal
  `.marreta` reproduction, expected vs actual, relevant logs). Triage decides: a plain fix
  goes to PR; a fix that implies a design decision escalates to a proposal.

The concrete examples above are part of the text, so "substantial" is defined by enumeration
rather than vibes (the Rust RFC document does the same).

### 2.2 Root `CONTRIBUTING.md`

Location decision: the **repo root**, not `docs/guide/`. GitHub surfaces a root
`CONTRIBUTING.md` automatically when someone opens an issue or PR, and `docs/guide/` is the
product documentation mirrored to marreta.dev (how to use the language); governance there
would pollute it. The guide is linked from CONTRIBUTING (style and conventions live at
`docs/guide/reference/conventions.md`), not the other way around.

Content outline:

1. The tiered model (2.1), leading with the trivial tier so small contributions read as
   welcome.
2. **Scope expectations, explicit**: Marreta is focused by design (the README section is
   referenced); a proposal outside the focused DSL scope can be declined for scope alone,
   and the proposal stage exists precisely so that happens before code is written, not
   after.
3. The development loop: prerequisites and the pre-PR gates, moved from the README
   (`cargo fmt --check`, `clippy -D warnings`, `cargo test`, release build, plus the
   relevant example suites for runtime-affecting changes). The README's Contributing section
   shrinks to a short pointer at CONTRIBUTING.md.
4. A pointer to the PR template's checklist (docs DoD) rather than a duplicate of it.
5. **License clause**: contributions are MIT-licensed, inbound equals outbound. One line, no
   CLA.
6. **Review expectations, honest**: a small maintainer team, review latency in days; silence
   is bandwidth, not rejection.

### 2.3 Issue forms (`.github/ISSUE_TEMPLATE/`)

GitHub issue forms (YAML), not markdown templates, so fields can be required:

- **`proposal.yml`**: problem, motivation, proposed change, scope fit (one question that
  echoes the focused-by-design clause), alternatives considered. A light mirror of the spec
  template (2.4), so an accepted proposal converts into a spec without re-asking everything.
- **`bug_report.yml`**: `marreta --version` output, platform (Linux, macOS, or Windows via
  WSL), minimal `.marreta` reproduction, expected vs actual, logs. A required checkbox: "this
  is not a security issue" pointing security reports at the private path (2.5).
- **`config.yml`**: `blank_issues_enabled: false`; contact links route questions to GitHub
  Discussions and security reports to the private advisory channel.

### 2.4 Public canonical spec template: `docs/spec/TEMPLATE.md`

The spec format is de facto today (consistent across the corpus, but defined nowhere a
contributor can see). Extract it into `docs/spec/TEMPLATE.md` as the canonical template:
Status/Type/Scope header, Purpose, The change, Implementation outline (with the coverage
analysis), Out of scope, Acceptance criteria, and the docs-of-record reminder (CHANGELOG +
SPEC.md, per SPEC.md section 1.3). Single source: the internal process and the external proposal
flow both point at it, and `proposal.yml` mirrors its top sections.

Freeze the **current** canonical format, the one the recent specs (071-074) use. Do not reconcile
with the older pre-064 layout (for example Spec 024's Motivation/Goals/Non-goals/Required
Semantics, a format that has since evolved). Two requirements from the design review:

- The template is a **fillable skeleton with one line of guidance per section** (what goes in each),
  not bare headers. An outside contributor converting a proposal needs to know what to write,
  especially in "Coverage analysis", which is not obvious from outside.
- The **Coverage analysis names its three axes explicitly** (VS Code extension, e2e, documentation),
  because that is precisely what `proposal.yml` mirrors and what keeps the coverage discipline alive.
  It is the part that most distinguishes this process from a generic template.

### 2.5 `SECURITY.md` and private vulnerability reporting

The most serious gap in the launch posture: there is no stated security-reporting path, so
the default is a public issue, which for a language runtime is instant image damage. Add a
root `SECURITY.md`: report privately via GitHub private vulnerability reporting (enable it
in repo settings, a delivery-checklist item), with `contact@marreta.dev` as the
contact fallback (owner decision on pull), what to include, response expectation, and no
public issue for suspected vulnerabilities. The bug form and `config.yml` both point at it.

### 2.6 `CODE_OF_CONDUCT.md` (owner decision: adopted)

Owner decision on pull (2026-06-12): **adopt** the Contributor Covenant. Add a root
`CODE_OF_CONDUCT.md` using the standard Contributor Covenant text, with the enforcement contact
set to `contact@marreta.dev`. GitHub's community profile stops flagging its absence and
launch audiences find it.

Consequence registered (design review, decision kept): this email then sits on two public pages
(`CODE_OF_CONDUCT.md` and `SECURITY.md`), which spam bots scrape and which do not rotate without
editing the files. Acceptable for a solo launch and it is the owner's call. If hardening is wanted
later, a dedicated alias swaps both files with no other change.

### 2.7 Improve the pull request template

`CONTRIBUTING.md` makes `.github/PULL_REQUEST_TEMPLATE.md` the canonical per-PR requirement, but
the current template reflects an earlier stage. It covers docs, the gate tiers, and the
docs-of-record sync, but it misses what the recent specs (071-074) taught: the change tier and the
design gate, the coverage analysis as a first-class prompt (the three axes now named in
`TEMPLATE.md`), functional coverage of the new behavior (not only no-regression), e2e for language
features, and the do-not-weaken-tests rule. Rework it to encode the process the project actually
runs.

The hard constraint is the tiered model (2.1): a trivial change has zero barrier, so the template
must not bury a one-line fix under a long checklist. The guiding principle from the design review:
the template is a **prompt and a hook for the maintainer, not an enforcement mechanism**. It cannot
stop a false tick; it makes the expectation visible and gives the maintainer the artifact to ask
for.

**Design decisions (resolved in review):**

- **Single template** (not multiple). GitHub auto-applies only the root `PULL_REQUEST_TEMPLATE.md`;
  multiple templates under `.github/PULL_REQUEST_TEMPLATE/` are reachable only by a `?template=` URL
  no one discovers. The escalation lives in the content: "Type of change" near the top, and the
  trivial path is visually first and explicitly terminable, guided by an HTML comment (which
  disappears in the render, so it does not pollute the PR description):
  `<!-- Trivial change (typo, doc, small fix)? Fill Summary + Type, delete the rest. -->`
- **Coverage analysis: one line per axis with a mandatory reason on the negative case**, not a
  checkbox. The real analysis lives in the spec; in the PR it is a confirmation gate. A checkbox
  "not affected" is the reflexive tick that kills the discipline. A line that forces a reason on the
  negative defeats the tick without demanding a paragraph:
  `Extension: updated in <file>, or not affected because <reason>`. Substantial path only (a trivial
  PR skips all three axes).
- **The design gate is a field, not a checkbox**: `Linked: #___` under the heading "Linked issue,
  proposal, or spec". An empty field on a substantial PR is an immediate signal to bounce
  ("substantial change with no linked proposal, open the proposal first"), which a checkbox cannot
  carry. The field names all three traceability artifacts so it serves every tier below the trivial
  cut: a substantial change links its spec, a non-trivial bug fix links its bug issue. Firm wording
  without a lecture: "Substantial changes need an approved spec first (see CONTRIBUTING). Bug fixes
  link their bug issue."
- **A conditional live-proof line on the substantial path**: "If this change only proves live (a
  published page, a rendered form, a release artifact), note how and when it is verified
  post-merge." Niche for most PRs but zero cost (skipped by those who do not need it), against a bug
  class that bit this project repeatedly (the 074 screenshot, the 075 forms, the 070 `--latest`):
  the "local check is not the live proof" pattern, institutionalized.

**Two consistency fixes:** the three axes use the exact names the frozen `TEMPLATE.md` uses
(extension / e2e / documentation), so a contributor sees one vocabulary in the spec and the PR; and
the records section is named **Docs of record** (CHANGELOG + SPEC.md), the term the house uses.

**Final structure:** Summary (always) -> Type of change (always, selects the path) -> [trivial stops
here] -> Linked issue/proposal/spec -> Tests (with functional coverage of the new behavior, not only
no-regression) -> Coverage analysis (three axes, line-with-reason) -> Documentation -> Quality gates
-> Docs of record -> the conditional live-proof line.

## 3. Implementation outline

- Root: `CONTRIBUTING.md` (2.2), `SECURITY.md` (2.5), `CODE_OF_CONDUCT.md` (2.6, adopted).
- `.github/ISSUE_TEMPLATE/`: `proposal.yml`, `bug_report.yml`, `config.yml` (2.3), valid per
  GitHub's issue-forms schema.
- `docs/spec/TEMPLATE.md` (2.4).
- `.github/PULL_REQUEST_TEMPLATE.md`: the reworked, escalated template (2.7).
- `README.md`: Contributing section shrinks to a pointer (move the prerequisites, layout, gates, and
  guidelines into `CONTRIBUTING.md`, keep one paragraph plus a link).
- **Repo settings (delivery checklist, not files), with three design-review conditions:**
  - **Mechanism:** `gh`, because it is auditable (the exact command goes in the delivery notes):
    `gh api` to enable private vulnerability reporting, `gh repo edit --enable-discussions` for
    Discussions.
  - **Hard ordering:** Discussions must be enabled before or together with the `config.yml` merge,
    otherwise there is a window where the issue-chooser contact link 404s (the same principle as the
    070 "publish before the listing exists").
  - **Verification, not assumption:** confirm the state actually changed by reading it back
    (`gh api` on `security_and_analysis` and `has_discussions`), not by trusting the command fired.
    The AC requires the confirmed state.
  - **Credentials:** these are owner-account actions on the live public repo (outward-facing config).
    The owner runs the `gh` commands (the `!` prefix in-session) or explicitly authorizes them. The
    agent does not fire them on its own.
- Cross-repo note: the site can link CONTRIBUTING on GitHub from its footer or docs page;
  that is a site-repo follow-up, not this spec.

### Test requirements (house standards)

Governance docs, no code: the gates are document-level. Issue forms validate against
GitHub's schema (lint the YAML; render check on a fork or after merge as the functional
proof, recorded in delivery notes). Links checked (every referenced path exists:
conventions page, PR template, SPEC.md section 1.3). English-only, docs punctuation rules
(no em-dashes, no semicolons, no emojis), WSL named alongside Linux/macOS wherever platforms
are listed (the bug form does this).

### Coverage analysis (spec protocol)

- **VS Code extension**: none.
- **e2e**: none.
- **Documentation**: this spec is documentation; the product guide (`docs/guide/`) is
  deliberately untouched except that CONTRIBUTING links INTO it (conventions). No
  `SUMMARY.md` change.

## 4. Out of scope

- A CLA or DCO bot (inbound = outbound MIT line covers it at this scale).
- Governance beyond contribution (decision records, maintainer ladder): premature.
- Automating proposal-to-spec conversion: the maintainer does it by hand with TEMPLATE.md.
- The site-side link to CONTRIBUTING (cross-repo follow-up).

## 5. Acceptance criteria

1. `CONTRIBUTING.md` at the root carries the tiered model with enumerated examples per tier,
   the explicit focused-by-design scope clause, the development gates moved from the README,
   the PR-template pointer, the MIT inbound-equals-outbound line, and the review-expectation
   line. The README Contributing section is a pointer.
2. `.github/ISSUE_TEMPLATE/` ships `proposal.yml`, `bug_report.yml` (with version, platform
   including WSL, minimal repro, expected vs actual, and the security checkbox), and
   `config.yml` with blank issues disabled and links to Discussions and the security channel.
   (Render proof is AC7.)
3. `docs/spec/TEMPLATE.md` exists and freezes the current canonical format (the 071-074 layout, not
   the pre-064 one). It is a fillable skeleton with one line of guidance per section, and its
   Coverage analysis names the three axes explicitly (VS Code extension, e2e, documentation). It
   includes the docs-of-record reminder, and `proposal.yml` mirrors its top sections.
4. `SECURITY.md` exists with `contact@marreta.dev` as the contact, no template or doc routes
   a suspected vulnerability to a public issue, and private vulnerability reporting is **confirmed
   enabled** by reading the repo state back (not just firing the command), recorded in delivery
   notes.
5. The `CODE_OF_CONDUCT.md` decision is made and recorded: adopted (Contributor Covenant, contact
   `contact@marreta.dev`).
6. Discussions is **confirmed enabled** (read back via `gh api has_discussions`) before or together
   with the `config.yml` merge, so the issue-chooser contact link never 404s. The repo-settings
   actions are owner-run or owner-authorized, with the exact `gh` commands in the delivery notes.
7. All texts follow the house prose rules and are in English; every internal link resolves. The
   issue forms render correctly on GitHub (post-merge proof, like the 074 screenshot, since YAML
   forms only render live on GitHub), recorded in delivery notes.
8. `.github/PULL_REQUEST_TEMPLATE.md` is reworked to the escalated shape (2.7): a trivial change
   fills only summary and type, while the substantial path prompts the design gate, the coverage
   analysis (three axes), functional coverage of new behavior, e2e for language features, and the
   do-not-weaken-tests rule, keeping the existing docs/gates/records sections.

---

## Delivery notes

Delivered. Governance docs only, no code. Document-level gates: the three issue forms parse as valid
YAML (`js-yaml`), every internal link resolves, and the prose follows the house rules (English, no
em-dashes, no semicolons, no emojis).

What landed:

- `CONTRIBUTING.md` (root): the tiered model (trivial straight to PR, substantial proposal-first and
  spec-driven, bug through a structured report), the focused-by-design scope clause, the development
  prerequisites/layout/gates moved from the README, the PR-template pointer, the MIT
  inbound-equals-outbound line, and honest review expectations.
- `SECURITY.md`: private vulnerability reporting (GitHub advisory plus `contact@marreta.dev`),
  no public issue for suspected vulnerabilities.
- `CODE_OF_CONDUCT.md`: the verbatim Contributor Covenant 2.1 (owner decision: adopted), fetched from
  contributor-covenant.org with the contact set to `contact@marreta.dev`. The earlier
  paraphrase was replaced after review because adopting a recognized document only has value if it is
  recognizable.
- `.github/ISSUE_TEMPLATE/`: `proposal.yml` (mirrors the spec template's top sections),
  `bug_report.yml` (version, platform including WSL, minimal repro, expected vs actual, not-a-security
  checkbox), `config.yml` (blank issues disabled, links to Discussions and the private advisory).
- `docs/spec/TEMPLATE.md`: the canonical spec format frozen (the 071-074 layout), a fillable skeleton
  with per-section guidance and the three named coverage axes (extension, e2e, documentation).
- `.github/PULL_REQUEST_TEMPLATE.md`: reworked to the escalated shape that distills what specs 071-074
  taught. Single auto-applied template, trivial fills Summary + Type and stops (HTML comment guide).
  Substantial path: a `Linked issue, proposal, or spec` field (the design gate as a field, an empty
  one bounces, covering all three tiers), Tests with functional coverage of new behavior and e2e for
  language features and do-not-weaken-tests, Coverage analysis as one line per axis with a mandatory
  reason on the negative (defeats the reflexive tick), and a conditional live-proof line. Vocabulary
  aligned with `TEMPLATE.md`.
- `README.md`: the Contributing section shrunk to a pointer.

Repo settings (owner-run via `gh`, executed and confirmed by read-back before merge, the
order-gate condition):

```bash
gh repo edit tm-dev-lab/marreta-lang --enable-discussions
gh api repos/tm-dev-lab/marreta-lang --jq .has_discussions          # -> true
gh api -X PUT repos/tm-dev-lab/marreta-lang/private-vulnerability-reporting
gh api repos/tm-dev-lab/marreta-lang/private-vulnerability-reporting --jq .enabled  # -> true
```

Post-merge proofs (live-only, the class this spec's own template now flags): the issue-form chooser
shows both forms plus the two contact links resolving, and the next PR carries the new template.

---

## P.S. Do not forget the docs of record

On delivery in marreta-lang, update both `CHANGELOG.md` and `docs/spec/SPEC.md` (see SPEC.md
section 1.3), and renumber this file into `docs/spec/` with the next free number.
