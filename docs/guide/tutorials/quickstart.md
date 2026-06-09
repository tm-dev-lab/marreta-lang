---
title: "Quickstart"
category: tutorials
slug: "tutorials/quickstart"
summary: "Install Marreta, scaffold a project, and serve your first endpoint in a couple of minutes."
---

# Quickstart

This is the five-minute tour: install the runtime, scaffold a project, and hit a live
endpoint. By the end you will have a running API and know where everything lives.

## Prerequisites

- A terminal on macOS, Linux, or Windows via WSL.
- `curl`, to install the runtime and to call the running API.

## 1. Install

```bash
curl -fsSL https://raw.githubusercontent.com/tm-dev-lab/marreta-lang/main/install.sh | sh
```

That drops a single `marreta` binary on your `PATH`. Check it:

```bash
marreta --version
```

## 2. Scaffold a project

```bash
marreta init hello
cd hello
```

`init` writes a tiny but complete project: an entrypoint, one schema, one task, one
route, and a test.

```
hello/
  app.marreta              # project metadata (name, version, required runtime)
  schemas/greetings.marreta
  tasks/greetings.marreta
  routes/greetings.marreta
  tests/greetings_test.marreta
  marreta.env              # local config (gitignored)
```

The route it generates is the whole story in four lines:

```ruby
route GET "/greetings"
    message = greetings.build_greeting("Marreta")
    reply 200 as GreetingResponse, { message: message }
```

`greetings.build_greeting` is a task defined in `tasks/greetings.marreta` and called by
its file's namespace, with no imports. `reply 200 as GreetingResponse` shapes and
validates the response against the schema.

## 3. Serve it

```bash
marreta serve
```

The server comes up on port `8080` by default. In another terminal:

```bash
curl http://localhost:8080/greetings
```

```json
{ "message": "Hello, Marreta!" }
```

That is a real, schema-validated HTTP response, with no framework wiring and no
boilerplate.

## 4. Run the tests

The scaffold ships a scenario test that exercises the endpoint end to end:

```bash
marreta test
```

Scenario tests live next to your code under `tests/` and read like the behavior they
check (`when GET "/greetings"` … `then response is { ... }`), so they double as
executable documentation.

## 5. Check the project's health

```bash
marreta doctor
```

`doctor` loads the project the same way `serve` does and reports what it found
(configuration, persistence, modules, test coverage) without starting the server. It is
the fastest way to catch a misconfiguration before you deploy.

## Result checkpoint

You should now have Marreta installed, a scaffolded `hello` project, and a running API
that answers `GET /greetings` with a schema-validated JSON response. `marreta test`
passes and `marreta doctor` reports a healthy project.

## Where to go next

- **[Validate a request payload](../how-to/validate-a-payload.md)**: contract the input
  to a route.
- **[Persist data with local services](../how-to/use-local-services.md)**: add a
  database and read and write records.
- **[Configuration](../reference/configuration.md)**: the `marreta.env` variables and
  what they control.

You did not configure a database, a router, or a serializer to get here, and that is the
point. Add capability only when you need it, and Marreta keeps the rest out of your way.
