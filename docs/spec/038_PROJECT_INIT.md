# 038 — Project Init

> Status: Approved
> Type: CLI / project bootstrap
> Scope: Container-first scaffold for a minimal Marreta HTTP project

---

## 1. Purpose

This spec introduces:

```bash
marreta init <project-path>
```

The command creates a new Marreta project with the recommended file layout,
a minimal HTTP example, a scenario test, and container-first local runtime
support.

The purpose is not to teach every Marreta feature. The purpose is to give a
new user a working project that demonstrates the project structure Marreta
expects before v1:

- `app.marreta` owns project metadata
- `routes/` owns HTTP routes
- `schemas/` owns request/response contracts
- `tasks/` owns reusable application logic
- `tests/` owns scenario tests
- Docker files let the project run as a container using a local Marreta runtime
  image before public registry images exist

---

## 2. Why Init Matters

Marreta now has a strong project model:

- canonical `app.marreta`
- required `project_name` / `project_version`
- multi-file loading by convention
- `marreta doctor`
- `marreta test`
- container-oriented runtime logs and trace context

Without `marreta init`, the first public user must learn those conventions from
documentation before running anything.

`marreta init` should make the first project executable immediately:

```bash
marreta init hello-api
cd hello-api
cp marreta.env.example marreta.env
marreta test
docker compose up --build
```

This is a v1 readiness feature: it reduces onboarding friction without adding
language syntax.

---

## 3. Design Principles

1. The scaffold should teach project layout, not the full feature surface.
2. The default project should be HTTP-only.
3. The default project should not require Postgres, MongoDB, Redis, RabbitMQ,
   auth, migrations, or external service setup.
4. The default project should be container-first.
5. The generated container should use a local Marreta runtime image, not a
   copied binary or public registry image.
6. Generated files should be simple enough to read and modify by hand.
7. The command should fail rather than overwrite existing user files.
8. `init` should not overlap with `doctor`; it creates a project, it does not
   validate an existing project beyond safe filesystem checks.

---

## 4. Command Shape

Primary form:

```bash
marreta init <project-path>
```

Example:

```bash
marreta init hello-api
```

Generated project root:

```text
hello-api/
```

The initial command should not require flags.

`<project-path>` may be either a simple directory name or a path to the
directory to create. The project metadata name is derived from the final path
component.

Examples:

```bash
marreta init hello-api
marreta init /tmp/hello-api
```

Both generate:

```marreta
project_name = "hello-api"
```

## 4.1 Project Name Rules

The final path component should be a simple filesystem-safe project name:

- first character must be a letter or digit
- lowercase letters
- uppercase letters
- digits
- hyphen
- underscore

Invalid examples:

- empty name
- `.` or `..`
- `-hello`
- `_hello`
- names containing control characters

If invalid, the command fails with a clear CLI error.

## 4.2 Existing Path Rules

If `<project-path>` does not exist, `init` creates it.

If `<project-path>` exists and is empty, `init` may use it.

If `<project-path>` exists and is not empty, `init` must fail. The first cut
does not include `--force`.

Rationale: project scaffolding should never risk overwriting user work.

---

## 5. Generated Tree

`marreta init hello-api` should generate:

```text
hello-api/
  app.marreta
  routes/
    greetings.marreta
  schemas/
    greetings.marreta
  tasks/
    greetings.marreta
  tests/
    greetings_test.marreta
  marreta.env.example
  .gitignore
  Dockerfile
  docker-compose.yml
  README.md
```

This is the default layout recommendation for public v1 projects.

The scaffold intentionally separates responsibilities even though the example
is small. The point is to make the convention visible from the first project.

---

## 6. Generated Marreta Files

## 6.1 `app.marreta`

```marreta
project_name = "hello-api"
project_version = "0.1.0"
```

`app.marreta` should stay minimal in the scaffold. It establishes project
identity and lets route/schema/task files live in their recommended folders.

## 6.2 `schemas/greetings.marreta`

```marreta
export schema GreetingRequest
    name: string

export schema GreetingResponse
    message: string
```

The schema file demonstrates:

- exported schemas
- request contract
- response contract

It should not include optional fields, nested schemas, persistence, or advanced
schema features in the first scaffold.

## 6.3 `tasks/greetings.marreta`

```marreta
export task build_greeting(name)
    "Hello, #{name}!"
```

The task file demonstrates reusable logic outside the route file.

It intentionally avoids branching, infrastructure, and side effects.

## 6.4 `routes/greetings.marreta`

```marreta
route POST "/greetings" take payload as GreetingRequest
    message = build_greeting(payload.name)
    reply 200 as GreetingResponse, { message: message }
```

The route demonstrates:

- HTTP route declaration
- payload binding with schema validation
- task call
- response schema serialization

## 6.5 `tests/greetings_test.marreta`

```marreta
scenario "creates greeting"
    when POST "/greetings" with {
        name: "Marreta"
    }

    then response is {
        status: 200,
        body: {
            message: "Hello, Marreta!"
        }
    }
```

The test demonstrates the scenario convention without mocks or external
infrastructure.

---

## 7. Environment File

The scaffold should generate `marreta.env.example`:

```env
MARRETA_HOST=0.0.0.0
MARRETA_PORT=8080
MARRETA_REQUEST_LOG=true
MARRETA_TRACE_CONTEXT=true
```

The generated project should not include `marreta.env` directly.

Rationale:

- `marreta.env.example` is safe to commit
- the user controls whether to create `marreta.env`
- the README and CLI output explain the copy step

No DB, doc, cache, queue, or auth variables should be generated in the default
scaffold.

---

## 8. Local Runtime Image

The default scaffold is container-first, but the project must not depend on a
public Marreta Docker image in the first cut.

The first cut should assume a local Docker image already exists:

```text
marreta:local
```

The generated project should not contain a copied Marreta binary.

Rationale:

- avoids committing runtime binaries into user projects
- avoids cross-platform binary mismatches between host and Linux containers
- keeps the generated project focused on application source
- makes future migration to a public runtime image a one-line image reference
  change

The first-cut container workflow is intentionally local-registry-first:

```bash
docker image ls marreta:local
docker compose up --build
```

If `marreta:local` does not exist, Docker build fails before the app image is
created. The README and CLI output must tell users to ensure that image exists.

`marreta init` itself should remain filesystem-only. It should not call Docker,
inspect Docker images, or require the Docker daemon during project generation.

This local-image approach is a pre-public bridge. It is not part of the public
v1 contract. Before public v1 ships, a runtime distribution spec must replace
or supplement this section by defining the official Marreta image name and
versioning policy. Until that spec exists, scaffolds generated by `init` are
tied to the local development workflow.

## 8.1 Future Public Image Migration

The generated Dockerfile should use an image build argument so the local image
can be replaced by a public image later without rewriting the scaffold shape.

Pre-public default:

```text
MARRETA_RUNTIME_IMAGE=marreta:local
```

Future public override:

```text
MARRETA_RUNTIME_IMAGE=ghcr.io/tm-dev-lab/marreta:1
```

The future registry name is illustrative, not committed by this spec.

---

## 9. Dockerfile

The generated `Dockerfile` should be:

```dockerfile
ARG MARRETA_RUNTIME_IMAGE=marreta:local
FROM ${MARRETA_RUNTIME_IMAGE}

WORKDIR /app

COPY . .

EXPOSE 8080

CMD ["/app/app.marreta"]
```

The Dockerfile should not install Rust, run Cargo, copy a local Marreta binary,
download Marreta, or use a public Marreta base image in the first cut.

It should build a runnable application image from the generated project files
and the local Marreta runtime image.

The runtime image is expected to own the `marreta serve` entrypoint. The
generated application Dockerfile should only provide the project entrypoint path
as `CMD`, so it does not duplicate the runtime command.

---

## 10. Docker Compose

The generated `docker-compose.yml` should be:

```yaml
services:
  app:
    build:
      context: .
      args:
        MARRETA_RUNTIME_IMAGE: marreta:local
    ports:
      - "8080:8080"
    env_file:
      - marreta.env
```

The default compose file should start exactly one service: the Marreta
application.

It should not include:

- Postgres
- MongoDB
- Redis
- RabbitMQ
- Jaeger
- OpenTelemetry collectors
- reverse proxies

Those belong in future templates or user-authored project evolution, not in the
minimal default scaffold.

---

## 11. Gitignore

The scaffold should generate a minimal `.gitignore`:

```gitignore
marreta.env
target/
```

The default scaffold does not contain runtime binaries, so there is no
`bin/marreta` or `.marreta/` rule.

Rationale:

- `marreta.env` may contain local secrets and should not be committed
- `target/` is a common local build artifact
- generated source files, Docker files, and `marreta.env.example` are intended
  to be committed

---

## 12. README

The generated `README.md` should include:

````markdown
# hello-api

A MarretaLang project generated by `marreta init`.

## Local Runtime

```bash
cp marreta.env.example marreta.env
marreta doctor
marreta test
marreta serve
```

## Container Runtime

Before running containers, make sure the local Marreta runtime image exists:

```bash
docker image ls marreta:local
```

```bash
cp marreta.env.example marreta.env
docker compose up --build
```

## Docker Runtime Without Compose

```bash
cp marreta.env.example marreta.env
docker build --build-arg MARRETA_RUNTIME_IMAGE=marreta:local -t hello-api .
docker run --rm --env-file marreta.env -p 8080:8080 hello-api
```

## Try The API

After starting the app locally, with Docker Compose, or with Docker directly,
call the generated endpoint:

```bash
curl -X POST http://localhost:8080/greetings \
  -H "content-type: application/json" \
  -d '{"name":"Marreta"}'
```

Expected response:

```json
{
  "message": "Hello, Marreta!"
}
```

## Project Layout

- `app.marreta` — project metadata and entrypoint
- `routes/` — HTTP routes
- `schemas/` — request and response contracts
- `tasks/` — reusable application logic
- `tests/` — scenario tests run by `marreta test`
- `Dockerfile` — container build using the local `marreta:local` runtime image
- `docker-compose.yml` — app-only container runtime
````

The README should not mention DB, queue, cache, auth, migrations, or deployment
targets in the first cut.

---

## 13. CLI Output

After successful generation, `marreta init hello-api` should print:

```text
Created Marreta project: hello-api

Next steps:
  cd hello-api
  cp marreta.env.example marreta.env
  marreta doctor
  marreta test
  marreta serve

Container-first:
  docker image ls marreta:local
  docker compose up --build

Docker without Compose:
  docker build --build-arg MARRETA_RUNTIME_IMAGE=marreta:local -t hello-api .
  docker run --rm --env-file marreta.env -p 8080:8080 hello-api

Try it:
  curl -X POST http://localhost:8080/greetings \
    -H "content-type: application/json" \
    -d '{"name":"Marreta"}'
```

The output should be human-readable and not JSON.

---

## 14. Relationship To Existing Specs

This spec builds on:

- `019_PROJECT_ENTRYPOINT.md` — `app.marreta` as project entrypoint
- `019b_PROJECT_METADATA_UNIFICATION.md` — `project_name` and
  `project_version`
- `020_SECRET_AWARE_CONFIG.md` — `marreta.env` / process env conventions
- `021_DOCTOR_COMMAND.md` — generated README points users to `marreta doctor`
- `023_TESTING_DSL.md` — generated scenario test
- `035_W3C_TRACE_CONTEXT.md` — default env enables trace context
- `037_RUNTIME_EVENT_LOG_CONTRACT.md` — default env enables request/consumer
  runtime events

This spec does not change any language syntax.

---

## 15. Non-Goals

The first cut does not include:

- `marreta check`
- formatter
- linter
- LSP
- project templates
- `--force`
- `--minimal`
- `--with-db`
- `--with-queue`
- Docker Compose services besides the application
- official public Docker images
- downloading Marreta from a registry
- copying Marreta binaries into generated projects
- compiling Marreta inside the generated Dockerfile
- generating production deployment manifests
- generating `marreta.env` directly
- initializing Git
- package manager integration

`marreta check` is intentionally out of scope because its boundary with
`marreta doctor` needs a separate design discussion.

---

## 16. Implementation Notes

Implementation should be small and explicit:

1. Add `init` to CLI command dispatch.
2. Validate `<project-path>` and derive the project name from its final path
   component.
3. Create the project directory if missing.
4. Reject non-empty existing directories.
5. Create subdirectories: `routes`, `schemas`, `tasks`, `tests`.
6. Write scaffold files with deterministic contents.
7. Print next steps.

No parser/runtime changes should be needed.

Tests for `marreta init` itself should remain filesystem-level tests. They
should assert generated paths and file contents, but they should not require a
Docker daemon or the `marreta:local` image. End-to-end Docker validation belongs
in a separate functional test path that can be gated by the test environment.

---

## 17. Test Plan

Unit tests should cover:

- valid project name accepted
- valid project path accepted
- empty project name rejected
- existing non-empty directory rejected
- generated file paths match the expected tree
- generated Marreta files contain the expected project name
- generated `.gitignore` ignores `marreta.env`
- generated Dockerfile uses `MARRETA_RUNTIME_IMAGE`
- generated compose file exposes `8080:8080`
- generated test file matches the `tests/**/*_test.marreta` convention from
  `023_TESTING_DSL.md`

Functional validation should cover:

1. Ensure a local runtime image exists:

```bash
docker image ls marreta:local
```

2. Generate a project:

```bash
marreta init /tmp/hello-api
```

3. Run scenario tests:

```bash
cd /tmp/hello-api
cp marreta.env.example marreta.env
marreta test
```

4. Run the container with Docker Compose:

```bash
docker compose up --build
```

5. Or run the container with Docker directly:

```bash
docker build --build-arg MARRETA_RUNTIME_IMAGE=marreta:local -t hello-api .
docker run --rm --env-file marreta.env -p 8080:8080 hello-api
```

6. Verify the endpoint:

```bash
curl -X POST http://localhost:8080/greetings \
  -H "content-type: application/json" \
  -d '{"name":"Marreta"}'
```

Expected response:

```json
{"message":"Hello, Marreta!"}
```

The implementation should also validate that request logs are emitted when the
container runs with `MARRETA_REQUEST_LOG=true`.

---

## 18. Watch Points

Known points to revisit after first implementation:

- Whether `marreta:local` remains the right pre-public local image tag.
- Whether the runtime distribution spec is a hard prerequisite for public v1
  release of `marreta init`.
- Whether `MARRETA_RUNTIME_IMAGE` should default to a public image once
  published, or remain a required override for container builds.
- Whether a future official Marreta Docker image should replace the local image
  default in generated Dockerfiles.
- Whether future templates should add DB/queue/cache variants.
- Whether `marreta check` should exist separately from `marreta doctor`.

---

## 19. Recommendation

Ship `marreta init` before public v1.

It is the smallest feature that turns Marreta from "a capable runtime" into
"a language a new user can start correctly in one command."

The default scaffold should remain intentionally small:

- one POST route
- one request schema
- one response schema
- one task
- one scenario test
- one app container

That teaches the project convention without overwhelming the first run.
