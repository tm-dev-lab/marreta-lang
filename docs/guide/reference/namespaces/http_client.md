---
title: "http_client"
category: namespaces
slug: "reference/namespaces/http_client"
summary: "Call external HTTP services and read the response envelope, guarding the status yourself."
---

# http_client

The `http_client` namespace makes outbound HTTP requests to other services. A
response is an envelope with `.status`, `.body`, and `.headers`. A 4xx or 5xx is not
an error in Marreta, so you decide how to handle each status.

## When to use

Use `http_client` whenever a route needs data from, or an action on, another service.
Always guard `response.status` with `require` before trusting the body, so an upstream
failure becomes a response of your choosing.

See [Call an external API](../../how-to/call-an-external-api.md) for query parameters,
headers, pipeline forms, and testing.

## Operations

Call the verb and read the envelope:

```ruby
response = http_client.get("https://api.example.com/users/#{params.id}")
require response.status == 200 else fail 502, "user service failed"
reply 200, response.body
```

| Name | Signature | Summary |
|---|---|---|
| `http_client.get` | `http_client.get(url, headers:, query:, timeout:)` | Sends a GET request. |
| `http_client.post` | `http_client.post(url, payload, headers:, query:, timeout:)` | Sends a POST request with a body. |
| `http_client.put` | `http_client.put(url, payload, headers:, query:, timeout:)` | Sends a PUT request with a body. |
| `http_client.patch` | `http_client.patch(url, payload, headers:, query:, timeout:)` | Sends a PATCH request with a body. |
| `http_client.delete` | `http_client.delete(url, headers:, query:, timeout:)` | Sends a DELETE request. |

For POST, PUT, and PATCH, you can pipe the body in with `>>` instead of passing it as
an argument: `payload >> http_client.post(url)`. For GET and DELETE, a piped map
becomes the query string.

## Notes

- A 4xx or 5xx response is returned normally, not raised. Guard `response.status`
  yourself and map it to your own status.
- `query:` adds a query string on any verb, `headers:` sets request headers, and
  `timeout:` overrides the default timeout in milliseconds.
- A scenario test stubs a call by its URL (plus the body for `post`, `put`, `patch`),
  so you can test a route without a live upstream.
