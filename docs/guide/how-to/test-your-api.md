---
title: "Test your API"
category: how-to
slug: "how-to/test-your-api"
summary: "Write scenario tests that run a route and assert its HTTP outcome, stubbing external calls so the tests stay self-contained."
---

# Test your API

Scenario tests check your API's behavior by running a route and asserting the HTTP
result. They run your real route logic (validation, control flow, shaping) in
memory, without starting a server or any provider, so they are fast and
deterministic. They live under `tests/` and run with `marreta test`.

`marreta test` discovers scenario files by a name convention: a file must be under
`tests/` and end in `_test.marreta` (for example `notes_test.marreta`). A file in
`tests/` without that suffix is ignored and its scenarios never run, so name the file
accordingly when you create one.

The examples below use the `/greetings` route from `marreta init` and a small
`/notes` endpoint backed by the document store, which needs no migration.

## Prerequisites

- A scaffolded project with at least one route (`marreta init hello`).
- The [Quickstart](../tutorials/quickstart.md) finished, so `marreta test` is
  familiar.

## Write a scenario

A scenario issues a request with `when` and asserts the outcome with `then`. The
scaffold ships one:

```ruby
scenario "reads a greeting"
    when GET "/greetings"

    then response is {
        status: 200,
        body: {
            message: "Hello, Marreta!"
        }
    }
```

`when` names the verb and path (add `with { ... }` to send a JSON body). `then
response is` matches the status and body. Run the file:

```bash
marreta test
```

## Given, when, then

A scenario reads like a behavior, in the Given-When-Then style popularized by BDD
(Behavior-Driven Development) tools such as Cucumber and its Gherkin language:

- **`scenario`** names the behavior under test, such as "creates a note".
- **`given`** sets up the preconditions, such as stubbing an external call (shown
  below).
- **`when`** performs the action, an HTTP request to one of your routes.
- **`then`** asserts the outcome, the response status and body.

This is the same Arrange-Act-Assert structure that unit and integration tests use,
written as readable steps: `given` is Arrange, `when` is Act, `then` is Assert.

Marreta scenarios are deliberately API-focused. The action is always an HTTP request
to a route, and the assertions are about the HTTP response, so you test your API the
way a client sees it. The syntax is Marreta's own, not a port of another framework.

## A route to test

The remaining examples test a small notes endpoint, backed by the document store so
there is no migration to run. Add a schema and two routes:

```ruby
export schema NewNote
    title: string
    body: string

route POST "/notes" take payload as NewNote
    note = doc.notes.save({
        title: payload.title,
        body: payload.body
    })
    reply 201, { id: note._id, title: note.title }

route GET "/notes/:id"
    note = doc.notes.find(params.id)
    require note else fail 404, "note not found"
    reply 200, note
```

## Assert the failure paths

Test the rejections as well as the happy path. A schema validation failure happens
before your route body runs, so it needs nothing else:

```ruby
scenario "rejects a malformed note"
    when POST "/notes" with {
        title: "no body"
    }
    then status 422
```

`body` is missing, so the request never reaches your logic and the scenario sees a
422.

## Stub external calls with `given`

A scenario does not connect to a document store, database, cache, queue, or HTTP
service. Instead, you declare what each external call returns with `given`, which
keeps the test self-contained. A route that saves and returns a note is tested like
this:

```ruby
scenario "creates a note"
    given doc.notes.save(anything) returns {
        _id: "note-1",
        title: "First note",
        body: "hello"
    }

    when POST "/notes" with {
        title: "First note",
        body: "hello"
    }
    then status 201
```

The route's validation, save, and shaping all run for real. Only the document store
answer comes from your `given`. `anything` matches any argument, so you do not have
to restate the exact map you saved. A read stubs the same way:

```ruby
scenario "reads a note back"
    given doc.notes.find("note-1") returns {
        _id: "note-1",
        title: "First note",
        body: "hello"
    }

    when GET "/notes/note-1"
    then status 200
```

The `db` namespace stubs identically, with `given db.<table>.<method>(...)`.

The mock is strict in both directions, which keeps your tests honest:

- An external call the route makes with **no matching `given`** fails the scenario
  as an unconfigured call. You cannot accidentally hit a real provider.
- A `given` the route **never calls** fails too, as an unused given. A stub that
  does not match reality is a bug in the test.

## What scenario tests do not do

Scenario tests verify route logic and contracts, not the store itself. They do not
check that a real query returns what you expect or that a migration applied. For
that end-to-end confidence, run the server against the local services and send real
requests, as in
[Build a relational API with migrations](../tutorials/relational-api-with-migrations.md)
and [Persist data with local services](use-local-services.md). The two layers are
complementary: scenarios for fast logic and contract checks, live requests for real
integration.

## Try it

```bash
marreta test
```

Every scenario in a `*_test.marreta` file under `tests/` runs, and the command exits
non-zero if any fails.

## Result checkpoint

You should now be able to write a scenario that asserts a route's status and body,
test a validation failure with no setup, and test a route that persists data by
stubbing the call with `given`.

## Common pitfalls

- **`unconfigured call: doc.<collection>.<method>(...)`.** The route made an
  external call you did not stub. Add a matching `given`, using `anything` for
  arguments you do not need to pin down.
- **`unused given`.** You declared a `given` the route never reached. Remove it, or
  fix the scenario so the route actually makes that call.

## Next steps

- [Validate a request payload](validate-a-payload.md): the validation that produces
  those 422s.
- [Persist data with local services](use-local-services.md): persisting for real,
  beyond the stubs.
