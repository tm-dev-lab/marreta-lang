---
title: "Inspect the generated OpenAPI docs"
category: how-to
slug: "how-to/openapi-docs"
summary: "Every project serves an OpenAPI spec and Swagger UI generated from its routes and schemas, with no annotations to maintain."
---

# Inspect the generated OpenAPI docs

Every Marreta project generates an OpenAPI document from its routes and schemas and serves
it for you. There are no annotations to write and nothing to keep in sync: the spec is
derived from the same code that runs.

## Open it

Serve the project and the docs are already there:

```bash
# Start the API
marreta serve
```

Visit `http://localhost:8080/docs` in your browser for the interactive Swagger UI. For the
raw document, to feed tooling or a client generator, fetch it with curl:

```bash
curl http://localhost:8080/openapi.json
```

The document is OpenAPI 3.0.3. Its title and version come from `project_name` and
`project_version` in `app.marreta`, and each route appears under a tag named after the file
it lives in.

## Schemas sharpen the contract

The more you describe with schemas, the richer the generated spec. Binding a request body
with `take payload as <Schema>` makes the request a named, typed component and documents the
automatic `422`, and `reply <status> as <Schema>` does the same for the response:

```ruby
route POST "/contracts/echo" take payload as ContractTypesPayload
    reply 200 as ContractTypesPayload, payload
```

A bare `take payload` with no schema still accepts a body, but it shows up as a free-form,
untyped object. For a stable public contract, bind the request and response to schemas.

Query and header schemas sharpen the contract the same way. `take query as <Schema>` and
`take headers as <Schema>` emit one named, typed parameter per field (a repeated-key
`list of <scalar>` becomes an array), while a bare `take query` / `take headers` reads arbitrary
input and contributes no parameters to the spec. So declaring a schema is how a route opts into
documented query and header parameters.

## Configure it

Two variables control the docs, both covered in the
[Configuration reference](../reference/configuration.md):

```bash
# Change the path the docs are served at (default /docs)
MARRETA_DOCS_PATH=/api-docs marreta serve

# Turn the docs off, for example in a locked-down environment (default on)
MARRETA_DOCS_ENABLED=false marreta serve
```

## Notes

- The spec regenerates from the code on every start, so it never drifts from the routes.
- See [Validate a request payload](validate-a-payload.md) and
  [Shape a response](shape-a-response.md) for the schema bindings that feed the contract.
