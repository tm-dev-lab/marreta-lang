---
title: "Configure environment variables"
category: how-to
slug: "how-to/configure-environment"
summary: "Set runtime configuration with marreta.env locally and real environment variables in production, and keep secrets out of git."
---

# Configure environment variables

Marreta reads its runtime configuration (the server port, provider hosts,
credentials, feature flags) from `MARRETA_*` environment variables. Locally you keep
them in a `marreta.env` file. In production you set real environment variables. This
page explains how the two relate and how to keep secrets out of git.

## Prerequisites

- A scaffolded project (`marreta init hello`).
- The [Quickstart](../tutorials/quickstart.md) finished.

## marreta.env and marreta.env.example

`marreta init` writes two files:

- **`marreta.env`** holds the real values your machine uses, including secrets. It
  is **gitignored**, so it never leaves your machine.
- **`marreta.env.example`** is a committed template. It lists the same keys with
  placeholder values, so a teammate knows what to set without ever seeing a secret.

The difference is only the secret values. For a database, `marreta.env` has the real
password:

```bash
MARRETA_DB_PROVIDER=postgres
MARRETA_DB_HOST=127.0.0.1
MARRETA_DB_PORT=5432
MARRETA_DB_NAME=marreta
MARRETA_DB_USER=marreta
MARRETA_DB_PASSWORD=marreta
```

while `marreta.env.example` has a placeholder:

```bash
MARRETA_DB_PASSWORD=change-me
```

A new contributor copies the example to `marreta.env` and fills in the real values.

## How a value is resolved

When more than one source sets the same key, Marreta uses this order, highest to
lowest:

1. A CLI flag, such as `--port`.
2. A process environment variable.
3. The `marreta.env` file.
4. The built-in default.

A process environment variable always wins over `marreta.env`. For example, with
`MARRETA_PORT=8080` in `marreta.env`:

```bash
MARRETA_PORT=9091 marreta serve
# the server starts on port 9091, not 8080
```

This is exactly how production works: you do not ship a `marreta.env` file, you set
the real environment variables in your platform, and they take effect because they
outrank the file.

## Read a value in your code

Configuration is also available to your code through the `env` namespace. `env.NAME`
returns the value of an environment variable, read from `marreta.env` with process
variables overriding it, as a string:

```ruby
route GET "/config/region"
    region = env.APP_REGION or "us-east-1"
    reply 200, { region: region }
```

A variable that is not set reads as `null`, so `or` supplies a default and `require` can
demand one before the route runs:

```ruby
require env.STRIPE_KEY else fail 500, "STRIPE_KEY is not configured"
```

Values always come back as strings. Auth providers in [Secure your API](secure-your-api.md)
read their secrets with the same `env.NAME` form, which is how secrets stay out of the
source and in your environment.

## Local services versus external providers

In a fresh project, `marreta.env` points at the local Docker providers on
`127.0.0.1` (see [Persist data with local services](use-local-services.md)). To use
a managed or remote provider instead, set the same keys to the real endpoint:

```bash
MARRETA_DB_HOST=db.internal.example.com
MARRETA_DB_PORT=5432
MARRETA_DB_NAME=catalog_prod
MARRETA_DB_USER=catalog
MARRETA_DB_PASSWORD=...
```

Locally you can put these in `marreta.env`. In production, set them as real
environment variables through your platform's secret management, so no credentials
are written to a file at all.

## Keep secrets out of git

- Never commit `marreta.env`. The scaffold already gitignores it.
- Commit `marreta.env.example` with placeholders, so the required keys are
  documented without exposing values.
- In production, prefer real environment variables over a file, so secrets live in
  your platform's secret store.

## Use it in CI and CD

A pipeline has no `marreta.env` (it is gitignored and never checked out with
secrets). Set the `MARRETA_*` values as environment variables in the pipeline
instead, reading secrets from the platform's secret store. Because process
environment variables outrank the file, the project runs unchanged.

A deploy or test step looks like this:

```bash
export MARRETA_DB_HOST=db.internal.example.com
export MARRETA_DB_USER=catalog
export MARRETA_DB_PASSWORD="$DB_PASSWORD"   # injected from the CI secret store

marreta migrate apply
marreta test
```

Reference secrets as pipeline variables (for example `$DB_PASSWORD` above), never
as literals in the workflow file, and never echo them to the log.

## Try it

```bash
# marreta.env sets MARRETA_PORT=8080, but the process env var wins:
MARRETA_PORT=9091 marreta serve
```

The startup line reports `http://0.0.0.0:9091`.

## Result checkpoint

You should now understand that `marreta.env` holds local values (and is gitignored),
`marreta.env.example` is the committed template, and real environment variables
override the file, which is how you configure a deployed app.

## Common pitfalls

- **A committed secret.** If `marreta.env` is tracked, your credentials are in git
  history. Keep it ignored and use `marreta.env.example` for sharing keys.
- **A change to `marreta.env` does not take effect.** A process environment variable
  with the same name overrides the file. Unset it, or change it instead.
- **A provider is configured but unreachable.** Run `marreta doctor` to see whether
  the host and port actually resolve.

## Next steps

- [Providers](../concepts/providers.md): what the `MARRETA_<concern>_PROVIDER`
  variables select.
- [Configuration reference](../reference/configuration.md): every `MARRETA_*`
  variable and what it controls.
- [Persist data with local services](use-local-services.md): the provider variables
  in context.
