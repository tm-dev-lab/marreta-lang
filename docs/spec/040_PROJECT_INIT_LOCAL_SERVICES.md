# 040 — Project Init Local Services

> Status: Approved
> Type: CLI / project bootstrap
> Scope: Optional local service scaffolding for `marreta init`

---

## 1. Purpose

Spec 038 introduced:

```bash
marreta init <project-path>
```

This spec keeps `init` focused on one job: create a project that can be started
quickly with `marreta serve`.

It adds an optional service selection flag:

```bash
marreta init <project-path> --with db,cache,doc,queue
```

The selected services prepare local development infrastructure only. They do
not generate example business logic for DB, document store, cache, or queue.

The first success path should stay small:

```bash
marreta init hello-api --with db,cache
cd hello-api
docker compose up -d --wait
marreta serve
open http://localhost:8080/greetings
```

The generated application still responds even before the user writes code that
uses the selected services.

---

## 2. Design Principles

1. **Initializer, not tutorial generator.**
   `init` should create a ready project, not demonstrate every language
   feature.

2. **The app runs with `marreta serve`.**
   Local development starts the Marreta runtime directly on the host.

3. **Docker Compose is for dependencies.**
   When services are selected, Compose starts local backing services only:
   Postgres, Redis, MongoDB, RabbitMQ.

4. **Generated code stays minimal.**
   The base route remains HTTP-only. `--with db` does not generate CRUD,
   migrations, persisted schemas, or DB-backed routes.

5. **Service guidance lives in README.**
   The code should not be filled with instructional comments. The README lists
   selected services and the Marreta namespaces they unlock.

6. **No hidden provisioning.**
   `marreta serve` does not start Docker, run migrations, create queues, or
   provision infrastructure.

7. **No deploy story in this spec.**
   Kubernetes, Helm, app container orchestration, and public runtime image
   distribution belong to separate specs.

---

## 3. Command Shape

### 3.1 Default

```bash
marreta init hello-api
```

Generates the minimal HTTP project.

### 3.2 With Local Services

```bash
marreta init hello-api --with db,cache,doc,queue
```

`--with` accepts a comma-separated list. Whitespace around items is ignored:

```bash
marreta init hello-api --with db, cache
```

Supported first-cut service names:

```text
db
cache
doc
queue
```

Unknown services fail before writing files:

```text
unknown init service 'redis'. Supported: db, cache, doc, queue
```

Duplicate services are ignored:

```bash
marreta init app --with db,db,cache
```

is equivalent to:

```bash
marreta init app --with db,cache
```

### 3.3 Non-Interactive Only

This spec does not introduce:

```bash
marreta init --interactive
```

Reason: prompt UIs vary across terminals, CI, remote shells, IDE terminals, and
Docker sessions. `--with` is deterministic and scriptable.

---

## 4. Generated Application

### 4.1 Base Endpoint

The generated endpoint should be browser-friendly and require no request body,
query string, or headers:

```text
GET /greetings
```

Generated route:

```marreta
route GET "/greetings"
    message = build_greeting("Marreta")
    reply 200 as GreetingResponse, { message: message }
```

Generated response:

```json
{
  "message": "Hello, Marreta!"
}
```

### 4.2 Base Files

The base project should generate:

```text
app.marreta
routes/greetings.marreta
schemas/greetings.marreta
tasks/greetings.marreta
tests/greetings_test.marreta
marreta.env
marreta.env.example
.gitignore
README.md
```

`marreta.env` is ready to use with local development defaults and is ignored by
git. `marreta.env.example` is safe to commit, uses placeholder credentials for
secrets, and exists as a reference for new environments. The README should not
ask users to copy the example file as a required first step.

### 4.3 Schema

Since the first endpoint has no request payload, the base schema file should
only contain the response schema:

```marreta
export schema GreetingResponse
    message: string
```

The scaffold should not include `GreetingRequest` unless a generated route uses
it.

### 4.4 Scenario Test

The base scenario test should validate the browser-friendly route:

```marreta
scenario "reads greeting"
    when GET "/greetings"

    then response is {
        status: 200,
        body: {
            message: "Hello, Marreta!"
        }
    }
```

---

## 5. Selected Services

Selecting services changes configuration and local development infrastructure,
not generated application logic.

### 5.1 `db`

Adds local PostgreSQL configuration:

```env
MARRETA_DB_PROVIDER=postgres
MARRETA_DB_HOST=127.0.0.1
MARRETA_DB_PORT=5432
MARRETA_DB_NAME=marreta
MARRETA_DB_USER=marreta
MARRETA_DB_PASSWORD=marreta
```

In `marreta.env.example`, the password should be `change-me`.

Adds a PostgreSQL service to `docker-compose.yml`.

README guidance:

```text
db: PostgreSQL is available through the `db` namespace.
```

No DB schema, migration, or DB route is generated.

### 5.2 `cache`

Adds local Redis configuration:

```env
MARRETA_CACHE_PROVIDER=redis
MARRETA_CACHE_HOST=127.0.0.1
MARRETA_CACHE_PORT=6379
MARRETA_CACHE_PASSWORD=redis-secret
```

In `marreta.env.example`, the password should be `change-me`.

Adds a Redis service to `docker-compose.yml`.

README guidance:

```text
cache: Redis is available through `cache.get(...)`, `cache.set(...)`, and other
cache namespace functions.
```

No cache-backed route is generated.

### 5.3 `doc`

Adds local MongoDB configuration:

```env
MARRETA_DOC_PROVIDER=mongodb
MARRETA_DOC_HOST=127.0.0.1
MARRETA_DOC_PORT=27017
MARRETA_DOC_NAME=marreta
MARRETA_DOC_USER=marreta
MARRETA_DOC_PASSWORD=marreta-secret
MARRETA_DOC_AUTH_SOURCE=admin
```

In `marreta.env.example`, the password should be `change-me`.

Adds a MongoDB service to `docker-compose.yml`.

README guidance:

```text
doc: MongoDB is available through the `doc` namespace.
```

No document route is generated.

### 5.4 `queue`

Adds local RabbitMQ configuration:

```env
MARRETA_QUEUE_PROVIDER=rabbitmq
MARRETA_QUEUE_HOST=127.0.0.1
MARRETA_QUEUE_PORT=5672
MARRETA_QUEUE_USER=guest
MARRETA_QUEUE_PASSWORD=guest
```

In `marreta.env.example`, the password should be `change-me`.

Adds a RabbitMQ service to `docker-compose.yml`.

README guidance:

````markdown
- queue: RabbitMQ is available through Marreta queue producers, consumers, and topics.

  Point-to-point:

  ```marreta
  queue.push "greetings.created", { message: "Hello" }
  ```

  Topic:

  ```marreta
  topic.publish "greetings.created", { message: "Hello" }
  ```
````

No producer route or consumer is generated.

---

## 6. Docker Compose

When no services are selected, the scaffold should not generate
`docker-compose.yml`. There are no local dependencies to start, and documenting
Compose would imply a containerized app runtime that this spec deliberately
keeps out of scope.

When one or more services are selected, `docker-compose.yml` should be optimized
for local backing services:

```yaml
services:
  postgres:
    image: postgres:16-alpine
    environment:
      POSTGRES_DB: marreta
      POSTGRES_USER: marreta
      POSTGRES_PASSWORD: marreta
    ports:
      - "5432:5432"
```

The generated README should use:

```bash
docker compose up -d --wait
marreta serve
```

The Compose file should not be used to run the generated Marreta app in this
spec. Running the app as a container depends on runtime image distribution and
belongs to a separate spec.

---

## 7. README Shape

For a project without selected services:

````markdown
## Run

```bash
marreta serve
```

Open:

```text
http://localhost:8080/greetings
```

## Tests

```bash
marreta test
```
````

For a project with selected services:

````markdown
## Run

Start selected local services. This requires Docker and Docker Compose:

```bash
docker compose up -d --wait
```

Start the app:

```bash
marreta serve
```

Open:

```text
http://localhost:8080/greetings
```

## Selected Services

- db: PostgreSQL is available through the Marreta `db` namespace.

  Example:

  ```marreta
  item = db.items.find(1)
  ```
- cache: Redis is available through the Marreta `cache` namespace.

  Example:

  ```marreta
  cache.set("greeting", "Hello")
  ```
- queue: RabbitMQ is available through Marreta queue producers, consumers, and topics.

  Point-to-point example:

  ```marreta
  queue.push "greetings.created", { message: "Hello" }
  ```

  Topic example:

  ```marreta
  topic.publish "greetings.created", { message: "Hello" }
  ```

## Tests

Tests do not require Docker or selected services.

```bash
marreta test
```

## Stop Services

When finished, stop selected local services:

```bash
docker compose down
```
````

The Selected Services section should explain that services selected with
`--with` are local backing services. Users should start them before
`marreta serve` so the providers configured in `marreta.env` are reachable.
It should also state that users can skip Compose if equivalent services are
already running locally, in another Compose stack, or in the cloud; in that
case they should edit `marreta.env` with the correct hosts, ports, and
credentials. It should also show `docker compose down` as the cleanup command
for Compose-managed services.

The README should not include:

- Dockerfile instructions
- app container runtime instructions
- Docker-only app run instructions
- Kubernetes instructions
- migration commands unless generated code includes migrations
- long explanations of the generated greeting route

---

## 8. CLI Output

After creating a project without services:

```text
Created Marreta project: hello-api

Next steps:
  cd hello-api
  marreta serve

Open:
  http://localhost:8080/greetings
```

After creating a project with services:

```text
Created Marreta project: hello-api

Next steps:
  cd hello-api
  docker compose up -d --wait
  marreta serve

Open:
  http://localhost:8080/greetings
```

If services were selected, the output may also mention:

```text
Selected services: db, cache
```

---

## 9. Testing Strategy

### 9.1 Unit Tests

Test:

- project-name validation
- empty/non-empty target directory behavior
- service list parsing
- unknown service rejection
- duplicate service normalization
- generated file list for each service
- generated README content for no-service and service-selected projects

### 9.2 Generated Tests

For every generated project:

```bash
marreta test
```

should pass without Docker and without selected backing services running.

### 9.3 Functional Smoke

Add or update an explicit functional script that validates:

```bash
marreta init tmp-app --with db,cache,doc,queue
cd tmp-app
docker compose up -d --wait
marreta serve
curl http://localhost:8080/greetings
```

The smoke test should prove:

- selected services can start locally
- the generated app can start with `marreta serve`
- the base endpoint responds

The smoke test does not need to write to DB, cache, doc, or queue, because the
generated code intentionally does not use those services.

---

## 10. Non-Goals

This spec does not introduce:

- generated DB CRUD examples
- generated migrations
- generated cache routes
- generated document routes
- generated queue producers or consumers
- generated http-client examples
- generated feature-flag examples
- auth scaffold
- Kubernetes manifests
- Helm or Kustomize
- app container orchestration
- Docker-only app runtime instructions
- public runtime image distribution
- interactive terminal wizard
- provider selection beyond the current default providers

---

## 11. Open Questions

1. Should `http-client` and `feature-flag` ever be accepted by `--with`?

   Current recommendation: not in this spec. They are not local backing
   services, and adding them would re-open tutorial-generator scope.
