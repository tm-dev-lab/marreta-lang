---
title: "Secure your API"
category: how-to
slug: "how-to/secure-your-api"
summary: "Authenticate callers with require auth and authorize them with allow, using API keys or JWTs."
---

# Secure your API

Securing a route has two parts, and Marreta keeps them distinct:

- **Authentication** is who the caller is. You declare an auth provider and gate the
  route with `require auth <provider>`. A failure returns **401**.
- **Authorization** is what the caller may do. You add `allow <expression>`. A
  failure returns **403**.

Both checks run before your route body, and both build the same normalized `auth`
context. Auth failures are standardized and never leak token contents or internal
details.

## Prerequisites

- A scaffolded project (`marreta init app`).
- The [Quickstart](../tutorials/quickstart.md) finished.

## Authenticate with an API key

An API key is the simplest provider and needs no identity provider. It is the
recommended shape for internal service-to-service access, where a caller sends a
fixed key in a header. Declare it with the header to read and a stored hash. You
store only the hash, never the raw key, and it comes from a secret in the
environment:

```ruby
auth api_key main {
    header: "x-api-key"
    secret_hash: env.API_KEY_HASH
    principal: "api-user"
}
```

`secret_hash` must be a hash, either `sha256:<64 hex chars>` or an Argon2id string,
not the raw key. At request time the runtime hashes the incoming header and compares.
The `principal` is who the caller is once the key checks out.

Gate a route with `require auth`. Without a valid `x-api-key`, the request gets a 401
and your code never runs:

```ruby
route GET "/me"
    require auth main
    reply 200, { subject: auth.subject }
```

## Authenticate with a JWT

A JWT lets a client present a signed token instead of a fixed key. The simplest form
uses an HMAC shared secret that you control, with no identity provider and no key
discovery. The same secret signs and validates the token, and the issuer and audience
are checked on every request:

```ruby
auth jwt tokens {
    issuer: "https://my-service"
    audience: "my-api"
    secret: env.JWT_SECRET
    algorithm: "HS256"
}
```

Gate a route the same way as an API key:

```ruby
route GET "/profile"
    require auth tokens
    reply 200, { subject: auth.subject, issuer: auth.claims.iss }
```

For tokens from an external identity provider (Auth0, Cognito, Keycloak, Okta, Entra
ID, Google), drop the secret and give the issuer and audience. The runtime discovers
the signing keys from the issuer (OIDC discovery):

```ruby
auth jwt tokens {
    issuer: "https://issuer.example.com"
    audience: "my-api"
}
```

Some providers want the JWKS endpoint pinned directly. This is a faithful Entra ID
(Azure AD) provider, with placeholder public identifiers, and role-based
authorization on a real route:

```ruby
auth jwt entra_id {
    issuer: "https://sts.windows.net/<your-tenant-id>/"
    audience: "api://<your-app-id>"
    jwks_url: "https://login.microsoftonline.com/common/discovery/keys"
    algorithm: "RS256"
}

route GET "/secure"
    require auth entra_id
    allow "marreta.validation" in auth.user.roles
    reply 200, { ok: true }
```

The issuer, audience, and JWKS URL are public identifiers, so they read clearly
inline. Only true secrets (an API key hash or an HMAC secret) belong in `env.*`.

For a partner's fixed PEM public key instead of OIDC or JWKS, use
`public_key_pem_file` with `algorithm`.

## Read the caller

A protected route gets a normalized `auth` context automatically. A public route has
no `auth`. The fields are:

- `auth.provider` is the provider name, and `auth.type` is `api_key` or `jwt`.
- `auth.subject` and `auth.user.id` identify the caller (the principal for an API
  key, the `sub` claim for a JWT).
- `auth.user.roles` holds the roles from a JWT.
- `auth.claims` holds the raw token claims (`auth.claims.iss`, `auth.claims.aud`).

```ruby
route GET "/whoami"
    require auth tokens
    reply 200, {
        provider: auth.provider,
        type: auth.type,
        subject: auth.subject,
        roles: auth.user.roles
    }
```

## Authorize with allow

Authentication got the caller in. Authorization decides what they may do.
`allow <expression>` asserts any boolean condition after `require auth`. If it is
false, the request gets a 403 before your body runs.

Authorize an API key by its principal:

```ruby
route GET "/admin"
    require auth main
    allow auth.user.id == "api-user"
    reply 200, { ok: true }
```

Authorize a JWT by its roles, and combine conditions freely:

```ruby
route GET "/reports"
    require auth tokens
    allow "analyst" in auth.user.roles or "admin" in auth.user.roles
    reply 200, { ok: true }
```

Roles come from the token, so role-based `allow` belongs with a `jwt` provider. An
`api_key` provider authenticates a single principal and carries no roles, so you
authorize it on `auth.user.id`.

## Scope data to the caller

The `auth` context is ordinary data you can use in the route. The most common pattern
is to scope results to the authenticated caller, so each user only sees their own
records (this assumes an `orders` table, see
[Persist data with local services](use-local-services.md)):

```ruby
route GET "/my/orders"
    require auth tokens
    orders = db.orders >> where(owner: auth.subject) >> fetch
    reply 200, { items: orders }
```

## Test it

A scenario test stubs the provider with `given auth.<provider>`, so it runs without
real credentials. For an API key the principal comes from the declaration, so the
stub is empty. For a JWT, the stub supplies `sub` and `roles`. Omitting the `given`
exercises the unauthenticated path:

```ruby
scenario "api key authenticates"
    given auth.main returns {}
    when GET "/me"
    then status 200

scenario "missing credentials is 401"
    when GET "/me"
    then status 401

scenario "the principal is allowed"
    given auth.main returns {}
    when GET "/admin"
    then status 200

scenario "a jwt without the role is forbidden"
    given auth.tokens returns { sub: "user-1", roles: ["viewer"] }
    when GET "/reports"
    then status 403

scenario "a jwt with the role is allowed"
    given auth.tokens returns { sub: "user-1", roles: ["admin"] }
    when GET "/reports"
    then status 200
```

```bash
marreta test
```

The scenarios pass without real credentials. The `given` provides the verified
identity, so you test authentication and authorization logic without minting tokens.

**Important:** a scenario test assumes an already-verified identity and exercises only
your route logic (the `require` and `allow` decisions). It does **not** validate token
cryptography, signatures, issuer, audience, or JWKS. That validation is the runtime's
responsibility, and you should cover it with runtime or live authentication tests.
`marreta test` is not a substitute for real token validation.

## Result checkpoint

You should now be able to authenticate a caller with an API key or a JWT, read the
caller from the `auth` context, and authorize with `allow`, with 401 for missing
credentials and 403 for a failed rule.

## Common pitfalls

- **`secret_hash` set to the raw key.** It must be `sha256:<64 hex chars>` or an
  Argon2id string. The project fails to load otherwise.
- **A committed secret.** The hash and JWT secret come from `env.*`, so keep them in
  `marreta.env` (gitignored) or real environment variables, never in source.
- **Role-based `allow` on an API key.** An API key has a principal but no roles, so
  `"role" in auth.user.roles` is always false. Authorize an API key on
  `auth.user.id`, and use a `jwt` provider for role-based rules.

## Next steps

- [Configure environment variables](configure-environment.md): where the auth
  secrets live.
- [Handle errors](handle-errors.md): the 401 and 403 responses.
