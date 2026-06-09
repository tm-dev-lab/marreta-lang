# 024 - Authentication and Authorization

Status: Delivered

## Delivery notes

- Delivered project-wide `auth jwt` and `auth api_key` providers.
- Delivered route-level `require auth <provider>` and `allow <expression>`.
- Delivered API key validation with safe hash comparison.
- Delivered API key `secret_hash` support for Argon2id PHC strings and explicit
  `sha256:<hex>` fixture hashes.
- Delivered JWT validation for issuer-derived OIDC discovery, explicit `jwks_url`,
  fixed `public_key_pem` / `public_key_pem_file`, and fixed HMAC `secret`.
- Delivered normalized automatic `auth` context for protected routes.
- Delivered sanitized `401` authentication and `403` authorization failures.
- Delivered OpenAPI security schemes and route-level security requirements.
- Delivered `marreta doctor` auth configuration reporting.
- Delivered `marreta test` auth provider mocks with `given auth.<provider>`.
- Delivered functional examples for API key plus JWT provider shapes.
- Delivered deterministic auth fixtures for RSA, EC, HMAC, JWKS, and OIDC
  discovery tests.

## Motivation

Marreta is a language for building REST APIs. Authentication and authorization
must therefore be first-class API behavior, not hand-written infrastructure code
inside every route.

The developer should describe access rules close to the route, using language
that reads like the API contract:

```marreta
auth jwt customer_auth {
    issuer: env.MARRETA_AUTH_CUSTOMER_ISSUER
    audience: env.MARRETA_AUTH_CUSTOMER_AUDIENCE
}

route GET "/orders"
    require auth customer_auth
    allow "customer" in auth.user.roles or "admin" in auth.user.roles

    reply 200, { user_id: auth.user.id }
```

The runtime is responsible for extracting credentials, validating them, building
the authenticated request context, enforcing authorization rules, and returning
standard `401` or `403` responses before the route body runs.

## Goals

- Add declarative authentication provider definitions.
- Add route-level authentication with `require auth <auth_provider>`.
- Add route-level authorization with `allow <boolean expression>`.
- Expose authenticated context automatically as `auth` inside protected routes.
- Support JWT bearer tokens as the first production-grade auth mechanism.
- Support API keys for simple internal/service-to-service APIs, including
  hashed secrets for production use.
- Generate OpenAPI security schemes from Marreta auth definitions.
- Make auth testable through `marreta test` scenario headers.
- Keep the route body focused on business behavior, not token parsing.
- Keep auth errors standardized and safe by default.

## Non-goals

- OAuth login flows.
- Refresh tokens.
- Session storage.
- Cookie-based browser auth.
- CSRF protection.
- RBAC or ABAC policy engines.
- Password hashing or user registration.
- Multi-factor authentication.
- Distributed rate limiting.
- Tenant isolation.
- Secrets manager integrations.
- Custom Rust auth plugins.
- Replacing infrastructure-level controls such as API gateways or WAFs.

## Required Semantics

These are language/runtime obligations, not recommendations.

1. A route with `require auth <provider>` must not execute its body unless
   authentication succeeds.
2. A route with `allow <expr>` must not execute its body unless all auth
   checks and all authorization expressions succeed.
3. Authentication failure returns `401`.
4. Authorization failure returns `403`.
5. Auth failures must not expose token contents, secrets, internal validation
   details, stack traces, provider URLs, or runtime errors to the client.
6. Auth failures must be logged with safe diagnostic context.
7. User-authored `reply` and `fail` responses remain untouched when the route
   body is reached.
8. Protected routes receive an `auth` context automatically.
9. Public routes cannot access `auth`; this must be rejected during project
   loading. `marreta doctor` surfaces the same error before the server starts.
10. `allow` expressions may read `auth`, `params`, `query`, `headers`, and
   config/env values, but must not call external components in v1.
11. `allow` expressions intentionally cannot read the request body;
    authorization decisions must not depend on payload parsing in v1.
12. Route auth clauses must appear in canonical order: one `require auth`,
    followed by all `allow` clauses, followed by the route body. Project loading
    rejects clauses outside this order.
13. Auth providers are loaded at project startup. Invalid auth configuration is
    a project loading error.
14. `marreta doctor` must validate auth provider configuration without requiring
    real user credentials.
15. `marreta test` runs requests through the real auth runtime path. The
    `given auth.<provider>` helper short-circuits token validation at the
    provider boundary while preserving downstream `require auth` and `allow`
    enforcement.
16. Auth providers are project-scoped. Any route loaded by the project may
    reference any auth provider by name.
17. Auth provider names must be unique within the project.

The key rule:

```text
Auth is enforced before route execution.
Routes describe access intent.
The runtime owns credential validation.
```

## Auth Providers

Auth providers are declared at project level.

Because Marreta does not have explicit imports, auth providers are top-level
project declarations discovered by the project loader. A provider may live in
`app.marreta`, beside related routes, or in a dedicated organization folder.
This is an organization example, not a loader requirement:

```text
app.marreta
auth/
  customer_auth.marreta
  internal_auth.marreta
routes/
  orders.marreta
  customers.marreta
```

Example:

```marreta
# auth/customer_auth.marreta
auth jwt customer_auth {
    issuer: env.MARRETA_AUTH_CUSTOMER_ISSUER
    audience: env.MARRETA_AUTH_CUSTOMER_AUDIENCE
}
```

```marreta
# routes/orders.marreta
route GET "/orders"
    require auth customer_auth
    allow "customer" in auth.user.roles or "admin" in auth.user.roles

    reply 200, { ok: true }
```

The provider is defined once and can be reused by any route in any loaded route
file.

### JWT Bearer

```marreta
auth jwt customer_auth {
    issuer: env.MARRETA_AUTH_CUSTOMER_ISSUER
    audience: env.MARRETA_AUTH_CUSTOMER_AUDIENCE
}
```

Runtime behavior:

- Reads `Authorization: Bearer <token>`.
- Validates JWT signature against JWKS.
- Validates issuer.
- Validates audience.
- Validates expiration.
- Validates not-before (`nbf`) when present.
- Rejects unsupported algorithms.
- Builds the `auth` context with normalized `user` and `claims` subfields.

This is intended to work with JWTs issued by external identity providers such as
Auth0, Keycloak, AWS Cognito, Azure AD, Okta, or another OIDC-compatible IdP.
Marreta does not issue tokens in this spec; it validates tokens issued by the
configured provider.

Required fields:

- `issuer`
- `audience`

Optional fields:

- `subject_claim`, default: `"sub"`
- `user_id_claim`, default: same as `subject_claim`
- `roles_claim`, default: `"roles"`
- `email_claim`, default: `"email"`
- `jwks_url`, explicit JWKS URL escape hatch
- `public_key_pem`, fixed public key string for asymmetric JWT validation
- `public_key_pem_file`, path to a fixed public key PEM file
- `secret`, fixed shared secret for HMAC JWT validation
- `algorithm`, required when using `public_key_pem`, `public_key_pem_file`, or
  `secret`
- `jwks_cache_ttl_seconds`, default: runtime-defined safe value
- `clock_skew_seconds`, default: small runtime-defined safe value

The runtime must not accept unsigned JWTs.

When `jwks_url` is not configured, Marreta derives the OIDC discovery URL from
the issuer:

```text
{issuer}/.well-known/openid-configuration
```

Marreta fetches the OIDC discovery document and uses its `jwks_uri`. This is
the default low-ceremony path for OIDC-compatible identity providers.

```marreta
auth jwt customer_auth {
    issuer: env.MARRETA_AUTH_CUSTOMER_ISSUER
    audience: env.MARRETA_AUTH_CUSTOMER_AUDIENCE
}
```

Using `jwks_url` directly remains valid and avoids discovery network calls at
startup.

Fixed public keys are supported for partners or internal deployments that do not
expose JWKS:

```marreta
auth jwt partner_auth {
    issuer: env.MARRETA_AUTH_PARTNER_ISSUER
    audience: env.MARRETA_AUTH_PARTNER_AUDIENCE
    algorithm: "RS256"
    public_key_pem_file: env.MARRETA_AUTH_PARTNER_PUBLIC_KEY_FILE
}
```

`public_key_pem_file` is the recommended form for projects and examples because
PEM files are naturally multiline. `public_key_pem` remains available for
runtimes that inject the PEM string directly.

The file path should normally come from environment configuration, not from a
hardcoded route file value. This keeps the same Marreta source portable across
local development, CI/CD, containers, and production secret mounts:

```env
MARRETA_AUTH_PARTNER_PUBLIC_KEY_FILE=/run/secrets/partner-public.pem
```

Relative `public_key_pem_file` paths are resolved from the project root, which
is also the command working directory for `marreta serve`, `marreta test`, and
`marreta doctor`. The path value has the same trust boundary as any other
environment-sourced secret/config value.

Fixed shared secrets are supported for controlled HMAC JWT deployments:

```marreta
auth jwt internal_jwt {
    issuer: env.MARRETA_AUTH_INTERNAL_JWT_ISSUER
    audience: env.MARRETA_AUTH_INTERNAL_JWT_AUDIENCE
    algorithm: "HS256"
    secret: env.MARRETA_AUTH_INTERNAL_JWT_SECRET
}
```

Shared-secret JWTs are more sensitive because the same secret can validate and
issue tokens. They are useful for internal systems, but OIDC/JWKS or
asymmetric public-key validation should be preferred for user-facing APIs.

Validation source rules:

- `jwks_url`, `public_key_pem`, `public_key_pem_file`, and `secret` are
  mutually exclusive.
- If no validation source is configured, Marreta uses OIDC discovery from
  `issuer`.
- `algorithm` is required when using `public_key_pem`,
  `public_key_pem_file`, or `secret`.
- `secret` is valid only for HMAC algorithms such as `HS256`, `HS384`, or
  `HS512`.
- `public_key_pem` and `public_key_pem_file` are valid only for asymmetric
  algorithms such as `RS256` or `ES256`.
- The configured algorithm must match the token header; the runtime must never
  trust the token algorithm by itself.

### API Key

```marreta
auth api_key internal_auth {
    header: "x-api-key"
    secret_hash: env.MARRETA_AUTH_INTERNAL_API_KEY_HASH
}
```

Runtime behavior:

- Reads the configured header.
- Compares the supplied value with the configured secret or secret hash.
- Builds a minimal `auth` context.

Required fields:

- `header`
- one secret source: `secret_hash` or `secret`

Optional fields:

- `principal`, default: provider name

API keys are intended for internal APIs and service-to-service communication.
They are not a replacement for user identity.

`secret_hash` is the production-oriented option because the configured runtime
secret is not the raw API key. Marreta supports Argon2id PHC hashes
(`$argon2id$...`) and explicit SHA-256 hashes (`sha256:<hex>`). Argon2id is the
recommended production format; SHA-256 is intended for high-entropy API keys,
local fixtures, and deterministic examples. `secret` remains valid for local
development, tests, and simple internal deployments.

## Environment Configuration

Auth values that vary by environment should live in `marreta.env` or real
process environment variables. As with the rest of Marreta configuration, real
environment variables provided by CI/CD, containers, or the host override values
from `marreta.env`.

Auth examples should use the `MARRETA_AUTH_<PROVIDER>_<SETTING>` convention to
avoid collisions with generic environment variables:

```env
MARRETA_AUTH_CUSTOMER_ISSUER=https://accounts.example.com
MARRETA_AUTH_CUSTOMER_AUDIENCE=customer-api

MARRETA_AUTH_PARTNER_ISSUER=https://partner.example.com
MARRETA_AUTH_PARTNER_AUDIENCE=partner-api
MARRETA_AUTH_PARTNER_PUBLIC_KEY_FILE=secrets/partner-public.pem

MARRETA_AUTH_INTERNAL_API_KEY_HASH=$argon2id$v=19$m=...
```

The language does not require these exact names because auth providers reference
environment values explicitly with `env.KEY`. The naming convention is the
recommended project standard.

`marreta test` uses the same environment loading rules as `marreta serve`,
`marreta doctor`, and migration commands: values from the project `marreta.env`
are loaded first, and real process environment variables may override them.
This allows scenario tests to use local fixture paths while CI/CD or production
can point the same provider at mounted secrets.

Multiple identity providers are modeled as multiple auth providers:

```marreta
auth jwt google_auth {
    issuer: env.MARRETA_AUTH_GOOGLE_ISSUER
    audience: env.MARRETA_AUTH_GOOGLE_AUDIENCE
}

auth jwt microsoft_auth {
    issuer: env.MARRETA_AUTH_MICROSOFT_ISSUER
    audience: env.MARRETA_AUTH_MICROSOFT_AUDIENCE
}
```

In v1, each route requires one provider:

```marreta
route GET "/google/profile"
    require auth google_auth

    reply 200, { provider: auth.provider, user_id: auth.user.id }

route GET "/microsoft/profile"
    require auth microsoft_auth

    reply 200, { provider: auth.provider, user_id: auth.user.id }
```

Accepting more than one provider on the same route is deferred:

```marreta
route GET "/profile"
    require auth google_auth or microsoft_auth

    reply 200, { provider: auth.provider, user_id: auth.user.id }
```

## Route Syntax

### Authentication

```marreta
route GET "/orders"
    require auth customer_auth

    reply 200, { ok: true }
```

`require auth` binds the route to a declared auth provider.

If the provider is missing or invalid, project loading fails.

The v1 syntax accepts exactly one provider per route. The AST/runtime design
should not block future provider composition such as:

```marreta
require auth customer_auth or internal_auth
```

Composition is deferred because it affects OpenAPI generation, error precedence,
and test helper semantics.

### Authorization

```marreta
route GET "/orders"
    require auth customer_auth
    allow "customer" in auth.user.roles or "admin" in auth.user.roles

    reply 200, { ok: true }
```

`allow` evaluates after authentication succeeds and before route execution.

Multiple `allow` statements are allowed and evaluated in declaration order:

```marreta
route GET "/orders/:id"
    require auth customer_auth
    allow "customer" in auth.user.roles or "admin" in auth.user.roles
    allow auth.claims.tenant_id == params.tenant_id

    reply 200, { ok: true }
```

All authorization checks must pass.

Multiple `allow` statements are equivalent to joining the expressions with
logical `and`; the separate form is preferred when each rule deserves its own
line.

Ownership checks are a primary authorization use case and should remain simple:

```marreta
route GET "/users/:id/orders"
    require auth customer_auth
    allow auth.user.id == params.id or "admin" in auth.user.roles

    reply 200, db.orders.where(user_id == params.id).fetch()
```

This keeps common API rules close to the route without requiring a separate
policy engine.

### Public Routes

Routes without `require auth` remain public:

```marreta
route GET "/products"
    reply 200, db.products.all()
```

Public routes cannot access `auth`.
Attempting to do so fails project loading.

## Auth Context

Protected routes receive `auth` automatically. No `take` binding is required.

```marreta
route GET "/me"
    require auth customer_auth

    reply 200, {
        id: auth.user.id,
        roles: auth.user.roles,
        provider: auth.provider
    }
```

### `auth`

Returns metadata about the authenticated request:

```marreta
{
    provider: "customer_auth",
    type: "jwt",
    subject: "user-123",
    user: {
        id: "user-123",
        subject: "user-123",
        email: "ana@example.com",
        roles: ["customer"]
    },
    claims: {
        sub: "user-123",
        roles: ["customer"]
    }
}
```

`auth.type` is `"jwt"` or `"api_key"`.

`auth.user` is the preferred path for common authorization checks. It contains
normalized fields derived from the configured provider claims:

- `id`
- `subject`
- `email`
- `roles`

`auth.claims` returns validated JWT claims as a map and remains available for
provider-specific or advanced authorization checks.

For API key providers, `auth.claims` is an empty map and `auth` has a minimal
normalized identity. `subject` and `auth.user.id` use the configured
`principal`, or the provider name when `principal` is not configured:

```marreta
{
    provider: "internal_auth",
    type: "api_key",
    subject: "internal_auth",
    user: {
        id: "internal_auth",
        subject: "internal_auth",
        roles: []
    },
    claims: {}
}
```

The normalized shape keeps route code independent from provider-specific token
details.

## Error Responses

Default auth failure responses are intentionally small.

Authentication failure:

```json
{
  "error": "unauthorized"
}
```

Authorization failure:

```json
{
  "error": "forbidden"
}
```

The runtime may include stable error codes later, but must not include token
details, provider URLs, claim values, Rust errors, stack traces, or line traces
in HTTP auth failure responses.

## OpenAPI

Auth provider definitions generate OpenAPI security schemes.

JWT:

```yaml
components:
  securitySchemes:
    customer_auth:
      type: http
      scheme: bearer
      bearerFormat: JWT
```

API key:

```yaml
components:
  securitySchemes:
    internal_auth:
      type: apiKey
      in: header
      name: x-api-key
```

Routes using `require auth` include the corresponding security requirement.

Public routes do not include route-level security requirements.

Reserved runtime endpoints such as `/_health`, `/docs`, and `/openapi.json` are
governed by server configuration specs, not by route-level `require auth`.

## Scenario Testing

Scenario tests use real request headers.

`given auth.<provider> returns { ... }` returns validated provider claims. The
scenario runtime normalizes that map into `auth.user` and `auth.claims` using
the provider's configured claim mappings, exactly as a real token would.

API key providers are exercised by sending the configured header directly in
`when ... headers { ... }`. No `given auth.<provider>` declaration is required.

Successful request:

```marreta
scenario "reads own orders"
    given auth.customer_auth returns {
        sub: "user-123",
        roles: ["customer"]
    }

    when GET "/orders" headers {
        authorization: "Bearer test-token"
    }

    then status 200
```

Unauthorized request:

```marreta
scenario "rejects missing token"
    when GET "/orders"

    then response is {
        status: 401,
        body: { error: "unauthorized" }
    }
```

Forbidden request:

```marreta
scenario "rejects unsupported role"
    given auth.customer_auth returns {
        sub: "user-123",
        roles: ["guest"]
    }

    when GET "/orders" headers {
        authorization: "Bearer test-token"
    }

    then response is {
        status: 403,
        body: { error: "forbidden" }
    }
```

The exact test helper shape is part of this spec because using real signed JWTs
in every scenario would make tests noisy and brittle. The scenario runtime may
mock auth providers at the auth boundary while preserving route-level auth and
authorization behavior.

## Auth Test Strategy

Auth tests must be deterministic and must not depend on a real external identity
provider.

Runtime and server tests are responsible for validating real token verification.
Those tests should generate JWTs from Rust test helpers, not from Marreta code:

- RS256/ES256 tests use local test key fixtures.
- JWKS/OIDC tests use a local stub server. Static fixture files back the stub
  responses.
- HMAC tests sign tokens with a local test secret.
- Invalid-token tests mutate signature, issuer, audience, expiration, `nbf`, or
  algorithm.

The recommended fixture layout is:

```text
tests/fixtures/auth/
  rsa_private_key.pem
  rsa_public_key.pem
  ec_private_key.pem
  ec_public_key.pem
  hmac_secret.txt
  jwks.json
  openid-configuration.json
```

Scenario tests are not the primary place to validate JWT cryptography. For JWT
providers, scenario tests use:

```marreta
given auth.customer_auth returns {
    sub: "user-123",
    email: "ana@example.com",
    roles: ["admin"]
}
```

This keeps scenario tests focused on route behavior, `require auth`, `allow`,
and `auth.user` normalization.

JWT providers that use `public_key_pem_file` should still read the path from the
project environment. Example projects may set the variable in `marreta.env` to a
local fixture path, while CI/CD can override it with a mounted secret path
without changing the Marreta source.

API key scenario tests send the configured header directly:

```marreta
when GET "/internal" headers {
    x-api-key: "test-api-key"
}
```

No test should call Auth0, Keycloak, Cognito, Azure AD, Okta, or any other real
IdP.

When the deferred deep JWKS connectivity check lands, it must be opt-in, for
example behind a `--network` flag, and excluded from default CI runs.

## Doctor

`marreta doctor` validates:

- Referenced auth providers exist.
- Auth provider names are unique.
- Required provider fields are configured.
- Referenced environment variables are present.
- JWT issuer/audience values are configured.
- JWT explicit `jwks_url`, when present, is syntactically valid.
- JWT fixed public keys, when present, are valid PEM public keys.
- JWT `public_key_pem_file`, when present, points to a readable PEM public key.
- JWT HMAC secrets, when present, are not empty.
- JWT validation sources are mutually exclusive.
- JWT algorithms are compatible with the configured validation source.
- API key headers are valid HTTP header names.
- API key `secret` or `secret_hash` values are not empty.
- Routes do not use `allow` without `require auth`.
- Public routes do not access `auth`.

`doctor` should not require real user tokens.

For JWT JWKS connectivity, `doctor` may support a deeper check later.

## Implementation Plan

### Phase 1 - AST and Parser

- Add auth provider definitions to the AST.
- Add route-level `require auth <identifier>`.
- Add route-level `allow <expression>`.
- Add automatic `auth` context for protected routes.
- Keep new DSL words contextual where possible.

### Phase 2 - Route Registry

- Discover auth provider declarations while loading project files.
- Store auth provider definitions in project runtime metadata.
- Store route auth requirements and authorization expressions in
  `RouteDefinition`.
- Reject duplicate auth provider names.
- Validate provider references during project loading.

### Phase 3 - Runtime Auth Context

- Add request auth context type.
- Add authentication result model.
- Inject auth context into route execution.
- Make `auth` read from that context.
- Ensure public routes cannot access auth context.

### Phase 4 - JWT Provider

- Add JWT validation dependency.
- Support issuer-based OIDC discovery.
- Support explicit `jwks_url`.
- Fetch and cache JWKS for issuer-based OIDC discovery or explicit `jwks_url`.
- Support fixed `public_key_pem` and `public_key_pem_file`.
- Support fixed HMAC `secret`.
- Validate signature, issuer, audience, expiration, not-before (`nbf`), and algorithm.
- Reject incompatible algorithm/source combinations.
- Build the `auth` context with normalized `user` and `claims` subfields.
- Redact token/provider details from runtime errors.

### Phase 5 - API Key Provider

- Validate configured header and secret source.
- Extract API key from request headers.
- Compare raw secrets safely.
- Compare hashed secrets safely.
- Build minimal auth context.

### Phase 6 - Authorization Evaluation

- Evaluate `allow` after authentication.
- Return `403` before route body execution when authorization fails.
- Restrict v1 authorization expressions from external calls.
- Preserve Marreta trace internally without leaking it to the HTTP response.

### Phase 7 - OpenAPI

- Generate security schemes for JWT and API key providers.
- Attach route security requirements.
- Keep public routes public in generated docs.

### Phase 8 - Doctor and Tests

- Add doctor checks for provider config and route usage.
- Add unit parser tests.
- Add route-loader tests.
- Add server tests for `401`, `403`, and success.
- Add scenario tests for auth helpers.
- Add functional examples.

## Test Plan

### Phase 1

- Parse JWT provider definitions.
- Parse API key provider definitions.
- Parse `require auth`.
- Parse single and multiple `allow`.
- Reject malformed auth provider blocks.
- Reject malformed `require auth` and `allow` route clauses.

### Phase 2

- Load project with auth providers.
- Fail loading when a route references an unknown auth provider.
- Fail loading when two auth providers use the same name.
- Preserve public routes without auth metadata.
- Store multiple authorization expressions in route order.

### Phase 3

- `auth` returns provider metadata in protected routes.
- `auth.user` returns normalized identity.
- `auth.claims` returns JWT claims.
- Public route using `auth` fails at project loading.

### Phase 4

- Valid JWT succeeds.
- Valid JWT generated by a Rust test helper succeeds.
- Valid JWT using issuer-derived OIDC discovery succeeds.
- Valid JWT using explicit `jwks_url` succeeds.
- Valid JWT using fixed `public_key_pem` succeeds.
- Valid JWT using fixed `public_key_pem_file` succeeds.
- Valid RS256 JWT with fixed public key succeeds.
- Valid ES256 JWT with fixed public key succeeds.
- Valid JWT using fixed HMAC `secret` succeeds.
- Missing bearer token returns `401`.
- Invalid signature returns `401`.
- Expired token returns `401`.
- Token with invalid `nbf` returns `401`.
- Wrong issuer returns `401`.
- Wrong audience returns `401`.
- Unsupported algorithm returns `401`.
- Token signed with a different algorithm family than configured returns `401`.
- Incompatible algorithm/source configuration fails doctor/config validation.
- HTTP response never leaks token details.

### Phase 5

- Valid API key succeeds.
- Valid API key with `secret_hash` succeeds.
- Missing API key returns `401`.
- Wrong API key returns `401`.
- Empty configured secret or secret hash fails doctor/config validation.

### Phase 6

- `allow` true lets the route execute.
- `allow` false returns `403`.
- Multiple `allow` clauses require all checks to pass.
- Route body side effects do not run on `401` or `403`.
- Authorization expression errors return safe server errors and log internally.

### Phase 7

- JWT provider appears as bearer security scheme.
- API key provider appears as header API key security scheme.
- Protected route includes security requirement.
- Public route has no route-level security requirement.

### Phase 8

- `doctor` reports missing auth env vars.
- `doctor` reports unknown provider references.
- `doctor` reports `allow` without `require auth`.
- `doctor` reports public route accessing `auth`.
- Scenario tests cover JWT success, unauthorized, and forbidden flows with
  `given auth.<provider>`.
- Scenario tests cover API key success and unauthorized flows with request
  headers.

## Deferred

- OAuth authorization-code flow.
- Cookie/session auth.
- CSRF protection.
- Refresh token lifecycle.
- Role policy declarations outside routes.
- Permission registry.
- Tenant-aware auth helpers.
- External policy engine integration.
- Auth provider composition such as `require auth customer_auth or internal_auth`.
- Optional route auth.
- Real signed JWT generation helpers for `marreta test`.
- Deep JWKS connectivity check in `doctor`.
- Rate limiting.
- Audit log sink.
- Secrets manager integration.

## Open Questions

- Should `allow` support task calls in a future version, or remain
  expression-only?
- Should `auth.user` be configurable per provider beyond `user_id_claim`?
