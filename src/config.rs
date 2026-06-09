use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use crate::feature_flags::{FEATURE_ENV_PREFIX, FeatureFlags, load_feature_flags};

#[derive(Debug, Clone, PartialEq)]
pub struct DbRuntimeConfig {
    pub provider: String,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub name: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub ssl_mode: Option<String>,
    pub pool_max_connections: Option<u32>,
    pub pool_min_connections: Option<u32>,
    pub pool_acquire_timeout_secs: Option<u64>,
    pub pool_idle_timeout_secs: Option<u64>,
    pub pool_max_lifetime_secs: Option<u64>,
    pub pool_test_before_acquire: Option<bool>,
}

impl DbRuntimeConfig {
    pub fn provider_name(&self) -> &str {
        self.provider.as_str()
    }

    pub fn connection_url(&self) -> Result<String, String> {
        match self.provider.to_lowercase().as_str() {
            "postgres" | "postgresql" => {
                let host = require_value(&self.host, "MARRETA_DB_HOST", "postgres")?;
                let port = self.port.unwrap_or(5432);
                let name = require_value(&self.name, "MARRETA_DB_NAME", "postgres")?;
                let user = require_value(&self.user, "MARRETA_DB_USER", "postgres")?;

                let mut url = format!(
                    "postgres://{}{}@{}:{}/{}",
                    percent_encode_component(&user),
                    self.password
                        .as_ref()
                        .map(|v| format!(":{}", percent_encode_component(v)))
                        .unwrap_or_default(),
                    host,
                    port,
                    percent_encode_path_segment(&name),
                );

                if let Some(ssl_mode) = self.ssl_mode.as_deref() {
                    url.push_str("?sslmode=");
                    url.push_str(&percent_encode_component(ssl_mode));
                }

                Ok(url)
            }
            other => Err(format!("unsupported structured db provider '{}'", other)),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DocRuntimeConfig {
    pub provider: String,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub name: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub auth_source: Option<String>,
    pub pool_max_connections: Option<u32>,
    pub pool_min_connections: Option<u32>,
    pub pool_connect_timeout_ms: Option<u64>,
    pub pool_server_selection_timeout_ms: Option<u64>,
}

impl DocRuntimeConfig {
    pub fn provider_name(&self) -> &str {
        self.provider.as_str()
    }

    pub fn connection_url(&self) -> Result<String, String> {
        match self.provider.to_lowercase().as_str() {
            "mongodb" => {
                let host = require_value(&self.host, "MARRETA_DOC_HOST", "mongodb")?;
                let port = self.port.unwrap_or(27017);
                let name = require_value(&self.name, "MARRETA_DOC_NAME", "mongodb")?;

                let auth = match (&self.user, &self.password) {
                    (Some(user), Some(password)) => format!(
                        "{}:{}@",
                        percent_encode_component(user),
                        percent_encode_component(password)
                    ),
                    (Some(user), None) => {
                        format!("{}@", percent_encode_component(user))
                    }
                    (None, Some(_)) => {
                        return Err(
                            "incomplete structured doc config for provider mongodb: missing MARRETA_DOC_USER"
                                .to_string(),
                        )
                    }
                    (None, None) => String::new(),
                };

                let mut url = format!(
                    "mongodb://{}{}:{}/{}",
                    auth,
                    host,
                    port,
                    percent_encode_path_segment(&name)
                );

                if let Some(auth_source) = self.auth_source.as_deref() {
                    url.push_str("?authSource=");
                    url.push_str(&percent_encode_component(auth_source));
                }

                Ok(url)
            }
            other => Err(format!("unsupported structured doc provider '{}'", other)),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CacheRuntimeConfig {
    pub provider: String,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub db: Option<u32>,
    pub prefix: String,
    pub default_ttl: Option<Duration>,
    pub pool_size: u32,
    pub connect_timeout: Duration,
    pub operation_timeout: Duration,
    pub reconnect_max_retries: u32,
}

impl CacheRuntimeConfig {
    pub fn provider_name(&self) -> &str {
        self.provider.as_str()
    }

    pub fn connection_url(&self) -> Result<String, String> {
        match self.provider.to_lowercase().as_str() {
            "redis" => {
                let host = require_value(&self.host, "MARRETA_CACHE_HOST", "redis")?;
                let port = self.port.unwrap_or(6379);
                let auth = match (&self.user, &self.password) {
                    (Some(user), Some(password)) => format!(
                        "{}:{}@",
                        percent_encode_component(user),
                        percent_encode_component(password)
                    ),
                    (Some(user), None) => format!("{}@", percent_encode_component(user)),
                    (None, Some(password)) => format!(":{}@", percent_encode_component(password)),
                    (None, None) => String::new(),
                };
                let db_suffix = self.db.map(|v| format!("/{}", v)).unwrap_or_default();

                Ok(format!("redis://{}{}:{}{}", auth, host, port, db_suffix))
            }
            other => Err(format!("unsupported structured cache provider '{}'", other)),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct QueueRuntimeConfig {
    pub provider: String,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub vhost: Option<String>,
    pub topic_exchange: String,
    pub prefetch_count: u16,
    pub reconnect_max_retries: u32,
}

impl QueueRuntimeConfig {
    pub fn provider_name(&self) -> &str {
        self.provider.as_str()
    }

    pub fn connection_url(&self) -> Result<String, String> {
        match self.provider.to_lowercase().as_str() {
            "rabbitmq" => {
                let host = require_value(&self.host, "MARRETA_QUEUE_HOST", "rabbitmq")?;
                let port = self.port.unwrap_or(5672);
                let user = require_value(&self.user, "MARRETA_QUEUE_USER", "rabbitmq")?;
                let auth = format!(
                    "{}{}@",
                    percent_encode_component(&user),
                    self.password
                        .as_ref()
                        .map(|v| format!(":{}", percent_encode_component(v)))
                        .unwrap_or_default(),
                );
                let vhost = self.vhost.as_deref().unwrap_or("/");
                let encoded_vhost = if vhost == "/" {
                    "%2F".to_string()
                } else {
                    percent_encode_path_segment(vhost.trim_start_matches('/'))
                };
                Ok(format!(
                    "amqp://{}{}:{}/{}",
                    auth, host, port, encoded_vhost
                ))
            }
            other => Err(format!("unsupported structured queue provider '{}'", other)),
        }
    }
}

/// Runtime configuration for the MarretaLang server.
///
/// Priority (highest → lowest):
/// 1. CLI flags (`--port`)
/// 2. process environment variables
/// 3. `marreta.env` file in the project root/current directory
/// 4. built-in defaults where allowed by provider semantics
#[derive(Debug, Clone, PartialEq)]
pub struct MarretaConfig {
    pub host: String,
    pub port: u16,
    pub cors_enabled: bool,
    pub cors_origin: String,
    pub docs_enabled: bool,
    pub docs_path: String,
    pub db: Option<DbRuntimeConfig>,
    pub doc: Option<DocRuntimeConfig>,
    pub cache: Option<CacheRuntimeConfig>,
    pub queue: Option<QueueRuntimeConfig>,
    pub feature_flags: FeatureFlags,
    pub config_errors: Vec<String>,
}

impl MarretaConfig {
    pub fn load() -> Self {
        Self::load_from_env_file(Path::new("marreta.env"))
    }

    pub fn load_from_project_root(project_root: &Path) -> Self {
        Self::load_from_env_file(&project_root.join("marreta.env"))
    }

    pub fn project_env_vars(project_root: &Path) -> HashMap<String, String> {
        read_env_file_path(&project_root.join("marreta.env"))
    }

    fn load_from_env_file(env_file: &Path) -> Self {
        let file_vars = read_env_file_path(env_file);
        let mut merged_vars = file_vars.clone();
        merged_vars.extend(std::env::vars());
        let get = |key: &str| {
            std::env::var(key)
                .ok()
                .or_else(|| file_vars.get(key).cloned())
        };
        let mut config_errors = Vec::new();
        let (feature_flags, feature_errors) = load_feature_flags(&merged_vars);
        config_errors.extend(feature_errors);

        Self {
            host: get("MARRETA_HOST").unwrap_or_else(|| "0.0.0.0".to_string()),
            port: parse_u16(get("MARRETA_PORT"), "MARRETA_PORT", &mut config_errors)
                .unwrap_or(8080),
            cors_enabled: parse_bool(get("MARRETA_CORS")).unwrap_or(true),
            cors_origin: get("MARRETA_CORS_ORIGIN").unwrap_or_else(|| "*".to_string()),
            docs_enabled: parse_bool(get("MARRETA_DOCS_ENABLED")).unwrap_or(true),
            docs_path: get("MARRETA_DOCS_PATH").unwrap_or_else(|| "/docs".to_string()),
            db: load_db_config(&get, &mut config_errors),
            doc: load_doc_config(&get, &mut config_errors),
            cache: load_cache_config(&get, &mut config_errors),
            queue: load_queue_config(&get, &mut config_errors),
            feature_flags,
            config_errors,
        }
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    pub fn first_config_error(&self) -> Option<&str> {
        self.config_errors.first().map(String::as_str)
    }

    pub fn first_feature_flag_config_error(&self) -> Option<&str> {
        self.config_errors
            .iter()
            .find(|message| message.contains(FEATURE_ENV_PREFIX))
            .map(String::as_str)
    }

    pub fn feature_flag_config_errors(&self) -> Vec<String> {
        self.config_errors
            .iter()
            .filter(|message| message.contains(FEATURE_ENV_PREFIX))
            .cloned()
            .collect()
    }
}

fn load_db_config(
    get: &dyn Fn(&str) -> Option<String>,
    config_errors: &mut Vec<String>,
) -> Option<DbRuntimeConfig> {
    let provider = get("MARRETA_DB_PROVIDER")?;
    Some(DbRuntimeConfig {
        provider,
        host: get("MARRETA_DB_HOST"),
        port: parse_u16(get("MARRETA_DB_PORT"), "MARRETA_DB_PORT", config_errors),
        name: get("MARRETA_DB_NAME"),
        user: get("MARRETA_DB_USER"),
        password: get("MARRETA_DB_PASSWORD"),
        ssl_mode: get("MARRETA_DB_SSL_MODE"),
        pool_max_connections: parse_u32(
            get("MARRETA_DB_POOL_MAX_CONNECTIONS"),
            "MARRETA_DB_POOL_MAX_CONNECTIONS",
            config_errors,
        ),
        pool_min_connections: parse_u32(
            get("MARRETA_DB_POOL_MIN_CONNECTIONS"),
            "MARRETA_DB_POOL_MIN_CONNECTIONS",
            config_errors,
        ),
        pool_acquire_timeout_secs: parse_u64(
            get("MARRETA_DB_POOL_ACQUIRE_TIMEOUT_SECS"),
            "MARRETA_DB_POOL_ACQUIRE_TIMEOUT_SECS",
            config_errors,
        ),
        pool_idle_timeout_secs: parse_u64(
            get("MARRETA_DB_POOL_IDLE_TIMEOUT_SECS"),
            "MARRETA_DB_POOL_IDLE_TIMEOUT_SECS",
            config_errors,
        ),
        pool_max_lifetime_secs: parse_u64(
            get("MARRETA_DB_POOL_MAX_LIFETIME_SECS"),
            "MARRETA_DB_POOL_MAX_LIFETIME_SECS",
            config_errors,
        ),
        pool_test_before_acquire: parse_bool(get("MARRETA_DB_POOL_TEST_BEFORE_ACQUIRE")),
    })
}

fn load_doc_config(
    get: &dyn Fn(&str) -> Option<String>,
    config_errors: &mut Vec<String>,
) -> Option<DocRuntimeConfig> {
    let provider = get("MARRETA_DOC_PROVIDER")?;
    Some(DocRuntimeConfig {
        provider,
        host: get("MARRETA_DOC_HOST"),
        port: parse_u16(get("MARRETA_DOC_PORT"), "MARRETA_DOC_PORT", config_errors),
        name: get("MARRETA_DOC_NAME"),
        user: get("MARRETA_DOC_USER"),
        password: get("MARRETA_DOC_PASSWORD"),
        auth_source: get("MARRETA_DOC_AUTH_SOURCE"),
        pool_max_connections: parse_u32(
            get("MARRETA_DOC_POOL_MAX_CONNECTIONS"),
            "MARRETA_DOC_POOL_MAX_CONNECTIONS",
            config_errors,
        ),
        pool_min_connections: parse_u32(
            get("MARRETA_DOC_POOL_MIN_CONNECTIONS"),
            "MARRETA_DOC_POOL_MIN_CONNECTIONS",
            config_errors,
        ),
        pool_connect_timeout_ms: parse_u64(
            get("MARRETA_DOC_POOL_CONNECT_TIMEOUT_MS"),
            "MARRETA_DOC_POOL_CONNECT_TIMEOUT_MS",
            config_errors,
        ),
        pool_server_selection_timeout_ms: parse_u64(
            get("MARRETA_DOC_POOL_SERVER_SELECTION_TIMEOUT_MS"),
            "MARRETA_DOC_POOL_SERVER_SELECTION_TIMEOUT_MS",
            config_errors,
        ),
    })
}

fn load_cache_config(
    get: &dyn Fn(&str) -> Option<String>,
    config_errors: &mut Vec<String>,
) -> Option<CacheRuntimeConfig> {
    let provider = get("MARRETA_CACHE_PROVIDER")?;
    Some(CacheRuntimeConfig {
        provider,
        host: get("MARRETA_CACHE_HOST"),
        port: parse_u16(
            get("MARRETA_CACHE_PORT"),
            "MARRETA_CACHE_PORT",
            config_errors,
        ),
        user: get("MARRETA_CACHE_USER"),
        password: get("MARRETA_CACHE_PASSWORD"),
        db: parse_u32(get("MARRETA_CACHE_DB"), "MARRETA_CACHE_DB", config_errors),
        prefix: get("MARRETA_CACHE_PREFIX").unwrap_or_default(),
        default_ttl: parse_u64(
            get("MARRETA_CACHE_DEFAULT_TTL"),
            "MARRETA_CACHE_DEFAULT_TTL",
            config_errors,
        )
        .map(Duration::from_secs),
        pool_size: parse_u32(
            get("MARRETA_CACHE_POOL_SIZE"),
            "MARRETA_CACHE_POOL_SIZE",
            config_errors,
        )
        .unwrap_or(10),
        connect_timeout: Duration::from_millis(
            parse_u64(
                get("MARRETA_CACHE_CONNECT_TIMEOUT_MS"),
                "MARRETA_CACHE_CONNECT_TIMEOUT_MS",
                config_errors,
            )
            .unwrap_or(2000),
        ),
        operation_timeout: Duration::from_millis(
            parse_u64(
                get("MARRETA_CACHE_OPERATION_TIMEOUT_MS"),
                "MARRETA_CACHE_OPERATION_TIMEOUT_MS",
                config_errors,
            )
            .unwrap_or(1000),
        ),
        reconnect_max_retries: parse_u32(
            get("MARRETA_CACHE_RECONNECT_MAX_RETRIES"),
            "MARRETA_CACHE_RECONNECT_MAX_RETRIES",
            config_errors,
        )
        .unwrap_or(10),
    })
}

fn load_queue_config(
    get: &dyn Fn(&str) -> Option<String>,
    config_errors: &mut Vec<String>,
) -> Option<QueueRuntimeConfig> {
    let provider = get("MARRETA_QUEUE_PROVIDER")?;
    Some(QueueRuntimeConfig {
        provider,
        host: get("MARRETA_QUEUE_HOST"),
        port: parse_u16(
            get("MARRETA_QUEUE_PORT"),
            "MARRETA_QUEUE_PORT",
            config_errors,
        ),
        user: get("MARRETA_QUEUE_USER"),
        password: get("MARRETA_QUEUE_PASSWORD"),
        vhost: get("MARRETA_QUEUE_VHOST"),
        topic_exchange: get("MARRETA_TOPIC_EXCHANGE")
            .unwrap_or_else(|| "marreta.topics".to_string()),
        prefetch_count: parse_u16(
            get("MARRETA_QUEUE_PREFETCH"),
            "MARRETA_QUEUE_PREFETCH",
            config_errors,
        )
        .unwrap_or(10),
        reconnect_max_retries: parse_u32(
            get("MARRETA_QUEUE_RECONNECT_MAX_RETRIES"),
            "MARRETA_QUEUE_RECONNECT_MAX_RETRIES",
            config_errors,
        )
        .unwrap_or(10),
    })
}

fn parse_bool(value: Option<String>) -> Option<bool> {
    value.map(|v| v.to_lowercase() != "false" && v != "0")
}

fn parse_u16(value: Option<String>, env_key: &str, config_errors: &mut Vec<String>) -> Option<u16> {
    match value {
        Some(v) => match v.parse() {
            Ok(parsed) => Some(parsed),
            Err(_) => {
                config_errors.push(format!(
                    "invalid {}: expected integer, got '{}'",
                    env_key, v
                ));
                None
            }
        },
        None => None,
    }
}

fn parse_u32(value: Option<String>, env_key: &str, config_errors: &mut Vec<String>) -> Option<u32> {
    match value {
        Some(v) => match v.parse() {
            Ok(parsed) => Some(parsed),
            Err(_) => {
                config_errors.push(format!(
                    "invalid {}: expected integer, got '{}'",
                    env_key, v
                ));
                None
            }
        },
        None => None,
    }
}

fn parse_u64(value: Option<String>, env_key: &str, config_errors: &mut Vec<String>) -> Option<u64> {
    match value {
        Some(v) => match v.parse() {
            Ok(parsed) => Some(parsed),
            Err(_) => {
                config_errors.push(format!(
                    "invalid {}: expected integer, got '{}'",
                    env_key, v
                ));
                None
            }
        },
        None => None,
    }
}

fn require_value(value: &Option<String>, env_key: &str, provider: &str) -> Result<String, String> {
    value.clone().ok_or_else(|| {
        format!(
            "incomplete structured config for provider {}: missing {}",
            provider, env_key
        )
    })
}

fn percent_encode_component(input: &str) -> String {
    percent_encode(input, false)
}

fn percent_encode_path_segment(input: &str) -> String {
    percent_encode(input, true)
}

fn percent_encode(input: &str, allow_slash: bool) -> String {
    let mut out = String::new();
    for &byte in input.as_bytes() {
        let is_unreserved = matches!(
            byte,
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~'
        );
        if is_unreserved || (allow_slash && byte == b'/') {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push_str(&format!("{:02X}", byte));
        }
    }
    out
}

#[cfg(test)]
fn read_env_file(path: &str) -> HashMap<String, String> {
    read_env_file_path(Path::new(path))
}

fn read_env_file_path(path: &Path) -> HashMap<String, String> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };

    let mut map = HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, val)) = line.split_once('=') {
            let key = key.trim().to_string();
            let val = val.split('#').next().unwrap_or("").trim();
            let val = val
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .or_else(|| val.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
                .unwrap_or(val);
            map.insert(key, val.to_string());
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn set_env(key: &str, val: &str) {
        // SAFETY: all env access in these tests is serialized through ENV_LOCK,
        // so no other thread reads or writes the environment concurrently.
        unsafe { std::env::set_var(key, val) }
    }

    fn remove_env(key: &str) {
        // SAFETY: see `set_env` — env access is serialized through ENV_LOCK.
        unsafe { std::env::remove_var(key) }
    }

    fn clear_all_marreta_vars() {
        for key in [
            "MARRETA_HOST",
            "MARRETA_PORT",
            "MARRETA_CORS",
            "MARRETA_CORS_ORIGIN",
            "MARRETA_DOCS_ENABLED",
            "MARRETA_DOCS_PATH",
            "MARRETA_DB_PROVIDER",
            "MARRETA_DB_HOST",
            "MARRETA_DB_PORT",
            "MARRETA_DB_NAME",
            "MARRETA_DB_USER",
            "MARRETA_DB_PASSWORD",
            "MARRETA_DB_SSL_MODE",
            "MARRETA_DB_POOL_MAX_CONNECTIONS",
            "MARRETA_DB_POOL_MIN_CONNECTIONS",
            "MARRETA_DB_POOL_ACQUIRE_TIMEOUT_SECS",
            "MARRETA_DB_POOL_IDLE_TIMEOUT_SECS",
            "MARRETA_DB_POOL_MAX_LIFETIME_SECS",
            "MARRETA_DB_POOL_TEST_BEFORE_ACQUIRE",
            "MARRETA_DOC_PROVIDER",
            "MARRETA_DOC_HOST",
            "MARRETA_DOC_PORT",
            "MARRETA_DOC_NAME",
            "MARRETA_DOC_USER",
            "MARRETA_DOC_PASSWORD",
            "MARRETA_DOC_AUTH_SOURCE",
            "MARRETA_DOC_POOL_MAX_CONNECTIONS",
            "MARRETA_DOC_POOL_MIN_CONNECTIONS",
            "MARRETA_DOC_POOL_CONNECT_TIMEOUT_MS",
            "MARRETA_DOC_POOL_SERVER_SELECTION_TIMEOUT_MS",
            "MARRETA_CACHE_PROVIDER",
            "MARRETA_CACHE_HOST",
            "MARRETA_CACHE_PORT",
            "MARRETA_CACHE_USER",
            "MARRETA_CACHE_PASSWORD",
            "MARRETA_CACHE_DB",
            "MARRETA_CACHE_PREFIX",
            "MARRETA_CACHE_DEFAULT_TTL",
            "MARRETA_CACHE_POOL_SIZE",
            "MARRETA_CACHE_CONNECT_TIMEOUT_MS",
            "MARRETA_CACHE_OPERATION_TIMEOUT_MS",
            "MARRETA_CACHE_RECONNECT_MAX_RETRIES",
            "MARRETA_QUEUE_PROVIDER",
            "MARRETA_QUEUE_HOST",
            "MARRETA_QUEUE_PORT",
            "MARRETA_QUEUE_USER",
            "MARRETA_QUEUE_PASSWORD",
            "MARRETA_QUEUE_VHOST",
            "MARRETA_QUEUE_PREFETCH",
            "MARRETA_QUEUE_RECONNECT_MAX_RETRIES",
            "MARRETA_TOPIC_EXCHANGE",
            "MARRETA_FEATURE_INVENTORY_API",
            "MARRETA_FEATURE_LOW_STOCK_ALERT",
            "MARRETA_FEATURE_BAD__NAME",
            "MARRETA_FEATURE_EMPTY",
        ] {
            remove_env(key);
        }
    }

    #[test]
    fn test_read_env_file_parses_key_value() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.env");
        std::fs::write(&path, "MARRETA_HOST=127.0.0.1\nMARRETA_PORT=9090\n").unwrap();
        let vars = read_env_file(path.to_str().unwrap());
        assert_eq!(vars.get("MARRETA_HOST").unwrap(), "127.0.0.1");
        assert_eq!(vars.get("MARRETA_PORT").unwrap(), "9090");
    }

    #[test]
    fn test_read_env_file_strips_inline_comments_and_quotes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.env");
        std::fs::write(&path, "MARRETA_HOST=\"localhost\" # comment\n").unwrap();
        let vars = read_env_file(path.to_str().unwrap());
        assert_eq!(vars.get("MARRETA_HOST").unwrap(), "localhost");
    }

    #[test]
    fn test_load_defaults_when_no_env_vars() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear_all_marreta_vars();
        let cfg = MarretaConfig::load();
        assert_eq!(cfg.host, "0.0.0.0");
        assert_eq!(cfg.port, 8080);
        assert!(cfg.db.is_none());
        assert!(cfg.doc.is_none());
        assert!(cfg.cache.is_none());
        assert!(cfg.queue.is_none());
    }

    #[test]
    fn test_load_structured_db_config_from_env() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear_all_marreta_vars();
        set_env("MARRETA_DB_PROVIDER", "postgres");
        set_env("MARRETA_DB_HOST", "localhost");
        set_env("MARRETA_DB_PORT", "5432");
        set_env("MARRETA_DB_NAME", "app");
        set_env("MARRETA_DB_USER", "marreta");
        set_env("MARRETA_DB_PASSWORD", "secret");
        let cfg = MarretaConfig::load();
        clear_all_marreta_vars();

        let db = cfg.db.expect("db config");
        assert_eq!(db.provider, "postgres");
        assert_eq!(db.host.as_deref(), Some("localhost"));
        assert_eq!(db.port, Some(5432));
        assert_eq!(db.name.as_deref(), Some("app"));
        assert_eq!(
            db.connection_url().unwrap(),
            "postgres://marreta:secret@localhost:5432/app"
        );
    }

    #[test]
    fn test_load_structured_doc_config_from_project_root() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear_all_marreta_vars();

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("marreta.env"),
            "MARRETA_DOC_PROVIDER=mongodb\nMARRETA_DOC_HOST=project-doc\nMARRETA_DOC_PORT=27017\nMARRETA_DOC_NAME=app\nMARRETA_DOC_USER=marreta\nMARRETA_DOC_PASSWORD=secret\n",
        )
        .unwrap();

        let cfg = MarretaConfig::load_from_project_root(dir.path());
        let doc = cfg.doc.expect("doc config");
        assert_eq!(doc.provider, "mongodb");
        assert_eq!(
            doc.connection_url().unwrap(),
            "mongodb://marreta:secret@project-doc:27017/app"
        );
    }

    #[test]
    fn test_process_env_overrides_project_file() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear_all_marreta_vars();

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("marreta.env"),
            "MARRETA_DB_PROVIDER=postgres\nMARRETA_DB_HOST=project-db\nMARRETA_DB_PORT=5432\nMARRETA_DB_NAME=project\nMARRETA_DB_USER=marreta\n",
        )
        .unwrap();

        set_env("MARRETA_DB_HOST", "override-db");
        let cfg = MarretaConfig::load_from_project_root(dir.path());
        clear_all_marreta_vars();

        let db = cfg.db.expect("db config");
        assert_eq!(db.host.as_deref(), Some("override-db"));
    }

    #[test]
    fn test_feature_flags_load_from_project_env_and_process_override() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear_all_marreta_vars();

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("marreta.env"),
            "MARRETA_FEATURE_INVENTORY_API=false\nMARRETA_FEATURE_LOW_STOCK_ALERT=yes\n",
        )
        .unwrap();

        set_env("MARRETA_FEATURE_INVENTORY_API", "true");
        let cfg = MarretaConfig::load_from_project_root(dir.path());
        clear_all_marreta_vars();

        assert!(cfg.feature_flags.enabled("inventory_api"));
        assert!(cfg.feature_flags.enabled("low_stock_alert"));
        assert!(!cfg.feature_flags.enabled("missing"));
        assert!(cfg.config_errors.is_empty());
    }

    #[test]
    fn test_feature_flag_invalid_values_are_config_errors() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear_all_marreta_vars();

        set_env("MARRETA_FEATURE_EMPTY", "");
        let cfg = MarretaConfig::load();
        clear_all_marreta_vars();

        assert_eq!(
            cfg.first_feature_flag_config_error(),
            Some("MARRETA_FEATURE_EMPTY has invalid boolean value ''")
        );
    }

    #[test]
    fn test_feature_flag_invalid_names_are_config_errors() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear_all_marreta_vars();

        set_env("MARRETA_FEATURE_BAD__NAME", "true");
        let cfg = MarretaConfig::load();
        clear_all_marreta_vars();

        assert!(
            cfg.first_feature_flag_config_error()
                .is_some_and(|err| err.contains("MARRETA_FEATURE_BAD__NAME"))
        );
    }

    #[test]
    fn test_structured_cache_defaults() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear_all_marreta_vars();
        set_env("MARRETA_CACHE_PROVIDER", "redis");
        set_env("MARRETA_CACHE_HOST", "localhost");
        let cfg = MarretaConfig::load();
        clear_all_marreta_vars();

        let cache = cfg.cache.expect("cache config");
        assert_eq!(cache.port, None);
        assert_eq!(cache.pool_size, 10);
        assert_eq!(cache.connection_url().unwrap(), "redis://localhost:6379");
    }

    #[test]
    fn test_structured_cache_acl_user_is_supported() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear_all_marreta_vars();
        set_env("MARRETA_CACHE_PROVIDER", "redis");
        set_env("MARRETA_CACHE_HOST", "localhost");
        set_env("MARRETA_CACHE_USER", "app");
        set_env("MARRETA_CACHE_PASSWORD", "secret");
        let cfg = MarretaConfig::load();
        clear_all_marreta_vars();

        let cache = cfg.cache.expect("cache config");
        assert_eq!(
            cache.connection_url().unwrap(),
            "redis://app:secret@localhost:6379"
        );
    }

    #[test]
    fn test_structured_queue_defaults() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear_all_marreta_vars();
        set_env("MARRETA_QUEUE_PROVIDER", "rabbitmq");
        set_env("MARRETA_QUEUE_HOST", "localhost");
        set_env("MARRETA_QUEUE_USER", "guest");
        set_env("MARRETA_QUEUE_PASSWORD", "guest");
        let cfg = MarretaConfig::load();
        clear_all_marreta_vars();

        let queue = cfg.queue.expect("queue config");
        assert_eq!(
            queue.connection_url().unwrap(),
            "amqp://guest:guest@localhost:5672/%2F"
        );
    }

    #[test]
    fn test_incomplete_doc_config_errors_clearly() {
        let cfg = DocRuntimeConfig {
            provider: "mongodb".to_string(),
            host: Some("localhost".to_string()),
            port: Some(27017),
            name: Some("app".to_string()),
            user: None,
            password: Some("secret".to_string()),
            auth_source: None,
            pool_max_connections: None,
            pool_min_connections: None,
            pool_connect_timeout_ms: None,
            pool_server_selection_timeout_ms: None,
        };

        assert_eq!(
            cfg.connection_url().unwrap_err(),
            "incomplete structured doc config for provider mongodb: missing MARRETA_DOC_USER"
        );
    }

    #[test]
    fn test_structured_doc_auth_source_is_supported() {
        let cfg = DocRuntimeConfig {
            provider: "mongodb".to_string(),
            host: Some("localhost".to_string()),
            port: Some(27017),
            name: Some("app".to_string()),
            user: Some("marreta".to_string()),
            password: Some("secret".to_string()),
            auth_source: Some("admin".to_string()),
            pool_max_connections: None,
            pool_min_connections: None,
            pool_connect_timeout_ms: None,
            pool_server_selection_timeout_ms: None,
        };

        assert_eq!(
            cfg.connection_url().unwrap(),
            "mongodb://marreta:secret@localhost:27017/app?authSource=admin"
        );
    }

    #[test]
    fn test_invalid_structured_port_is_reported() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear_all_marreta_vars();
        set_env("MARRETA_DB_PROVIDER", "postgres");
        set_env("MARRETA_DB_HOST", "localhost");
        set_env("MARRETA_DB_PORT", "abc");
        set_env("MARRETA_DB_NAME", "app");
        set_env("MARRETA_DB_USER", "marreta");
        let cfg = MarretaConfig::load();
        clear_all_marreta_vars();

        assert_eq!(
            cfg.first_config_error(),
            Some("invalid MARRETA_DB_PORT: expected integer, got 'abc'")
        );
    }

    #[test]
    fn test_invalid_queue_prefetch_is_reported() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear_all_marreta_vars();
        set_env("MARRETA_QUEUE_PROVIDER", "rabbitmq");
        set_env("MARRETA_QUEUE_HOST", "localhost");
        set_env("MARRETA_QUEUE_USER", "guest");
        set_env("MARRETA_QUEUE_PASSWORD", "guest");
        set_env("MARRETA_QUEUE_PREFETCH", "ten");
        let cfg = MarretaConfig::load();
        clear_all_marreta_vars();

        assert_eq!(
            cfg.first_config_error(),
            Some("invalid MARRETA_QUEUE_PREFETCH: expected integer, got 'ten'")
        );
    }
}
