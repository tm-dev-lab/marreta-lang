<div align="center">

<img src="docs/assets/brand/images/mascot_original.png" alt="Martim, the Marreta Lang mascot" width="180">

# Marreta Lang

**Zero Ceremony REST APIs.**

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![web: marreta.dev](https://img.shields.io/badge/web-marreta.dev-7c3aed.svg)](https://marreta.dev)

</div>

Marreta Lang is a focused DSL for building REST APIs, designed for good developer
experience, low resource usage, and performance-conscious runtime behavior.

A large part of backend work is still about REST APIs that receive data, validate
it, apply business rules, call common infrastructure, and return clear responses.
Many of those services matter, but they don't always need a pile of architectural
ceremony. Marreta Lang focuses on exactly that space, so you write business
behavior instead of repetitive framework wiring:

```ruby
route GET "/accounts/:id"
    account = doc.accounts.find(params.id)
    require account else fail 404, "account not found"

    reply 200, account
```

No imports, no framework setup, no connection boilerplate. The route reads like
the behavior it describes.

## Origins

Marreta Lang began as a research project at [TM Dev Lab](https://www.tmdevlab.com),
exploring whether a new programming language could validate three hypotheses at once:

1. **Low resource usage**, with efficient consumption especially in containerized
   environments.
2. **Strong performance**, even as an interpreted DSL with a high level of
   abstraction.
3. **Good developer experience**, so building APIs that integrate with SQL and
   NoSQL databases, messaging, cache, HTTP services, logging, and OpenAPI docs no
   longer means wiring up the same infrastructure, protocol, and integration
   boilerplate every time, all with zero project dependencies.

The idea was for the language to work in terms of building blocks. Integration
components such as databases, message buses, and caches are exposed as
**providers**: high-level abstractions that hand the developer a native language
namespace, one block per concern, for using each component without implementing
any of the integration. You choose a provider and set its connection details such
as URL, user, and password in the `marreta.env` file, and once the project starts
you simply use it, with no imports or extra wiring.

A provider can have many implementations, which means the same namespace could be
backed by different technologies, for example PostgreSQL or MySQL for the
relational database. In the project's current state each namespace ships with a
single provider: PostgreSQL for the relational database, MongoDB for the document
(NoSQL) database, Redis for the cache, and RabbitMQ for messaging (point to point
and pub/sub).

After several rounds of experiments the results validated all three. You can read
more about them at [marreta.dev](https://marreta.dev).

## Why Marreta?

*Marreta* is the Brazilian Portuguese word for a sledgehammer, and the name is
kept in Portuguese on purpose: it preserves the project's Brazilian origin rather
than hiding it behind a generic English brand. The metaphor is also the point, a
focused tool that breaks through boilerplate to reach the code that actually
describes the API behavior.

**Zero Ceremony** doesn't mean hiding behavior. It means removing repetitive setup
and glue code while keeping the API behavior visible in the source. Common REST
building blocks are first-class language and CLI concepts (routes, schemas,
tasks, database, cache, queues, auth, tests, generated docs, formatting, and
linting), so you go from business intent to a running endpoint fast, without
giving up clarity, efficiency, or tooling.

- **Great DX.** Routes, validation, responses, tests, docs, and infrastructure
  live in one coherent project model with a simple CLI loop.
- **Low footprint.** A native runtime binary with predictable resource usage.
  Infrastructure is configured via the environment, not wired in code.
- **Performance-minded.** Implemented in Rust, with the HTTP hot path measured by
  benchmarks and guided by runtime profiling rather than guesswork.
- **Focused by design.** Built for REST APIs, not as a general-purpose language.

## Installation

Marreta Lang is distributed as a standalone runtime binary. The quickest way to
install the latest release is the one-line installer, which detects your platform,
downloads the matching binary, installs it to `~/.local/bin`, and verifies it:

```bash
curl -fsSL https://raw.githubusercontent.com/tm-dev-lab/marreta-lang/main/install.sh | sh
```

To install a specific version, pass the tag:

```bash
curl -fsSL https://raw.githubusercontent.com/tm-dev-lab/marreta-lang/main/install.sh | sh -s -- v0.2.0
```

You can read the script before running it at
[install.sh](https://github.com/tm-dev-lab/marreta-lang/blob/main/install.sh).
The installer never edits your shell profile. If `~/.local/bin` is not on your
`PATH`, it prints the line to add. Set `MARRETA_INSTALL_DIR` to install elsewhere.

Prefer to install by hand? Download the binary for your platform from
[GitHub Releases](https://github.com/tm-dev-lab/marreta-lang/releases), make it
executable, and put it on your `PATH`:

```bash
chmod +x marreta
mv marreta ~/.local/bin/marreta
marreta --version
```

Supported environments: Linux, macOS, and Windows via WSL (native Windows is not
part of the first distribution target). If `~/.local/bin` is not on your `PATH`,
add it to your shell profile.

## Quickstart

```bash
marreta init hello-api      # scaffold a project
cd hello-api
marreta serve               # run the API at http://localhost:8080
```

Open <http://localhost:8080/greetings> and you have a running REST API, with
interactive OpenAPI docs (Swagger UI) served at <http://localhost:8080/docs>.

The everyday commands:

| Command | What it does |
| --- | --- |
| `marreta init <name>` | Scaffold a new project (add `--with db,cache,doc,queue` for local services) |
| `marreta serve` | Run the API (default `:8080`) |
| `marreta test` | Run scenario tests |
| `marreta migrate` | Generate and apply relational database migrations |
| `marreta doctor` | Validate project structure, config, and connectivity |
| `marreta fmt` / `marreta lint` | Format and lint the source |

## A Taste of the Language

Validate a request against a schema and persist it:

```ruby
schema NewAccount
    owner: string

route POST "/accounts" take payload as NewAccount
    account = doc.accounts.save({ owner: payload.owner, balance: 0 })

    reply 201, account
```

Read through a cache, falling back to a query pipeline on a miss:

```ruby
route GET "/products/active"
    cached = cache.get("products:active")
    if cached
        reply 200, cached

    products = db.products
    >> where(active: true)
    >> order_by("name asc")
    >> fetch

    response = { products: products }
    cache.set("products:active", response, ttl: 60)
    reply 200, response
```

Write atomically in a transaction, then hand the work off to a queue:

```ruby
route POST "/orders" take payload as NewOrder
    transaction
        order = db.orders.save({ customer: payload.customer, total: payload.total })
        db.line_items.save({ order_id: order.id, sku: payload.sku, qty: payload.qty })

    queue.push "orders.processing", { order_id: order.id }
    reply 202, { order_id: order.id }

on queue "orders.processing" take message
    saved = doc.fulfillments.save(message)
```

Transform and filter in a single pass with `map` and `keep`:

```ruby
route POST "/scores/labeled" take payload
    labeled = payload.scores >> map s
        keep "high"   if s > 100
        keep "medium" if s > 50
        keep "low"    if s > 0

    reply 200, { labeled: labeled }
```

Branch with `match`, which returns a value:

```ruby
route GET "/status/:code"
    label = match params.code
        200 -> "OK"
        404 -> "Not Found"
        500 -> "Internal Server Error"
        fallback -> "Unknown"

    reply 200, { code: params.code, label: label }
```

Fan out calls to several services in parallel with broadcast (`*>>`):

```ruby
route GET "/users/:id/dashboard"
    sections = params.id *>>
        -> http_client.get("https://users.internal/#{params.id}").body
        -> http_client.get("https://orders.internal/#{params.id}").body
        -> http_client.get("https://catalog.internal/#{params.id}/favorites").body

    reply 200, {
        profile: sections[0],
        orders: sections[1],
        favorites: sections[2]
    }
```

Declare an auth provider once, then require it on a route and authorize with `allow`:

```ruby
auth api_key console_key {
    header: "x-api-key"
    secret_hash: env.MARRETA_CONSOLE_KEY_HASH
    principal: "console"
}

route GET "/accounts/:id"
    require auth console_key
    allow auth.user.id == "console"

    account = doc.accounts.find(params.id)
    require account else fail 404, "account not found"

    reply 200, account
```

You can declare more than one provider and pick the right one per route. A `jwt`
provider validates tokens from an external identity provider against its JWKS,
so you can authorize on the roles those tokens carry:

```ruby
auth jwt identity {
    issuer: "https://login.your-idp.com/tenant/v2.0"
    audience: "api://accounts"
    jwks_url: "https://login.your-idp.com/tenant/discovery/keys"
    algorithm: "RS256"
}

route GET "/reports/usage"
    require auth identity
    allow "reports.read" in auth.user.roles

    reply 200, { generated_by: auth.user.id }
```

Test endpoints against mocked infrastructure, with no separate test harness:

```ruby
scenario "returns an account by id"
    given doc.accounts.find("acc_1") returns { _id: "acc_1", owner: "Ana", balance: 500 }

    when GET "/accounts/acc_1"

    then status 200
    then response is { body: { owner: "Ana", balance: 500 } }
```

## Local Services

Some APIs need local infrastructure during development. The `--with` flag tells
`marreta init` which providers to prepare. It generates `marreta.env` and a
`docker-compose.yml`, not business code:

```bash
marreta init orders-api --with db,cache,queue
cd orders-api
docker compose up -d        # requires Docker & Docker Compose
marreta serve
```

If you'd rather not use Docker, point the variables in `marreta.env` at services
you already run locally, in the cloud, or in your company infrastructure.

Infrastructure is exposed through language namespaces, and the provider-specific
client code is hidden behind runtime configuration.

| Concern | Surface | Current provider |
| --- | --- | --- |
| Relational database | `db.*` | PostgreSQL |
| Document database | `doc.*` | MongoDB |
| Cache | `cache.*` | Redis |
| Queues & topics | `queue.push` / `topic.publish`, `on queue` / `on topic` | RabbitMQ |

The goal isn't to make provider details vanish. Connection settings, credentials,
and operational behavior still matter. It's to keep application code focused on
API behavior while Marreta Lang handles the common integration layer.

## Built-In API Concepts

| Concern | Marreta Lang concept |
| --- | --- |
| HTTP routes | `route GET "/path"` |
| Request validation | `take payload as Schema` |
| Responses | `reply` and `fail` |
| Reusable logic | `task` |
| Relational / document data | `db.*`, `doc.*` |
| Cache | `cache.*` |
| Queues & topics | `queue.push` / `topic.publish`, `on queue` / `on topic` |
| Authentication & authorization | `require auth`, `allow` |
| Tests | scenario tests via `marreta test` |
| API docs | generated OpenAPI spec |
| Project checks | `marreta doctor` |
| Formatting & linting | `marreta fmt`, `marreta lint` |

## Meet Martim

Marreta Lang's mascot is **Martim**, an energetic, anthropomorphic sledgehammer
who breaks through boilerplate with zero ceremony.

Legend has it there was a time when every API was born buried under layers,
scaffolding, and configuration files, and no one could see what it actually did
anymore. Then came Martim, a sledgehammer of steel, soul, and no patience for
ceremony. One swing was enough for the excess to collapse, leaving only the
essentials standing: take the request, apply the rule, return a clear answer.
Everywhere he passed, the code went back to telling its own story, and that is
how he became the spirit of Marreta Lang.

And do not be fooled by that happy little grin. Beneath it lies pure steel, and
wherever Martim passes he leaves more than clean code: he leaves quality,
efficiency, and performance. He may not be the hero your project asked for,
but he is the one it needed.

<div align="center">
  <img src="docs/assets/brand/images/mascot_success.png"   width="200" alt="Martim">
  <img src="docs/assets/brand/images/mascot_debugger.png"  width="200" alt="Martim">
  <img src="docs/assets/brand/images/mascot_ninja.png"     width="200" alt="Martim">
  <img src="docs/assets/brand/images/mascot_wizard.png"    width="200" alt="Martim">
</div>

<p align="center"><em>See the rest of Martim's poses in <a href="docs/assets/brand/images">docs/assets/brand/images</a>.</em></p>

## Focused by Design

Marreta Lang isn't meant to replace every backend stack or become a
general-purpose language. Its purpose is narrower: make the common case of
building REST APIs simpler, cleaner, and efficient enough for real services.

If a service needs a highly customized architecture, an unusual runtime model, or
low-level control, another stack may fit better. If it's mainly about exposing
REST endpoints, validating contracts, applying business rules, and integrating
with common infrastructure, that's the space Marreta Lang is built for.

## Project Status

Marreta Lang is **pre-1.0**. The language and runtime are evolving and the public
API surface is not frozen yet. The versioning policy keeps that manageable: a
project declares the minimum runtime it needs with `requires_marreta` in
`app.marreta`, enforced at load, and a breaking release bumps the version. The
version number is a reliable signal of what runs where, and a runtime too old for a
project is refused at load instead of failing in surprising ways. Use it today if
you're exploring a focused DSL for REST APIs and are comfortable with an
early-stage language.

## Documentation

The language documentation lives at [marreta.dev/docs](https://marreta.dev/docs),
with the project site at [marreta.dev](https://marreta.dev). That guide is the
primary reference for using the language. This repository is the reference for
contributors and for reading the source.

For idiomatic style, naming, and project layout, see the
[conventions reference](docs/guide/reference/conventions.md) in the documentation guide.

## Editor Support

A VS Code extension provides syntax highlighting, completion, hover docs,
diagnostics, symbols, and formatting. Install it from the
[VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=MarretaTeam.marretalang)
or [Open VSX](https://open-vsx.org/extension/MarretaTeam/marretalang) (for Cursor,
VSCodium, and Windsurf) by searching **MarretaLang** in the Extensions view, or
install the VSIX attached to each [release](https://github.com/tm-dev-lab/marreta-lang/releases)
through the command palette (**Extensions: Install from VSIX**). The extension is a
thin client over the `marreta` CLI, so install the binary first. Full steps are in
[Install the editor extension](docs/guide/how-to/install-the-editor-extension.md).

## Contributing

Marreta Lang is early-stage, so contributions should favor small, reviewable
changes with clear tests. Use the standard fork-and-PR flow.

**Prerequisites:** Rust 1.85 or newer, Docker and Docker Compose (only for
containerized example validation), and Node.js (only for editor tooling).

**Repository layout:**

| Path | Purpose |
| --- | --- |
| `src/` | Runtime, CLI, parser, interpreter, server, providers, tooling commands |
| `tests/` | Rust integration tests and fixtures |
| `e2e/` | In-memory feature suite exercised over localhost (see `e2e/README.md`) |
| `docs/examples/` | Example projects and functional validation suites |
| `docs/benchmarks/`, `docs/performance/` | Performance harnesses and historical measurements |
| `docs/editors/` | The published VS Code extension (a thin client over the CLI) |
| `docs/spec/`, `docs/assets/` | Language specs and brand assets |
| `.github/workflows/` | Manual build, release, extension release, e2e, and smoke workflows |

**Before opening a pull request:**

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

If your change affects runtime behavior for generated projects, functional
examples, migrations, or provider integration, also run the relevant example
suites before requesting review.

Guidelines:

- Keep changes scoped to one problem.
- Add or update tests for behavior changes.
- **Do not weaken tests to make a change pass.**
- Avoid broad refactors unless the refactor is the point.
- Prefer explicit design notes for language semantics and public CLI behavior.

## License

MIT. See [LICENSE](LICENSE).
