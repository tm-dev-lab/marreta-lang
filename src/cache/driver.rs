use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;

use crate::value::Value;

// --- Error -------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum CacheDriverError {
    ConnectionFailed(String),
    OperationTimeout(String),
    SerializationError(String),
    OperationFailed(String),
}

impl std::fmt::Display for CacheDriverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConnectionFailed(m) => write!(f, "connection failed: {}", m),
            Self::OperationTimeout(m) => write!(f, "operation timeout: {}", m),
            Self::SerializationError(m) => write!(f, "serialization error: {}", m),
            Self::OperationFailed(m) => write!(f, "operation failed: {}", m),
        }
    }
}

pub type CacheResult<T> = Result<T, CacheDriverError>;

// --- CacheDriver trait -------------------------------------------------------

/// Abstraction over key-value cache providers.
/// Implementations: Redis (v0.9). Future: Memcached.
#[async_trait]
pub trait CacheDriver: Send + Sync {
    /// GET — returns `None` on miss or expiry.
    async fn get(&self, key: &str) -> CacheResult<Option<Value>>;

    /// SET — stores a value with optional TTL.
    /// `only_if_absent`: when true, only sets if the key does not exist.
    /// Returns `Some(value)` on success, `None` if `only_if_absent` and key exists.
    async fn set(
        &self,
        key: &str,
        value: &Value,
        ttl: Option<Duration>,
        only_if_absent: bool,
    ) -> CacheResult<Option<Value>>;

    /// DELETE — returns true if the key existed.
    async fn delete(&self, key: &str) -> CacheResult<bool>;

    /// EXISTS — returns true if the key exists (without fetching).
    async fn exists(&self, key: &str) -> CacheResult<bool>;

    /// TTL — returns remaining TTL; `None` if no TTL or key does not exist.
    async fn ttl(&self, key: &str) -> CacheResult<Option<Duration>>;

    /// EXPIRE — refreshes TTL without rewriting the value. Returns true if key exists.
    async fn expire(&self, key: &str, ttl: Duration) -> CacheResult<bool>;

    /// INCR — atomic increment. Returns the new value.
    async fn incr(&self, key: &str, by: i64, ttl: Option<Duration>) -> CacheResult<i64>;

    /// DECR — atomic decrement. Returns the new value.
    async fn decr(&self, key: &str, by: i64, ttl: Option<Duration>) -> CacheResult<i64>;

    /// MGET — bulk get. Returns a map; misses are `None`.
    async fn get_many(&self, keys: &[String]) -> CacheResult<HashMap<String, Option<Value>>>;

    /// MSET — bulk set with shared TTL.
    async fn set_many(
        &self,
        entries: &HashMap<String, Value>,
        ttl: Option<Duration>,
    ) -> CacheResult<()>;

    /// Lightweight health check (e.g. PING).
    async fn ping(&self) -> CacheResult<()>;
}

// --- Mock driver for unit tests ----------------------------------------------

#[cfg(test)]
pub mod mock {
    use super::*;
    use std::sync::Mutex;

    #[derive(Debug, Default)]
    pub struct MockCacheDriver {
        pub store: Mutex<HashMap<String, Value>>,
        pub ttls: Mutex<HashMap<String, Duration>>,
        pub fail_next: Mutex<bool>,
    }

    impl MockCacheDriver {
        pub fn new() -> std::sync::Arc<Self> {
            std::sync::Arc::new(Self::default())
        }
    }

    #[async_trait]
    impl CacheDriver for MockCacheDriver {
        async fn get(&self, key: &str) -> CacheResult<Option<Value>> {
            if *self.fail_next.lock().unwrap() {
                return Err(CacheDriverError::ConnectionFailed("mock failure".into()));
            }
            Ok(self.store.lock().unwrap().get(key).cloned())
        }

        async fn set(
            &self,
            key: &str,
            value: &Value,
            ttl: Option<Duration>,
            only_if_absent: bool,
        ) -> CacheResult<Option<Value>> {
            if *self.fail_next.lock().unwrap() {
                return Err(CacheDriverError::ConnectionFailed("mock failure".into()));
            }
            let mut store = self.store.lock().unwrap();
            if only_if_absent && store.contains_key(key) {
                return Ok(None);
            }
            store.insert(key.to_string(), value.clone());
            if let Some(t) = ttl {
                self.ttls.lock().unwrap().insert(key.to_string(), t);
            }
            Ok(Some(value.clone()))
        }

        async fn delete(&self, key: &str) -> CacheResult<bool> {
            Ok(self.store.lock().unwrap().remove(key).is_some())
        }

        async fn exists(&self, key: &str) -> CacheResult<bool> {
            Ok(self.store.lock().unwrap().contains_key(key))
        }

        async fn ttl(&self, key: &str) -> CacheResult<Option<Duration>> {
            Ok(self.ttls.lock().unwrap().get(key).copied())
        }

        async fn expire(&self, key: &str, ttl: Duration) -> CacheResult<bool> {
            let exists = self.store.lock().unwrap().contains_key(key);
            if exists {
                self.ttls.lock().unwrap().insert(key.to_string(), ttl);
            }
            Ok(exists)
        }

        async fn incr(&self, key: &str, by: i64, ttl: Option<Duration>) -> CacheResult<i64> {
            let mut store = self.store.lock().unwrap();
            let current = match store.get(key) {
                Some(Value::Integer(n)) => *n,
                None => 0,
                _ => {
                    return Err(CacheDriverError::OperationFailed(
                        "value is not an integer".into(),
                    ));
                }
            };
            let new_val = current + by;
            store.insert(key.to_string(), Value::Integer(new_val));
            if let Some(t) = ttl {
                self.ttls.lock().unwrap().insert(key.to_string(), t);
            }
            Ok(new_val)
        }

        async fn decr(&self, key: &str, by: i64, ttl: Option<Duration>) -> CacheResult<i64> {
            self.incr(key, -by, ttl).await
        }

        async fn get_many(&self, keys: &[String]) -> CacheResult<HashMap<String, Option<Value>>> {
            let store = self.store.lock().unwrap();
            let mut result = HashMap::new();
            for k in keys {
                result.insert(k.clone(), store.get(k).cloned());
            }
            Ok(result)
        }

        async fn set_many(
            &self,
            entries: &HashMap<String, Value>,
            ttl: Option<Duration>,
        ) -> CacheResult<()> {
            let mut store = self.store.lock().unwrap();
            for (k, v) in entries {
                store.insert(k.clone(), v.clone());
                if let Some(t) = ttl {
                    self.ttls.lock().unwrap().insert(k.clone(), t);
                }
            }
            Ok(())
        }

        async fn ping(&self) -> CacheResult<()> {
            if *self.fail_next.lock().unwrap() {
                return Err(CacheDriverError::ConnectionFailed("mock failure".into()));
            }
            Ok(())
        }
    }
}
