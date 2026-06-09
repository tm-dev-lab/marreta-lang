---
title: "Conventions"
category: reference
slug: "reference/conventions"
summary: "The house style for idiomatic Marreta: indentation, naming, schemas, routes, tasks, guards, responses, auth, comments, and file structure."
---

# Conventions

This is the house style for writing idiomatic Marreta. Following it keeps routes readable
and makes `marreta fmt` and `marreta lint` predictable across a project.

## Indentation

Indentation is significant. It defines blocks: route and task bodies, `match` arms,
`transaction` blocks, and any nested structure. The structure of the code comes from how it
is indented, so consistency is not optional.

```ruby
route POST "/orders" take payload as NewOrder
    require payload.total else fail 400, "total is required"

    transaction
        order = db.orders.save({ total: payload.total })
        db.line_items.save({ order_id: order.id })

    reply 201, { id: order.id }
```

- Indent with spaces, 4 per level. The language only requires consistent widths, but every
  project uses 4, so do the same.
- Pressing Tab is fine when your editor inserts spaces, but a literal tab character is
  rejected. If you hit an indentation error, set your editor to insert spaces.
- A dedent must return to a level you opened before, otherwise it is an error.
- Blank lines and comment-only lines do not affect indentation.

## Naming

| Construct | Convention | Example |
|---|---|---|
| Variables | `snake_case` | `user_id`, `order_total` |
| Tasks | `snake_case` | `task calculate_tax(item)` |
| Task parameters | `snake_case` | `task apply_discount(base_price)` |
| Schema names | `PascalCase` | `UserPayload`, `NewOrder` |
| Schema fields | `snake_case` | `first_name`, `is_active` |
| Optional schema fields | `snake_case?` | `email?`, `phone_number?` |
| Auth providers | `snake_case` | `internal_auth`, `entra_id` |
| Globals (`app.marreta`) | `snake_case` | `project_name = "payments-api"` |

It is `snake_case` everywhere with one exception. Variables, tasks, parameters, and fields
are `snake_case`. Schema names are the exception: they name a type, so they use `PascalCase`.

## Schemas

```ruby
schema UserPayload
    name: string
    age: integer
    email?: string
    is_active: boolean
```

- Schema names use `PascalCase`, the one place that is not `snake_case`.
- Fields are `snake_case`, one per line, indented 4 spaces.
- Optional fields use `?` on the field name, not the type.

## Routes

```ruby
route POST "/users" take payload as UserPayload
    require payload.name else fail 400, "name is required"
    reply 201, { id: 1 }
```

- Route paths use lowercase and hyphens: `"/user-profiles"`, `"/order-items/:id"`.
- Bind schemas with `as SchemaName` immediately after the take binding.

## Tasks

```ruby
task calculate_discount(price, category)
    rate = match category
        "vip"     -> 0.15
        "premium" -> 0.10
        fallback  -> 0.0
    price * rate
```

- Name a task as a verb in its base form, since a task does something:
  `calculate_tax`, `load_profile`, not `tax`, `calculated_tax`, or `loading_profile`.
- Prefer inline tasks (`=>`) for a single expression, and block tasks for multi-step logic.
- There is no explicit `return`. The last expression is the implicit return.

## Guards

```ruby
require payload.user_id else fail 400, "user_id is required"
reject client.blocked else fail 403, "account blocked"
```

- Use `require` for "must be truthy" checks and `reject` for "must be falsy" checks.
- Place all guards at the top of the route body, before business logic.

## Responses

```ruby
reply 200, { users: users, total: total }
reply 201, { id: new_user.id }
reply html 200, "<h1>Welcome</h1>"
reply text 200, "pong"
fail 404, "user not found"
```

- Prefer `fail` for early error exits. `reply` can intentionally model any HTTP status,
  including 4xx and 5xx, for example when mirroring an upstream response.
- `reply` and `fail` terminate execution immediately, so no code after them runs.

## Auth

```ruby
auth api_key internal_auth {
    header: "x-api-key"
    secret_hash: env.INTERNAL_KEY_HASH
    principal: "service"
}

route GET "/reports"
    require auth internal_auth
    allow auth.user.id == "service"
    reply 200, { ok: true }
```

- Provider names are `snake_case`, like variables. The provider type (`api_key`, `jwt`)
  comes first, then the name.
- Keep secrets out of the source, reading them from `env` in `marreta.env`.
- Place `require auth` and `allow` at the top of the route, with the other guards.
- Role checks like `allow "reports.read" in auth.user.roles` need a provider whose tokens
  carry roles, like `jwt`. An `api_key` principal carries no roles, so authorize it on
  `auth.user.id`.

## Comments

```ruby
# Route: list all active users
route GET "/users"
    limit = query.limit or 10  # default page size
    reply 200, users
```

- Use `#` for all comments.
- Prefer comments that explain why, not what.

## File structure

```text
project/
├── marreta.env          # infrastructure config (never commit secrets)
├── app.marreta          # entry point, global metadata only
├── routes/
│   ├── users.marreta
│   └── orders.marreta
├── schemas/
│   └── payloads.marreta
├── tasks/
│   └── calculations.marreta
└── tests/
    └── users_test.marreta
```

- One concern per file. Route files in `routes/`, shared tasks in `tasks/`, shared schemas
  in `schemas/`.
- Scenario tests live in `tests/` with the `_test.marreta` suffix, which is how
  `marreta test` and `marreta doctor` discover them. A test outside `tests/` or without the
  suffix is not picked up.
- `marreta.env` is for config only, no logic. Keep `app.marreta` metadata-only: the language
  allows routes and tasks there, but prefer them in their own directories.

## Multi-file and scoping

All symbols are file-private by default. Use `export` to share a task or schema across files,
reaching it through its [file namespace](../concepts/namespaces.md) as `file.task`:

```ruby
# tasks/calculations.marreta
export task calculate_discount(price) => price * 0.9
internal_rate = 0.05   # private, not visible outside this file
```

- Never use `export` in route files, since routes are never shared.
- `export` a task or schema only when it is used in more than one file.
- Name conflicts on `export` are a load error, so use distinct names.
- In `app.marreta` everything is implicitly global, so no `export` is needed.
