//! Redis implementation of CacheDriver via the `redis` crate.

use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;
use redis::AsyncCommands;

use super::CacheConfig;
use super::driver::{CacheDriver, CacheDriverError, CacheResult};
use crate::value::Value;

/// Redis-backed cache driver with transparent key prefixing and operation timeout.
pub struct RedisDriver {
    pool: redis::aio::ConnectionManager,
    prefix: String,
    operation_timeout: Duration,
}

impl RedisDriver {
    pub async fn connect(config: &CacheConfig) -> CacheResult<Self> {
        let client = redis::Client::open(config.url.as_str())
            .map_err(|e| CacheDriverError::ConnectionFailed(e.to_string()))?;

        let pool = tokio::time::timeout(config.connect_timeout, client.get_connection_manager())
            .await
            .map_err(|_| CacheDriverError::ConnectionFailed("connect timeout".into()))?
            .map_err(|e| CacheDriverError::ConnectionFailed(e.to_string()))?;

        Ok(Self {
            pool,
            prefix: config.prefix.clone(),
            operation_timeout: config.operation_timeout,
        })
    }

    fn prefixed(&self, key: &str) -> String {
        if self.prefix.is_empty() {
            key.to_string()
        } else {
            format!("{}{}", self.prefix, key)
        }
    }

    /// Run a future with the configured operation timeout.
    async fn with_timeout<T, F>(&self, fut: F) -> CacheResult<T>
    where
        F: std::future::Future<Output = CacheResult<T>>,
    {
        tokio::time::timeout(self.operation_timeout, fut)
            .await
            .map_err(|_| CacheDriverError::OperationTimeout("operation timed out".into()))?
    }

    fn value_to_bytes(value: &Value) -> CacheResult<Vec<u8>> {
        let json = crate::value::value_to_json(value);
        serde_json::to_vec(&json).map_err(|e| CacheDriverError::SerializationError(e.to_string()))
    }

    fn bytes_to_value(data: &[u8]) -> CacheResult<Value> {
        let json: serde_json::Value = serde_json::from_slice(data)
            .map_err(|e| CacheDriverError::SerializationError(e.to_string()))?;
        Ok(crate::value::json_to_value(&json))
    }
}

#[async_trait]
impl CacheDriver for RedisDriver {
    async fn get(&self, key: &str) -> CacheResult<Option<Value>> {
        let pkey = self.prefixed(key);
        let mut conn = self.pool.clone();
        self.with_timeout(async {
            let result: Option<Vec<u8>> = conn
                .get(&pkey)
                .await
                .map_err(|e| CacheDriverError::OperationFailed(e.to_string()))?;
            match result {
                Some(data) => Ok(Some(Self::bytes_to_value(&data)?)),
                None => Ok(None),
            }
        })
        .await
    }

    async fn set(
        &self,
        key: &str,
        value: &Value,
        ttl: Option<Duration>,
        only_if_absent: bool,
    ) -> CacheResult<Option<Value>> {
        let pkey = self.prefixed(key);
        let data = Self::value_to_bytes(value)?;
        let mut conn = self.pool.clone();
        self.with_timeout(async {
            if only_if_absent {
                // SET NX
                let set: bool = if let Some(t) = ttl {
                    redis::cmd("SET")
                        .arg(&pkey)
                        .arg(data.as_slice())
                        .arg("NX")
                        .arg("EX")
                        .arg(t.as_secs())
                        .query_async(&mut conn)
                        .await
                        .map_err(|e| CacheDriverError::OperationFailed(e.to_string()))?
                } else {
                    conn.set_nx(&pkey, data.as_slice())
                        .await
                        .map_err(|e| CacheDriverError::OperationFailed(e.to_string()))?
                };
                if set {
                    Ok(Some(value.clone()))
                } else {
                    Ok(None)
                }
            } else if let Some(t) = ttl {
                conn.set_ex::<_, _, ()>(&pkey, data.as_slice(), t.as_secs())
                    .await
                    .map_err(|e| CacheDriverError::OperationFailed(e.to_string()))?;
                Ok(Some(value.clone()))
            } else {
                conn.set::<_, _, ()>(&pkey, data.as_slice())
                    .await
                    .map_err(|e| CacheDriverError::OperationFailed(e.to_string()))?;
                Ok(Some(value.clone()))
            }
        })
        .await
    }

    async fn delete(&self, key: &str) -> CacheResult<bool> {
        let pkey = self.prefixed(key);
        let mut conn = self.pool.clone();
        self.with_timeout(async {
            let count: i64 = conn
                .del(&pkey)
                .await
                .map_err(|e| CacheDriverError::OperationFailed(e.to_string()))?;
            Ok(count > 0)
        })
        .await
    }

    async fn exists(&self, key: &str) -> CacheResult<bool> {
        let pkey = self.prefixed(key);
        let mut conn = self.pool.clone();
        self.with_timeout(async {
            let count: i64 = conn
                .exists(&pkey)
                .await
                .map_err(|e| CacheDriverError::OperationFailed(e.to_string()))?;
            Ok(count > 0)
        })
        .await
    }

    async fn ttl(&self, key: &str) -> CacheResult<Option<Duration>> {
        let pkey = self.prefixed(key);
        let mut conn = self.pool.clone();
        self.with_timeout(async {
            let secs: i64 = redis::cmd("TTL")
                .arg(&pkey)
                .query_async(&mut conn)
                .await
                .map_err(|e| CacheDriverError::OperationFailed(e.to_string()))?;
            if secs < 0 {
                Ok(None) // -1 = no TTL, -2 = key doesn't exist
            } else {
                Ok(Some(Duration::from_secs(secs as u64)))
            }
        })
        .await
    }

    async fn expire(&self, key: &str, ttl: Duration) -> CacheResult<bool> {
        let pkey = self.prefixed(key);
        let mut conn = self.pool.clone();
        self.with_timeout(async {
            let set: bool = conn
                .expire(&pkey, ttl.as_secs() as i64)
                .await
                .map_err(|e| CacheDriverError::OperationFailed(e.to_string()))?;
            Ok(set)
        })
        .await
    }

    async fn incr(&self, key: &str, by: i64, ttl: Option<Duration>) -> CacheResult<i64> {
        let pkey = self.prefixed(key);
        let mut conn = self.pool.clone();
        self.with_timeout(async {
            let new_val: i64 = conn
                .incr(&pkey, by)
                .await
                .map_err(|e| CacheDriverError::OperationFailed(e.to_string()))?;
            if let Some(t) = ttl {
                // Only set TTL if this was the first increment (value == by means key was new)
                // or always — simpler and consistent
                let _: () = conn
                    .expire(&pkey, t.as_secs() as i64)
                    .await
                    .map_err(|e| CacheDriverError::OperationFailed(e.to_string()))?;
            }
            Ok(new_val)
        })
        .await
    }

    async fn decr(&self, key: &str, by: i64, ttl: Option<Duration>) -> CacheResult<i64> {
        self.incr(key, -by, ttl).await
    }

    async fn get_many(&self, keys: &[String]) -> CacheResult<HashMap<String, Option<Value>>> {
        if keys.is_empty() {
            return Ok(HashMap::new());
        }
        let prefixed_keys: Vec<String> = keys.iter().map(|k| self.prefixed(k)).collect();
        let mut conn = self.pool.clone();
        self.with_timeout(async {
            let results: Vec<Option<Vec<u8>>> = conn
                .mget(&prefixed_keys)
                .await
                .map_err(|e| CacheDriverError::OperationFailed(e.to_string()))?;
            let mut map = HashMap::new();
            for (i, key) in keys.iter().enumerate() {
                let val = match results.get(i).and_then(|r| r.as_ref()) {
                    Some(data) => Some(Self::bytes_to_value(data)?),
                    None => None,
                };
                map.insert(key.clone(), val);
            }
            Ok(map)
        })
        .await
    }

    async fn set_many(
        &self,
        entries: &HashMap<String, Value>,
        ttl: Option<Duration>,
    ) -> CacheResult<()> {
        if entries.is_empty() {
            return Ok(());
        }
        let mut conn = self.pool.clone();
        self.with_timeout(async {
            // Build MSET pairs
            let mut pairs: Vec<(String, Vec<u8>)> = Vec::new();
            for (k, v) in entries {
                pairs.push((self.prefixed(k), Self::value_to_bytes(v)?));
            }
            let refs: Vec<(&str, &[u8])> = pairs
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_slice()))
                .collect();
            redis::cmd("MSET")
                .arg(
                    refs.iter()
                        .flat_map(|(k, v)| vec![k.as_bytes(), *v])
                        .collect::<Vec<&[u8]>>(),
                )
                .query_async::<()>(&mut conn)
                .await
                .map_err(|e| CacheDriverError::OperationFailed(e.to_string()))?;
            // Apply TTL to each key if set (MSET doesn't support TTL natively)
            if let Some(t) = ttl {
                for (pkey, _) in &pairs {
                    let _: () = conn
                        .expire(pkey, t.as_secs() as i64)
                        .await
                        .map_err(|e| CacheDriverError::OperationFailed(e.to_string()))?;
                }
            }
            Ok(())
        })
        .await
    }

    async fn ping(&self) -> CacheResult<()> {
        let mut conn = self.pool.clone();
        self.with_timeout(async {
            redis::cmd("PING")
                .query_async::<()>(&mut conn)
                .await
                .map_err(|e| CacheDriverError::ConnectionFailed(e.to_string()))?;
            Ok(())
        })
        .await
    }
}
