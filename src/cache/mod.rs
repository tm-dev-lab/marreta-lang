//! Cache abstraction layer — CacheDriver trait, Redis impl, engine.
//!
//! Operations: `cache.get`, `cache.set`, `cache.delete`, `cache.exists`,
//! `cache.ttl`, `cache.expire`, `cache.incr`, `cache.decr`,
//! `cache.get_many`, `cache.set_many`.

pub mod driver;
pub mod redis;

use std::sync::Arc;
use std::time::Duration;

use crate::config::MarretaConfig;
use driver::{CacheDriver, CacheDriverError};

// --- Config ------------------------------------------------------------------

/// Resolved cache driver configuration used by the Redis driver.
#[derive(Debug, Clone)]
pub struct CacheConfig {
    pub url: String,
    /// Transparent key prefix (multi-tenant / multi-env isolation).
    pub prefix: String,
    /// Safety-net TTL applied when `cache.set` has no explicit `ttl:`.
    /// `None` means no implicit TTL.
    pub default_ttl: Option<Duration>,
    pub pool_size: u32,
    pub connect_timeout: Duration,
    pub operation_timeout: Duration,
    pub reconnect_max_retries: u32,
}

// --- Engine ------------------------------------------------------------------

/// Owns the live cache connection and exposes the driver to the interpreter.
pub struct CacheEngine {
    pub driver: Arc<dyn CacheDriver>,
    pub config: CacheConfig,
}

impl CacheEngine {
    /// Connect to the configured cache provider from the resolved Marreta config.
    pub async fn from_config(config: &MarretaConfig) -> Result<Option<Self>, CacheDriverError> {
        if let Some(message) = config.first_config_error() {
            return Err(CacheDriverError::OperationFailed(message.to_string()));
        }
        let cache = match &config.cache {
            Some(cache) => cache,
            None => return Ok(None),
        };
        let config = CacheConfig {
            url: cache
                .connection_url()
                .map_err(CacheDriverError::OperationFailed)?,
            prefix: cache.prefix.clone(),
            default_ttl: cache.default_ttl,
            pool_size: cache.pool_size,
            connect_timeout: cache.connect_timeout,
            operation_timeout: cache.operation_timeout,
            reconnect_max_retries: cache.reconnect_max_retries,
        };
        let driver: Arc<dyn CacheDriver> = {
            let redis_driver = redis::RedisDriver::connect(&config).await?;
            Arc::new(redis_driver)
        };
        Ok(Some(Self { driver, config }))
    }
}
