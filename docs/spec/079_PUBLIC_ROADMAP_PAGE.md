# 079 - Public Roadmap Page

> Status: Delivered
> Type: Site (content page on marreta-lang-site), no language or runtime change
> Scope: Add a `/roadmap` page to the site before launch, showing the project's direction in
> undated tiers. It is a positioning artifact, not a commitment, and ships no roadmap item itself.

---

## 1. Purpose

At launch the site has Home, Docs, About, and Initializer, but nothing forward-looking. A new
language needs to show momentum and direction: where it is going, so an evaluator sees a living
project, and so the two strongest theses (AI-ready, and one namespace per concern backed by
swappable providers) are stated as a trajectory, not just current features. The page also gives the
launch post a place to point for "what is next" without over-promising in the post itself.

The risk to avoid is a roadmap that reads as a promise. Project roadmaps with dates rot and become a
liability. This page is explicitly framed as direction shaped by the community, with no dates.

## 2. The change

A single page at `/roadmap` (marreta-lang-site), linked from the nav and the footer, modeled on the
existing `about.astro` (BaseLayout, the `cl-*` component styles). Static content, no data source.

### 2.1 Framing

- **Forward only.** The page is a vision of the future, not a record of the past: it holds only what
  is coming, no "recently shipped" section.
- **Vertical timeline.** Rendered as a vertical timeline (Next then Exploring) so it reads as forward
  motion, without implying dates.
- **No dates.** Items live in tiers by intent, not by calendar.
- **Direction, not promise.** A short lead states the roadmap is the current direction, shaped by
  what the community needs, and may change. Tone consistent with the launch post (honest, no hype).
- Each item is a title plus one or two sentences: the value (what and why), not the how. No design
  detail and no implementation constraints, the full design is each item's own future spec.

### 2.2 Tiers (settled in the design review)

- **Next** — the near-term direction:
  1. **MCP server.** An official Marreta MCP server so an AI assistant can check and self-correct
     against the real toolchain, plus an "Ask Professor Martim" chat on the site.
  2. **Cloud-native messaging providers.** Run `queue.`/`topic.` on a cloud's managed messaging
     without changing a line of application code, only the provider in configuration: Azure Service
     Bus, AWS (SQS for `queue.`, SNS for `topic.`), and Google Cloud (Pub/Sub for `topic.`, Cloud
     Tasks for `queue.`). The showcase of the multi-provider design. (`db`/`doc`/`cache` already run
     on each cloud's managed Postgres, MongoDB, and Redis by pointing the existing provider at them.
     Messaging is the gap because those clouds' native services speak their own protocols, not the
     AMQP the current RabbitMQ provider speaks, so reaching them needs new provider implementations.
     AWS Amazon MQ (managed RabbitMQ) is listed alongside SQS/SNS as an AWS messaging target, as a
     future direction, not a current-support claim: it is AMQP, so it would extend the existing
     provider, but Amazon MQ is TLS-only (`amqps://`) and the provider has no explicit TLS handling
     yet and was never tested against it, so the page does not say it works today.)
  3. **Schema field defaults.** Declare a default on a schema field, filled by the runtime and, for
     persistent schemas, as a database column default tracked by `migrate` (the named follow-up
     deferred from Spec 077).
  4. **More relational providers.** Additional `db.` implementations behind the same namespace:
     MySQL for another production database, and SQLite for zero-setup local development and tests
     (no container to run), making the multi-provider promise concrete on the relational side.
- **Exploring** — under consideration, not committed:
  5. **Scheduled tasks.** First-class recurring/scheduled work (cron-like), a gap for backend
     services today.
  6. **Richer authentication.** OAuth2/OIDC discovery and JWKS rotation, beyond the current API key
     and HMAC JWT.
  7. **Observability exporters.** OpenTelemetry traces and metrics for production readiness.

### 2.3 Placement

- Nav: a fifth item, `Roadmap` to `/roadmap`.
- Footer: a `Roadmap` link in the existing links column.
- The page is picked up by the existing sitemap generation.

## 3. Implementation outline

- **Site only**: `src/pages/roadmap.astro` (new), `src/components/SiteNav.astro` (+1 nav entry),
  `src/components/SiteFooter.astro` (+1 link). No `docs/guide` change (this is a site-native page,
  not synced documentation), no language/runtime/CLI/catalog change.
- Reuse `BaseLayout` and the `cl-*` styles already used by `about.astro`; no new dependency.
- Tests: the site build must pass; no fidelity or unit test applies (static content).

## 4. Out of scope

- **Implementing any roadmap item.** Each (MCP, messaging providers, defaults, MySQL, scheduled
  tasks) is its own future spec through the normal design gate. This spec ships only the page.
- **The messaging-provider design.** Whether AWS is one combined SNS+SQS provider, and the exact GCP
  `queue.` mapping (Cloud Tasks vs a single-subscription Pub/Sub), are deferred to that item's spec.
- **Dates or version commitments.**

## 5. Acceptance criteria

1. `/roadmap` exists, linked from the nav and the footer, built with `BaseLayout` and the shared
   styles.
2. Items are presented in undated tiers; no calendar dates appear; the lead frames it as direction,
   not a promise.
3. The agreed item set and tier placement are present (final set settled in the design review).
4. `npm run build` passes and the page is in the generated sitemap.
5. No language, runtime, CLI, or `docs/guide` change.

## 6. Design decisions (approved 2026-06-15)

- **Item cut.** Scheduled tasks moved to "Exploring" (new capability, design-heavy, "a gap today" is
  a reason to want it, not evidence of near-term); MySQL stays in "Next" (contained second `db.`
  driver, concretizes the multi-provider thesis). Final: Next = MCP, messaging, defaults, MySQL;
  Exploring = scheduled tasks, richer auth, observability. The principle is under-promise: a lean
  Next that ships builds credit.
- **Recently shipped tier: approved in review, then removed by the owner.** The review added it for
  cadence, but the owner's call is that a roadmap is a forward vision, not a record of the past, so
  only Next and Exploring remain. The page is a vertical timeline to convey forward motion.
- **Concrete services named** (Service Bus / SQS / SNS / Pub/Sub / Cloud Tasks): naming concrete tech
  is allowed on a providers/integration page (the "refer to the provider" rule's exception), and the
  deflation lesson was about not boasting or making a contestable comparative claim, not about hiding
  facts.
- **MCP implementation constraint kept off the page.** The "validation is strictly static" rule is a
  security/implementation detail that belongs to the MCP spec, not a positioning roadmap; the page
  states the value, not the internals.
- **Messaging rationale corrected.** Not "RabbitMQ is not a managed cloud service" (false, Amazon MQ
  is managed RabbitMQ), but: the clouds' native messaging services speak their own protocols, not the
  AMQP the current provider speaks, so reaching them needs new provider implementations. Amazon MQ
  (managed RabbitMQ) is named on the page alongside SQS/SNS as an AWS messaging target (owner's
  call), but as a future direction only: the first draft's "already works with the current provider"
  was removed in code review because it was an untested capability claim, and Amazon MQ is TLS-only
  while the provider has no explicit TLS handling yet. The page states intent, not current support.

---

## Delivery notes

Delivered 2026-06-15. A `/roadmap` page on the site (`src/pages/roadmap.astro`, plus a nav and a
footer link), forward-only as a vertical timeline, no dates, with a "direction, not a promise" lead.
Next: MCP server; cloud-native messaging (Azure Service Bus, AWS SQS/SNS and Amazon MQ as a future
target, Google Cloud Pub/Sub and Cloud Tasks); schema field defaults; more relational providers
(MySQL, and SQLite for zero-setup local development). Exploring: scheduled tasks, richer auth,
observability. Site-only, no language/runtime change.

Two gates passed (design and code review). Code review took two rounds on the Amazon MQ line: the
first draft's "already works with the current provider" was an untested capability claim (and Amazon
MQ is TLS-only while the provider has no explicit TLS handling yet), so it was removed; Amazon MQ
stays as a future AWS messaging target only, with TLS verification deferred to that item's own spec.
Site build and the init fidelity tests are green.

---

## P.S. Do not forget the docs of record

On delivery, update both `CHANGELOG.md` and `docs/spec/SPEC.md`. See SPEC.md section 1.3.
