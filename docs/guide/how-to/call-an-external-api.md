---
title: "Call an external API"
category: how-to
slug: "how-to/call-an-external-api"
summary: "Make HTTP requests to other services with http_client, guard on the response status, and test without a live upstream."
---

# Call an external API

The `http_client` namespace makes HTTP requests to other services. A response is an
envelope with `.status`, `.body`, and `.headers`. A 4xx or 5xx is **not** an error
in Marreta, so you decide how to handle it with `require`.

## Prerequisites

- A scaffolded project (`marreta init hello`).
- The [Quickstart](../tutorials/quickstart.md) finished, and familiarity with
  [Handle errors](handle-errors.md).

## Make a GET request

Call `http_client.get(url)` and read the envelope. Guard the status before using
the body, so an upstream failure becomes a clear response of your choosing:

```ruby
route GET "/users/:id"
    response = http_client.get("https://api.example.com/users/#{params.id}")
    require response.status == 200 else fail 502, "user service failed"
    reply 200, response.body
```

String interpolation (`#{params.id}`) builds the URL from request data.

## Send a body

For POST, PUT, and PATCH, pipe the body into the call with `>>`. It reads as "take
this payload and post it":

```ruby
route POST "/orders" take payload
    response = payload >> http_client.post("https://api.example.com/orders")
    require response.status == 201 else fail 502, "order service failed"
    reply 201, response.body
```

The same call without the pipeline passes the body as a second argument. The two
forms are equivalent, so pick whichever reads better:

```ruby
route POST "/orders" take payload
    response = http_client.post("https://api.example.com/orders", payload)
    require response.status == 201 else fail 502, "order service failed"
    reply 201, response.body
```

`put` and `patch` work the same way. `delete` takes no body, like `get`.

## Pass query parameters

There are two ways to add a query string, and which one to use depends on the verb.

For a GET (or DELETE), pipe a map of parameters into the call. The map becomes the
query string, so this request hits
`https://api.example.com/search?q=marreta&limit=5`:

```ruby
route GET "/search"
    response = { q: "marreta", limit: 5 } >> http_client.get("https://api.example.com/search")
    reply 200, response.body
```

The `query:` named argument also adds a query string, works on any verb, and is the
way to add one to a request that already has a body. On a POST the piped value is the
body, so the query goes in `query:`:

```ruby
route POST "/orders" take payload
    response = payload >> http_client.post("https://api.example.com/orders",
        query: { trace: "abc" })
    reply 201, response.body
```

In short: for GET and DELETE, the piped map is the query. For POST, PUT, and PATCH,
the piped value is the body, so reach for `query:` to add query parameters. `query:`
works on every verb.

## Headers and timeout

Pass `headers:` and `timeout:` (milliseconds) as named arguments:

```ruby
route GET "/me"
    response = http_client.get("https://api.example.com/me",
        headers: { authorization: "Bearer #{env.API_TOKEN}" },
        timeout: 5000)
    require response.status == 200 else fail 502, "profile service failed"
    reply 200, response.body
```

## Handle upstream failures

Because a 4xx or 5xx is a normal response, not a thrown error, you choose what each
status means for your API. Guard with `require`, and map the upstream result to your
own status:

```ruby
route GET "/users/:id"
    response = http_client.get("https://api.example.com/users/#{params.id}")
    require response.status != 404 else fail 404, "user not found"
    require response.status == 200 else fail 502, "user service failed"
    reply 200, response.body
```

## Test it

A scenario test stubs the call with `given`, so it runs without a live upstream.
This is the right way to test a route that calls a service: you control exactly what
the service returns.

```ruby
scenario "returns the upstream user"
    given http_client.get("https://api.example.com/users/42") returns {
        status: 200,
        body: { id: 42, name: "Ada" }
    }

    when GET "/users/42"
    then status 200
    then response is {
        body: { id: 42, name: "Ada" }
    }

scenario "maps an upstream failure to 502"
    given http_client.get("https://api.example.com/users/99") returns {
        status: 500,
        body: { error: "boom" }
    }

    when GET "/users/99"
    then status 502
```

The `given` matches on the URL, plus the body for `post`, `put`, and `patch`. Query
parameters and headers are not part of the match, so you stub a GET by its URL
alone. See [Test your API](test-your-api.md) for the full testing model.

## Try it

```bash
marreta test
```

The scenarios pass without a live upstream. Unlike a database or cache page, there
is no real provider to curl here, so this page is verified with scenario tests
rather than a live call.

## Result checkpoint

You should now be able to call a service with `http_client`, read the
`.status`/`.body` envelope, guard the status with `require`, and test the route
without a live upstream by stubbing the call.

## Next steps

- [Handle errors](handle-errors.md): map upstream failures to the right status.
- [Cache expensive work](use-cache.md): cache slow upstream responses.
