//! HTTP client abstraction layer — HttpClient trait, reqwest impl, engine.
//!
//! Operations: `http_client.get`, `http_client.post`, `http_client.put`,
//! `http_client.patch`, `http_client.delete`.

pub mod driver;
pub mod reqwest;

use std::sync::Arc;
use std::time::Duration;

use driver::{HttpClient, HttpClientDriverError};

// --- Config ------------------------------------------------------------------

/// Configuration read from MARRETA_HTTP_* environment variables.
#[derive(Debug, Clone)]
pub struct HttpClientConfig {
    /// Global safety-net timeout. Per-request `timeout:` overrides.
    pub default_timeout: Duration,
}

impl HttpClientConfig {
    pub fn from_env() -> Self {
        let timeout_ms = std::env::var("MARRETA_HTTP_TIMEOUT_MS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(30_000);

        Self {
            default_timeout: Duration::from_millis(timeout_ms),
        }
    }
}

// --- Engine ------------------------------------------------------------------

/// Owns the HTTP client instance and exposes the driver to the interpreter.
pub struct HttpClientEngine {
    pub driver: Arc<dyn HttpClient>,
    pub config: HttpClientConfig,
}

impl HttpClientEngine {
    /// Create the HTTP client engine from environment configuration.
    pub fn from_env() -> Result<Self, HttpClientDriverError> {
        let config = HttpClientConfig::from_env();
        let driver: Arc<dyn HttpClient> = {
            let reqwest_driver = reqwest::ReqwestDriver::new(config.default_timeout)?;
            Arc::new(reqwest_driver)
        };
        Ok(Self { driver, config })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn with_http_timeout_env<T>(value: Option<&str>, f: impl FnOnce() -> T) -> T {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let _guard = ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("http client env test lock poisoned");

        let previous = std::env::var("MARRETA_HTTP_TIMEOUT_MS").ok();
        // SAFETY: env access in this test helper is serialized through ENV_LOCK;
        // the variable is set, the closure runs, then the original is restored,
        // all while holding the lock, so no other thread observes the mutation.
        unsafe {
            match value {
                Some(value) => std::env::set_var("MARRETA_HTTP_TIMEOUT_MS", value),
                None => std::env::remove_var("MARRETA_HTTP_TIMEOUT_MS"),
            }
        }

        let result = f();

        // SAFETY: restore step of the lock-guarded swap above.
        unsafe {
            match previous {
                Some(value) => std::env::set_var("MARRETA_HTTP_TIMEOUT_MS", value),
                None => std::env::remove_var("MARRETA_HTTP_TIMEOUT_MS"),
            }
        }

        result
    }

    #[test]
    fn test_config_defaults() {
        with_http_timeout_env(None, || {
            let config = HttpClientConfig::from_env();
            assert_eq!(config.default_timeout, Duration::from_millis(30_000));
        });
    }

    #[test]
    fn test_config_custom_timeout() {
        with_http_timeout_env(Some("5000"), || {
            let config = HttpClientConfig::from_env();
            assert_eq!(config.default_timeout, Duration::from_millis(5000));
        });
    }

    #[test]
    fn test_config_invalid_timeout_uses_default() {
        with_http_timeout_env(Some("not_a_number"), || {
            let config = HttpClientConfig::from_env();
            assert_eq!(config.default_timeout, Duration::from_millis(30_000));
        });
    }

    #[test]
    fn test_engine_creation() {
        with_http_timeout_env(None, || {
            let engine = HttpClientEngine::from_env();
            assert!(engine.is_ok());
        });
    }
}
