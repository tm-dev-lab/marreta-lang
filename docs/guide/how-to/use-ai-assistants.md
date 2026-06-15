---
title: "Use AI assistants"
category: how-to
slug: "how-to/use-ai-assistants"
summary: "Give AI coding assistants correct Marreta context with the generated AGENTS.md primer and the llms.txt reference, so the model writes Marreta instead of guessing."
---

# Use AI assistants

Most code today is written with an AI assistant in the editor. Marreta is a new, focused
language, so a model that has never seen it tends to guess, falling back to Python or
JavaScript shapes that do not run. Marreta closes that gap by putting correct, authoritative
context where assistants already look, generated from this documentation and the language's
own tested examples so it never drifts from the runtime.

## In your project: AGENTS.md

`marreta init` writes an `AGENTS.md` at the project root. `AGENTS.md` is the cross-tool
convention that agentic assistants load as in-context instructions. Marreta's primer leads
with the corrections a model most often needs (the things it gets wrong coming from another
language), then a compact syntax cheat, then a pointer to the full reference.

Alongside it, `init` writes a thin pointer for GitHub Copilot, which reads its own
`.github/copilot-instructions.md` rather than `AGENTS.md`. It points back to `AGENTS.md`,
so there is a single source of truth.

To scaffold without the agent guide, pass `--no-agents`:

```bash
marreta init shop --no-agents
```

## Refresh after upgrading

The primer is stamped with the runtime version it was generated for. After upgrading the
runtime, regenerate it for the current project:

```bash
marreta agents
```

That rewrites `AGENTS.md` and the pointers from the installed runtime. `marreta doctor`
reports when the file is behind, without ever changing it:

```text
Agent guide:
  SKIP  AGENTS.md is stamped for 0.1.0, runtime is 0.2.0. Run `marreta agents` to refresh
```

## On the web: llms.txt

Some assistants fetch documentation at query time instead of reading a project file. For
them, the site publishes two files at its root:

- [`marreta.dev/llms.txt`](https://marreta.dev/llms.txt) is a curated index of the
  documentation, one line per page with a short description.
- [`marreta.dev/llms-full.txt`](https://marreta.dev/llms-full.txt) is the full reference,
  every guide page concatenated so a model can read it in one fetch.

Both are generated from this same documentation, so they stay current as the language grows.
