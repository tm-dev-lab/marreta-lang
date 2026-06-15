---
title: "CLI"
category: runtime
slug: "reference/cli"
summary: "The marreta command-line interface: build and run, migrate, format, lint, and editor tooling."
---

# CLI

The `marreta` command runs and inspects a project. The everyday commands are `init`,
`serve`, and `test`.

## Build and run

| Command | Purpose |
|---|---|
| `marreta init <path> [--with db,cache,doc,queue] [--no-agents]` | Scaffolds a new project, adding the chosen providers and a `docker-compose.yml`. `--no-agents` skips the AI-agent guide. |
| `marreta serve` | Serves the project over HTTP on `MARRETA_PORT` (8080 by default). |
| `marreta test [path] [--filter TEXT] [--coverage]` | Runs the scenario tests, optionally filtered, with `--coverage` for a report. |
| `marreta agents` | Writes or refreshes the AI-agent guide (`AGENTS.md` and its pointers) for the current project. See [Use AI assistants](../how-to/use-ai-assistants.md). |
| `marreta doctor` | Loads the project and reports its configuration without serving. |

```bash
marreta init shop --with db,cache
cd shop
marreta serve
```

## Database

`marreta migrate <subcommand>` manages relational migrations:

| Subcommand | Purpose |
|---|---|
| `generate` | Writes a migration from the current `db:` schemas. Reports unsupported changes (a type change, a removed field) as drift instead of writing them. |
| `diff` | Shows the SQL a new migration would contain, without writing it. Also reports unsupported changes as drift. |
| `status` / `list` | Show applied and pending migrations. |
| `explain <state>` | Explains a migration state (`pending`, `changed`, `missing_local`, `workflow`). |
| `apply` | Applies pending migrations and changes the database. |
| `rollback` | Reverts the most recent applied migration. |
| `discard <version>` | Removes an unapplied local migration. |

See [Evolve your database with migrations](../how-to/migrations.md).

## Code quality

| Command | Purpose |
|---|---|
| `marreta fmt [--check]` | Formats every `.marreta` file the project loads (the same files `serve` and `test` read, in any folder). `--check` verifies formatting without writing. |
| `marreta lint [--strict]` | Lints the project for problems, with `--strict` to fail on warnings. |

## Editor tooling

`marreta tooling <catalog, symbols, completions, hover, definition> --format json`
powers editor features. It is consumed by the VS Code extension, not run by hand.

## Notes

- `serve` and `doctor` load the project the same way, so `doctor` catches a
  misconfiguration before you deploy.
- `fmt` and `lint` also read from stdin (`--stdin --file <path>`) for editor
  integration.
