# Marreta + Microsoft Entra ID Auth Validation

External validation that Marreta accepts a real Microsoft Entra ID access token
for a protected route.

The test can obtain the token with the Azure SDK, but Marreta itself remains
provider-agnostic. Secrets and tokens are passed through environment variables
only and are not written to disk.

## Prerequisites

- `marreta` in `PATH`, or `MARRETA_BIN` pointing to a local binary.
- `curl`.
- `python3`.
- `node` and `npm` if the test should obtain the token with Azure SDK.
- Two Microsoft Entra ID App Registrations:
  - API app: `marreta-auth-api-test`.
  - Client app: `marreta-auth-client-test`.

## Azure Environment

Export these values from the Azure portal:

```bash
export AZURE_TENANT_ID="<Directory tenant ID>"
export AZURE_API_CLIENT_ID="<Application client ID of marreta-auth-api-test>"
export AZURE_CLIENT_ID="<Application client ID of marreta-auth-client-test>"
export AZURE_CLIENT_SECRET="<client secret value>"
```

The expected issuer and audience are derived automatically:

```bash
ENTRA_ISSUER="https://sts.windows.net/${AZURE_TENANT_ID}/"
ENTRA_AUDIENCE="api://${AZURE_API_CLIENT_ID}"
ENTRA_JWKS_URL="https://login.microsoftonline.com/common/discovery/keys"
```

Override them only if your Entra setup uses a different issuer/audience:

```bash
export ENTRA_ISSUER="..."
export ENTRA_AUDIENCE="..."
export ENTRA_JWKS_URL="..."
```

Client credentials tokens issued for this setup are Entra ID v1 access tokens
(`ver = "1.0"`), whose issuer is `https://sts.windows.net/<tenant-id>/`.
The example therefore configures `jwks_url` explicitly instead of relying on
issuer-based OIDC discovery.

The auth provider also sets `algorithm: "RS256"` because Microsoft Entra ID
JWKS entries do not include an `alg` field, and Marreta requires an explicit
algorithm in that case.

## Install Token Dependency

```bash
npm install
```

This installs `@azure/identity`, used only by `get_token.mjs`.

## Run

```bash
./test_entra_id.sh
```

The script:

1. Gets an access token with `get_token.mjs` unless `ENTRA_ACCESS_TOKEN` is
   already set.
2. Starts `marreta serve` locally.
3. Verifies `/_health`.
4. Verifies `/secure` rejects missing token with `401`.
5. Verifies `/secure` accepts the real Entra ID token with `200`.
6. Optionally verifies negative tokens if they are provided.

## Use A Token You Generated Elsewhere

If you prefer to generate the token yourself:

```bash
export ENTRA_ACCESS_TOKEN="<jwt>"
./test_entra_id.sh
```

In that mode `node`, `npm`, and `@azure/identity` are not needed.

## Optional Negative Cases

```bash
export ENTRA_BAD_AUDIENCE_TOKEN="<jwt-for-another-resource>"
export ENTRA_BAD_ISSUER_TOKEN="<jwt-from-another-issuer>"
export ENTRA_EXPIRED_TOKEN="<expired-jwt>"
./test_entra_id.sh
```

Missing optional tokens are reported as `SKIP`.

## Optional Role Check

The project includes `/secure/role`, which requires:

```text
marreta.validation
```

in `auth.user.roles`.

Run it only if the API app role was configured and granted to the client app:

```bash
export ENTRA_TEST_ROLE_CHECK=1
./test_entra_id.sh
```

## Secret Handling

Do not commit:

- `AZURE_CLIENT_SECRET`
- `ENTRA_ACCESS_TOKEN`
- any negative-case JWTs

If a secret is pasted into chat, logs, or shell history, rotate it after the
validation.
