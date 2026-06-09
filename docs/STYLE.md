# Documentation style guide

This guide keeps the prose in `docs/guide` in one voice as it grows. It is for
people writing docs, not for end users, and it lives outside `docs/guide` so it is
not part of the published guide. The
[Quickstart](guide/tutorials/quickstart.md) is the reference example. When in
doubt, match its tone.

## Voice

- **Concrete and warm, never marketing.** Show a real command, a real response,
  and the reason behind it in one sentence ("no imports", "schema-validated", "no
  boilerplate"). Sell the idea by demonstrating it, not by adjectives.
- **Lead with the practical path.** A reader should be able to copy a block, run
  it, and see the same output. Explanation comes after the working example.
- Keep sentences short. Prefer the active voice.
- Confident, contextual claims like "no framework wiring, no boilerplate" belong
  in the Quickstart and introductory tutorials. Do not repeat them on technical
  reference or how-to pages, where they read as filler.

## Punctuation

- **No em-dashes.** Use a comma, parentheses, or two sentences instead.
- **No semicolons.** Split the sentence in two.
- **No emojis.** Reference documentation stays plain.

## Vocabulary

- **Marreta Lang** for the full name, on the first mention of a page or in
  institutional context.
- **Marreta** in the body of the prose after the first mention.
- **`marreta`** (code font) only for the binary and its commands
  (`marreta serve`, `marreta init`).
- **Second person ("you")** in tutorials and how-to guides. Avoid "we" except in
  opinionated recommendations such as "we recommend".
- **scenario test** is the standard term for the tests under `tests/`. Define it
  on its first occurrence in each tutorial or how-to section, as the Quickstart
  does.
- **local services** are the Docker and Compose containers a project runs against
  (`marreta init ... --with db,cache`).
- **provider** is the selectable backend behind a namespace (`db`, `doc`, `cache`,
  messaging). Marreta abstracts each concern behind a provider, and there may be
  several per concern over time. In prose, refer to "the database provider", "the
  cache provider", and so on, **not** the technology directly. Name the current
  provider (PostgreSQL, MongoDB, Redis, RabbitMQ) only on the
  [Providers](guide/concepts/providers.md) page or in literal config values.

## Examples must be real

Every code example must come from, or be verified against, the tested example
suite under `docs/examples`. Lift schema and route snippets from those files when
it makes sense, so a reader who copies a block gets working code. A documented
example that does not run is worse than no example.

## Code fences

- **Marreta code uses a `ruby` fence.** GitHub has no Marreta highlighter, and
  Ruby highlighting renders Marreta well. This matches the root `README.md`.
- **Shell commands use a `bash` fence.**
- **JSON bodies use a `json` fence.**
- **Generated SQL uses a `sql` fence, and command output uses a `text` fence.**

## Page structure (tutorials and how-to)

Every practical page follows the same skeleton, so a reader always knows where to
look:

1. **Prerequisites** at the top. A short list of what the reader needs before
   starting (a scaffolded project, running local services, a prior page). Keep it
   to the essentials.
2. The body, **one concept per page**. A how-to does one thing. Do not mix cache,
   database, auth, and queue in a single flow. If a recipe needs another
   capability, link to its page instead of teaching it again.
3. **"Try it"**: a runnable block the reader can paste verbatim to see the result.
4. **Result checkpoint**: end with "You should now have ...", so the reader can
   confirm success.
5. **Next steps**: two or three links, never a long list.

## Page patterns

- **Copy-paste-safe examples.** Every code block either runs as shown or states
  clearly that it is a fragment. Never leave a reader guessing whether a block is
  complete.
- **"When to use"** opens the introduction of a namespace reference page (`db`,
  `doc`, `cache`, `queue`, `topic`, `http_client`) so a reader knows in one line
  whether this is the tool they need.
- **"Common pitfalls"** is a short list, only when there is a real, common error,
  such as Docker not running, an unmet `requires_marreta`, a file-namespace
  mismatch, or `db:` relation validation. Skip it when there is nothing real to
  warn about.
- **"Troubleshooting"** is a short section on pages that touch infrastructure
  (databases, caches, queues), covering the failures a reader actually hits.
- **Version note.** Mention a minimum runtime only when the feature depends on
  `requires_marreta` or on recent behavior. Do not annotate stable features.
- **Cross-links.** A tutorial points to the relevant how-to, a how-to points to
  the reference, and an explanation points to the conceptual decision behind it.

## Generated reference vs. authored prose

Reference pages under `docs/guide/reference` mix generated and authored content.

- The text inside the `BEGIN GENERATED` and `END GENERATED` markers is produced
  by `marreta tooling docs`. It is factual and dry. Never edit inside the markers.
  Your change will be overwritten on the next regeneration, and the docs gate will
  flag it as stale.
- The human voice, meaning the introduction, "When to use", gotchas, and examples,
  goes before or after the generated block, never inside it.
