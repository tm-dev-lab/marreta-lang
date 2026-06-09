use std::collections::{HashMap, HashSet};

use argon2::PasswordHash;
use axum::http::HeaderName;
use jsonwebtoken::DecodingKey;
use reqwest::Url;

use crate::ast::{AuthProvider, AuthProviderConfig, Expression};
use crate::error::MarretaError;

const DEFAULT_SUBJECT_CLAIM: &str = "sub";
const DEFAULT_ROLES_CLAIM: &str = "roles";
const DEFAULT_EMAIL_CLAIM: &str = "email";
const DEFAULT_JWKS_CACHE_TTL_SECONDS: u64 = 300;
const DEFAULT_CLOCK_SKEW_SECONDS: u64 = 60;
const MAX_PUBLIC_KEY_PEM_FILE_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct AuthRegistry {
    pub providers: HashMap<String, AuthProviderRuntimeConfig>,
}

impl AuthRegistry {
    pub fn empty() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AuthProviderRuntimeConfig {
    Jwt(JwtAuthConfig),
    ApiKey(ApiKeyAuthConfig),
}

#[derive(Debug, Clone, PartialEq)]
pub struct JwtAuthConfig {
    pub name: String,
    pub issuer: String,
    pub audience: String,
    pub subject_claim: String,
    pub user_id_claim: String,
    pub roles_claim: String,
    pub email_claim: String,
    pub validation_source: JwtValidationSource,
    pub algorithm: Option<String>,
    pub jwks_cache_ttl_seconds: u64,
    pub clock_skew_seconds: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum JwtValidationSource {
    OidcDiscovery,
    JwksUrl(String),
    PublicKeyPem(String),
    Secret(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ApiKeyAuthConfig {
    pub name: String,
    pub header: String,
    pub secret_source: ApiKeySecretSource,
    pub principal: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ApiKeySecretSource {
    SecretHash(String),
    Secret(String),
}

pub fn build_auth_registry(
    providers: &HashMap<String, AuthProvider>,
) -> Result<AuthRegistry, MarretaError> {
    let mut runtime_providers = HashMap::new();

    for (name, provider) in providers {
        let runtime = match provider {
            AuthProvider::Jwt(config) => AuthProviderRuntimeConfig::Jwt(build_jwt_config(config)?),
            AuthProvider::ApiKey(config) => {
                AuthProviderRuntimeConfig::ApiKey(build_api_key_config(config)?)
            }
        };
        runtime_providers.insert(name.clone(), runtime);
    }

    Ok(AuthRegistry {
        providers: runtime_providers,
    })
}

fn build_jwt_config(config: &AuthProviderConfig) -> Result<JwtAuthConfig, MarretaError> {
    let fields = AuthFieldMap::new(
        &config.name,
        "jwt",
        &[
            "issuer",
            "audience",
            "subject_claim",
            "user_id_claim",
            "roles_claim",
            "email_claim",
            "jwks_url",
            "public_key_pem",
            "public_key_pem_file",
            "secret",
            "algorithm",
            "jwks_cache_ttl_seconds",
            "clock_skew_seconds",
        ],
        config,
    )?;

    let issuer = fields.required_string("issuer")?;
    let audience = fields.required_string("audience")?;
    let subject_claim = fields
        .optional_string("subject_claim")?
        .unwrap_or_else(|| DEFAULT_SUBJECT_CLAIM.into());
    let user_id_claim = fields
        .optional_string("user_id_claim")?
        .unwrap_or_else(|| subject_claim.clone());
    let roles_claim = fields
        .optional_string("roles_claim")?
        .unwrap_or_else(|| DEFAULT_ROLES_CLAIM.into());
    let email_claim = fields
        .optional_string("email_claim")?
        .unwrap_or_else(|| DEFAULT_EMAIL_CLAIM.into());
    let jwks_url = fields.optional_string("jwks_url")?;
    let public_key_pem = match (
        fields.optional_string("public_key_pem")?,
        fields.optional_string("public_key_pem_file")?,
    ) {
        (Some(_), Some(_)) => {
            return auth_config_error(
                &config.name,
                "public_key_pem and public_key_pem_file are mutually exclusive",
            );
        }
        (Some(pem), None) => Some(pem),
        (None, Some(path)) => Some(read_public_key_pem_file(&config.name, &path)?),
        (None, None) => None,
    };
    let secret = fields.optional_string("secret")?;
    let algorithm = fields.optional_string("algorithm")?;
    let jwks_cache_ttl_seconds = fields
        .optional_u64("jwks_cache_ttl_seconds")?
        .unwrap_or(DEFAULT_JWKS_CACHE_TTL_SECONDS);
    let clock_skew_seconds = fields
        .optional_u64("clock_skew_seconds")?
        .unwrap_or(DEFAULT_CLOCK_SKEW_SECONDS);

    let configured_sources = [
        jwks_url.as_ref().map(|_| "jwks_url"),
        public_key_pem.as_ref().map(|_| "public_key_pem"),
        secret.as_ref().map(|_| "secret"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();

    if configured_sources.len() > 1 {
        return auth_config_error(
            &config.name,
            "jwt validation sources are mutually exclusive: use only one of jwks_url, public_key_pem, public_key_pem_file, or secret",
        );
    }

    if (public_key_pem.is_some() || secret.is_some()) && algorithm.is_none() {
        return auth_config_error(
            &config.name,
            "algorithm is required when using public_key_pem, public_key_pem_file, or secret",
        );
    }

    if let Some(alg) = &algorithm {
        validate_supported_algorithm(&config.name, alg)?;
        if secret.is_some() && !is_hmac_algorithm(alg) {
            return auth_config_error(
                &config.name,
                "secret is valid only with HMAC algorithms HS256, HS384, or HS512",
            );
        }
        if public_key_pem.is_some() && !is_asymmetric_algorithm(alg) {
            return auth_config_error(
                &config.name,
                "public_key_pem and public_key_pem_file are valid only with asymmetric algorithms such as RS256 or ES256",
            );
        }
        if let Some(pem) = public_key_pem.as_deref() {
            validate_public_key_pem(&config.name, pem, alg)?;
        }
    }

    if let Some(url) = jwks_url.as_deref() {
        validate_http_url(&config.name, "jwks_url", url)?;
    }

    let validation_source = if let Some(url) = jwks_url {
        JwtValidationSource::JwksUrl(url)
    } else if let Some(pem) = public_key_pem {
        JwtValidationSource::PublicKeyPem(pem)
    } else if let Some(value) = secret {
        JwtValidationSource::Secret(value)
    } else {
        JwtValidationSource::OidcDiscovery
    };

    Ok(JwtAuthConfig {
        name: config.name.clone(),
        issuer,
        audience,
        subject_claim,
        user_id_claim,
        roles_claim,
        email_claim,
        validation_source,
        algorithm,
        jwks_cache_ttl_seconds,
        clock_skew_seconds,
    })
}

fn build_api_key_config(config: &AuthProviderConfig) -> Result<ApiKeyAuthConfig, MarretaError> {
    let fields = AuthFieldMap::new(
        &config.name,
        "api_key",
        &["header", "secret_hash", "secret", "principal"],
        config,
    )?;

    let header = fields.required_string("header")?;
    let secret_hash = fields.optional_string("secret_hash")?;
    let secret = fields.optional_string("secret")?;
    let principal = fields
        .optional_string("principal")?
        .unwrap_or_else(|| config.name.clone());
    validate_api_key_header(&config.name, &header)?;

    let secret_source = match (secret_hash, secret) {
        (Some(hash), None) => {
            validate_secret_hash(&config.name, &hash)?;
            ApiKeySecretSource::SecretHash(hash)
        }
        (None, Some(value)) => ApiKeySecretSource::Secret(value),
        (None, None) => {
            return auth_config_error(
                &config.name,
                "api_key auth requires one secret source: secret_hash or secret",
            );
        }
        (Some(_), Some(_)) => {
            return auth_config_error(
                &config.name,
                "api_key secret_hash and secret are mutually exclusive",
            );
        }
    };

    Ok(ApiKeyAuthConfig {
        name: config.name.clone(),
        header,
        secret_source,
        principal,
    })
}

struct AuthFieldMap<'a> {
    provider_name: &'a str,
    fields: HashMap<String, &'a Expression>,
}

impl<'a> AuthFieldMap<'a> {
    fn new(
        provider_name: &'a str,
        provider_type: &str,
        allowed_fields: &[&str],
        config: &'a AuthProviderConfig,
    ) -> Result<Self, MarretaError> {
        let allowed = allowed_fields.iter().copied().collect::<HashSet<_>>();
        let mut fields = HashMap::new();

        for field in &config.fields {
            if !allowed.contains(field.name.as_str()) {
                return auth_config_error(
                    provider_name,
                    &format!("unknown {} auth field '{}'", provider_type, field.name),
                );
            }
            if fields.insert(field.name.clone(), &field.value).is_some() {
                return auth_config_error(
                    provider_name,
                    &format!("duplicate auth field '{}'", field.name),
                );
            }
        }

        Ok(Self {
            provider_name,
            fields,
        })
    }

    fn required_string(&self, field: &str) -> Result<String, MarretaError> {
        match self.optional_string(field)? {
            Some(value) => Ok(value),
            None => auth_config_error(
                self.provider_name,
                &format!("missing required auth field '{}'", field),
            ),
        }
    }

    fn optional_string(&self, field: &str) -> Result<Option<String>, MarretaError> {
        self.fields
            .get(field)
            .map(|expr| eval_string(self.provider_name, field, expr))
            .transpose()
    }

    fn optional_u64(&self, field: &str) -> Result<Option<u64>, MarretaError> {
        self.fields
            .get(field)
            .map(|expr| eval_u64(self.provider_name, field, expr))
            .transpose()
    }
}

fn eval_string(
    provider_name: &str,
    field: &str,
    expr: &Expression,
) -> Result<String, MarretaError> {
    let value = match expr {
        Expression::StringLiteral(value) => value.clone(),
        Expression::PropertyAccess { object, property } if matches!(object.as_ref(), Expression::Identifier(name) if name == "env") => {
            std::env::var(property).map_err(|_| MarretaError::RuntimeError {
                message: format!(
                    "auth provider '{}' field '{}' references missing environment variable '{}'",
                    provider_name, field, property
                ),
                line: 0,
                column: 0,
            })?
        }
        _ => {
            return auth_config_error(
                provider_name,
                &format!(
                    "auth field '{}' must be a string literal or env.VAR reference",
                    field
                ),
            );
        }
    };

    if value.trim().is_empty() {
        return auth_config_error(
            provider_name,
            &format!("auth field '{}' cannot be empty", field),
        );
    }

    Ok(value)
}

fn eval_u64(provider_name: &str, field: &str, expr: &Expression) -> Result<u64, MarretaError> {
    match expr {
        Expression::Integer(value) if *value >= 0 => Ok(*value as u64),
        Expression::StringLiteral(value) => {
            value
                .parse::<u64>()
                .map_err(|_| MarretaError::RuntimeError {
                    message: format!(
                        "auth provider '{}' field '{}' must be a positive integer",
                        provider_name, field
                    ),
                    line: 0,
                    column: 0,
                })
        }
        Expression::PropertyAccess { object, property } if matches!(object.as_ref(), Expression::Identifier(name) if name == "env") =>
        {
            let value = std::env::var(property).map_err(|_| MarretaError::RuntimeError {
                message: format!(
                    "auth provider '{}' field '{}' references missing environment variable '{}'",
                    provider_name, field, property
                ),
                line: 0,
                column: 0,
            })?;
            value.parse::<u64>().map_err(|_| MarretaError::RuntimeError {
                message: format!(
                    "auth provider '{}' field '{}' references environment variable '{}' that must be a positive integer",
                    provider_name, field, property
                ),
                line: 0,
                column: 0,
            })
        }
        _ => auth_config_error(
            provider_name,
            &format!(
                "auth field '{}' must be an integer literal or env.VAR reference",
                field
            ),
        ),
    }
}

fn validate_supported_algorithm(provider_name: &str, algorithm: &str) -> Result<(), MarretaError> {
    if is_hmac_algorithm(algorithm) || is_asymmetric_algorithm(algorithm) {
        return Ok(());
    }
    auth_config_error(
        provider_name,
        &format!("unsupported JWT algorithm '{}'", algorithm),
    )
}

fn validate_http_url(provider_name: &str, field: &str, value: &str) -> Result<(), MarretaError> {
    let Ok(url) = Url::parse(value) else {
        return auth_config_error(
            provider_name,
            &format!("auth field '{}' must be a valid URL", field),
        );
    };
    if !matches!(url.scheme(), "http" | "https") {
        return auth_config_error(
            provider_name,
            &format!("auth field '{}' must use http or https", field),
        );
    }
    Ok(())
}

fn validate_public_key_pem(
    provider_name: &str,
    pem: &str,
    algorithm: &str,
) -> Result<(), MarretaError> {
    let result = if matches!(algorithm, "RS256" | "RS384" | "RS512") {
        DecodingKey::from_rsa_pem(pem.as_bytes())
    } else {
        DecodingKey::from_ec_pem(pem.as_bytes())
    };
    result.map(|_| ()).map_err(|_| MarretaError::RuntimeError {
        message: format!(
            "invalid auth provider '{}': public_key_pem must be a valid PEM public key for {}",
            provider_name, algorithm
        ),
        line: 0,
        column: 0,
    })
}

fn read_public_key_pem_file(provider_name: &str, path: &str) -> Result<String, MarretaError> {
    if let Ok(metadata) = std::fs::metadata(path)
        && metadata.len() > MAX_PUBLIC_KEY_PEM_FILE_BYTES
    {
        return auth_config_error(
            provider_name,
            &format!(
                "public_key_pem_file '{}' is too large; expected a PEM file up to {} bytes",
                path, MAX_PUBLIC_KEY_PEM_FILE_BYTES
            ),
        );
    }

    std::fs::read_to_string(path).map_err(|err| MarretaError::RuntimeError {
        message: format!(
            "invalid auth provider '{}': failed to read public_key_pem_file '{}': {}",
            provider_name, path, err
        ),
        line: 0,
        column: 0,
    })
}

fn validate_api_key_header(provider_name: &str, header: &str) -> Result<(), MarretaError> {
    HeaderName::from_bytes(header.as_bytes())
        .map(|_| ())
        .map_err(|_| MarretaError::RuntimeError {
            message: format!(
                "invalid auth provider '{}': api_key header must be a valid HTTP header name",
                provider_name
            ),
            line: 0,
            column: 0,
        })
}

fn validate_secret_hash(provider_name: &str, hash: &str) -> Result<(), MarretaError> {
    if hash.starts_with("$argon2id$") {
        return PasswordHash::new(hash).map(|_| ()).map_err(|_| MarretaError::RuntimeError {
            message: format!(
                "invalid auth provider '{}': api_key secret_hash must be a valid Argon2id PHC string",
                provider_name
            ),
            line: 0,
            column: 0,
        });
    }

    if let Some(hex) = hash
        .strip_prefix("sha256:")
        .or_else(|| hash.strip_prefix("SHA256:"))
        && hex.len() == 64
        && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Ok(());
    }

    auth_config_error(
        provider_name,
        "api_key secret_hash must be either an Argon2id PHC string or sha256:<64 hex chars>",
    )
}

fn is_hmac_algorithm(algorithm: &str) -> bool {
    matches!(algorithm, "HS256" | "HS384" | "HS512")
}

fn is_asymmetric_algorithm(algorithm: &str) -> bool {
    matches!(algorithm, "RS256" | "RS384" | "RS512" | "ES256" | "ES384")
}

fn auth_config_error<T>(provider_name: &str, message: &str) -> Result<T, MarretaError> {
    Err(MarretaError::RuntimeError {
        message: format!("invalid auth provider '{}': {}", provider_name, message),
        line: 0,
        column: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{AuthProviderConfig, AuthProviderField};

    fn jwt_provider(fields: Vec<(&str, Expression)>) -> AuthProvider {
        AuthProvider::Jwt(AuthProviderConfig {
            name: "customer_auth".into(),
            fields: fields
                .into_iter()
                .map(|(name, value)| AuthProviderField {
                    name: name.into(),
                    value,
                    line: 1,
                    column: 1,
                })
                .collect(),
        })
    }

    fn api_key_provider(fields: Vec<(&str, Expression)>) -> AuthProvider {
        AuthProvider::ApiKey(AuthProviderConfig {
            name: "internal_auth".into(),
            fields: fields
                .into_iter()
                .map(|(name, value)| AuthProviderField {
                    name: name.into(),
                    value,
                    line: 1,
                    column: 1,
                })
                .collect(),
        })
    }

    fn string(value: &str) -> Expression {
        Expression::StringLiteral(value.into())
    }

    #[test]
    fn builds_jwt_provider_with_public_key_pem_file() {
        let mut providers = HashMap::new();
        providers.insert(
            "customer_auth".into(),
            jwt_provider(vec![
                ("issuer", string("https://issuer.example.test")),
                ("audience", string("shop-api")),
                ("algorithm", string("RS256")),
                (
                    "public_key_pem_file",
                    string("tests/fixtures/auth/rsa_public_key.pem"),
                ),
            ]),
        );

        let registry = build_auth_registry(&providers).unwrap();
        let AuthProviderRuntimeConfig::Jwt(jwt) = &registry.providers["customer_auth"] else {
            panic!("expected jwt provider");
        };
        let JwtValidationSource::PublicKeyPem(pem) = &jwt.validation_source else {
            panic!("expected public key pem source");
        };
        assert!(pem.contains("BEGIN PUBLIC KEY"));
    }

    #[test]
    fn rejects_jwt_with_missing_public_key_pem_file() {
        let mut providers = HashMap::new();
        providers.insert(
            "customer_auth".into(),
            jwt_provider(vec![
                ("issuer", string("https://issuer.example.test")),
                ("audience", string("shop-api")),
                ("algorithm", string("RS256")),
                (
                    "public_key_pem_file",
                    string("tests/fixtures/auth/missing-public-key.pem"),
                ),
            ]),
        );

        let err = build_auth_registry(&providers).unwrap_err();
        let message = format!("{}", err);
        assert!(message.contains("failed to read public_key_pem_file"));
        assert!(message.contains("missing-public-key.pem"));
    }

    #[test]
    fn rejects_jwt_with_invalid_public_key_pem_file() {
        let mut providers = HashMap::new();
        providers.insert(
            "customer_auth".into(),
            jwt_provider(vec![
                ("issuer", string("https://issuer.example.test")),
                ("audience", string("shop-api")),
                ("algorithm", string("RS256")),
                ("public_key_pem_file", string("Cargo.toml")),
            ]),
        );

        let err = build_auth_registry(&providers).unwrap_err();
        assert!(format!("{}", err).contains("public_key_pem"));
    }

    #[test]
    fn rejects_jwt_with_public_key_pem_and_file() {
        let mut providers = HashMap::new();
        providers.insert(
            "customer_auth".into(),
            jwt_provider(vec![
                ("issuer", string("https://issuer.example.test")),
                ("audience", string("shop-api")),
                ("algorithm", string("RS256")),
                ("public_key_pem", string("inline pem")),
                (
                    "public_key_pem_file",
                    string("tests/fixtures/auth/rsa_public_key.pem"),
                ),
            ]),
        );

        let err = build_auth_registry(&providers).unwrap_err();
        assert!(
            format!("{}", err)
                .contains("public_key_pem and public_key_pem_file are mutually exclusive")
        );
    }

    #[test]
    fn rejects_jwt_with_public_key_pem_file_and_hmac_algorithm() {
        let mut providers = HashMap::new();
        providers.insert(
            "customer_auth".into(),
            jwt_provider(vec![
                ("issuer", string("https://issuer.example.test")),
                ("audience", string("shop-api")),
                ("algorithm", string("HS256")),
                (
                    "public_key_pem_file",
                    string("tests/fixtures/auth/rsa_public_key.pem"),
                ),
            ]),
        );

        let err = build_auth_registry(&providers).unwrap_err();
        assert!(format!("{}", err).contains("valid only with asymmetric algorithms"));
    }

    #[test]
    fn builds_minimal_jwt_provider_with_oidc_defaults() {
        let mut providers = HashMap::new();
        providers.insert(
            "customer_auth".into(),
            jwt_provider(vec![
                ("issuer", string("https://issuer.example.test")),
                ("audience", string("shop-api")),
            ]),
        );

        let registry = build_auth_registry(&providers).unwrap();
        let AuthProviderRuntimeConfig::Jwt(jwt) = &registry.providers["customer_auth"] else {
            panic!("expected jwt provider");
        };
        assert_eq!(jwt.subject_claim, "sub");
        assert_eq!(jwt.user_id_claim, "sub");
        assert_eq!(jwt.roles_claim, "roles");
        assert_eq!(jwt.validation_source, JwtValidationSource::OidcDiscovery);
    }

    #[test]
    fn rejects_jwt_with_multiple_validation_sources() {
        let mut providers = HashMap::new();
        providers.insert(
            "customer_auth".into(),
            jwt_provider(vec![
                ("issuer", string("https://issuer.example.test")),
                ("audience", string("shop-api")),
                ("jwks_url", string("https://issuer.example.test/jwks")),
                ("secret", string("secret")),
            ]),
        );

        let err = build_auth_registry(&providers).unwrap_err();
        assert!(format!("{}", err).contains("mutually exclusive"));
    }

    #[test]
    fn rejects_hmac_secret_with_asymmetric_algorithm() {
        let mut providers = HashMap::new();
        providers.insert(
            "customer_auth".into(),
            jwt_provider(vec![
                ("issuer", string("https://issuer.example.test")),
                ("audience", string("shop-api")),
                ("algorithm", string("RS256")),
                ("secret", string("secret")),
            ]),
        );

        let err = build_auth_registry(&providers).unwrap_err();
        assert!(format!("{}", err).contains("HMAC algorithms"));
    }

    #[test]
    fn builds_api_key_provider_with_secret_hash() {
        let mut providers = HashMap::new();
        providers.insert(
            "internal_auth".into(),
            api_key_provider(vec![
                ("header", string("x-api-key")),
                (
                    "secret_hash",
                    string(
                        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    ),
                ),
            ]),
        );

        let registry = build_auth_registry(&providers).unwrap();
        let AuthProviderRuntimeConfig::ApiKey(config) = &registry.providers["internal_auth"] else {
            panic!("expected api_key provider");
        };
        assert_eq!(config.header, "x-api-key");
        assert_eq!(config.principal, "internal_auth");
    }

    #[test]
    fn rejects_api_key_without_secret_source() {
        let mut providers = HashMap::new();
        providers.insert(
            "internal_auth".into(),
            api_key_provider(vec![("header", string("x-api-key"))]),
        );

        let err = build_auth_registry(&providers).unwrap_err();
        assert!(format!("{}", err).contains("requires one secret source"));
    }

    #[test]
    fn rejects_api_key_with_invalid_header_name() {
        let mut providers = HashMap::new();
        providers.insert(
            "internal_auth".into(),
            api_key_provider(vec![
                ("header", string("x api key")),
                (
                    "secret_hash",
                    string(
                        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    ),
                ),
            ]),
        );

        let err = build_auth_registry(&providers).unwrap_err();
        assert!(format!("{}", err).contains("valid HTTP header name"));
    }

    #[test]
    fn rejects_api_key_with_invalid_secret_hash_format() {
        let mut providers = HashMap::new();
        providers.insert(
            "internal_auth".into(),
            api_key_provider(vec![
                ("header", string("x-api-key")),
                ("secret_hash", string("not-a-supported-hash")),
            ]),
        );

        let err = build_auth_registry(&providers).unwrap_err();
        assert!(format!("{}", err).contains("secret_hash must be either"));
    }

    #[test]
    fn rejects_jwt_with_invalid_jwks_url() {
        let mut providers = HashMap::new();
        providers.insert(
            "customer_auth".into(),
            jwt_provider(vec![
                ("issuer", string("https://issuer.example.test")),
                ("audience", string("shop-api")),
                ("jwks_url", string("not a url")),
            ]),
        );

        let err = build_auth_registry(&providers).unwrap_err();
        assert!(format!("{}", err).contains("jwks_url"));
    }

    #[test]
    fn rejects_jwt_with_invalid_public_key_pem() {
        let mut providers = HashMap::new();
        providers.insert(
            "customer_auth".into(),
            jwt_provider(vec![
                ("issuer", string("https://issuer.example.test")),
                ("audience", string("shop-api")),
                ("algorithm", string("RS256")),
                ("public_key_pem", string("not a pem")),
            ]),
        );

        let err = build_auth_registry(&providers).unwrap_err();
        assert!(format!("{}", err).contains("public_key_pem"));
    }
}
