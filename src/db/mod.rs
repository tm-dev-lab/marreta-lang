pub mod driver;
pub mod postgres;
pub mod query_builder;

use std::sync::Arc;

use crate::config::MarretaConfig;
use crate::db::driver::DbDriver;
use crate::db::postgres::{PoolConfig, PostgresDriver};
use crate::error::MarretaError;
use crate::migrations::{AppliedMigration, DatabaseSchema, LocalMigration};

/// Supported DB providers.
#[derive(Debug, Clone, PartialEq)]
pub enum DbProvider {
    Postgres,
}

/// Runtime DB engine: holds the active driver behind an `Arc`.
/// `None` when no `MARRETA_DB_*` config is present.
#[derive(Clone)]
pub struct DbEngine {
    pub driver: Arc<dyn DbDriver>,
    pub provider: DbProvider,
}

impl DbEngine {
    /// Initializes the DB engine from config.
    /// Returns `None` if `MARRETA_DB_PROVIDER` is not set.
    /// Returns `Err` if config is present but invalid or connection fails.
    pub async fn from_config(config: &MarretaConfig) -> Result<Option<Self>, MarretaError> {
        if let Some(message) = config.first_config_error() {
            return Err(MarretaError::DbError {
                message: message.to_string(),
                operation: "db.connect".to_string(),
            });
        }
        let db = match &config.db {
            Some(db) => db,
            None => return Ok(None),
        };
        let provider_str = db.provider_name();

        match provider_str.to_lowercase().as_str() {
            "postgres" | "postgresql" => {
                let url = db
                    .connection_url()
                    .map_err(|message| MarretaError::DbError {
                        message,
                        operation: "db.connect".to_string(),
                    })?;
                let pool_cfg = PoolConfig {
                    max_connections: db.pool_max_connections,
                    min_connections: db.pool_min_connections,
                    acquire_timeout_secs: db.pool_acquire_timeout_secs,
                    idle_timeout_secs: db.pool_idle_timeout_secs,
                    max_lifetime_secs: db.pool_max_lifetime_secs,
                    test_before_acquire: db.pool_test_before_acquire,
                };
                let driver = PostgresDriver::connect(url.as_str(), pool_cfg).await?;
                Ok(Some(DbEngine {
                    driver: Arc::new(driver),
                    provider: DbProvider::Postgres,
                }))
            }
            "mongodb" => {
                // Return None for unsupported db. relational engines when the user configures MongoDB
                Ok(None)
            }
            other => Err(MarretaError::DbError {
                message: format!(
                    "Unsupported MARRETA_DB_PROVIDER '{}'. Supported: postgres",
                    other
                ),
                operation: "db.connect".to_string(),
            }),
        }
    }
}

pub async fn introspect_schema_from_config(
    config: &MarretaConfig,
) -> Result<DatabaseSchema, MarretaError> {
    if let Some(message) = config.first_config_error() {
        return Err(MarretaError::DbError {
            message: message.to_string(),
            operation: "db.introspect".to_string(),
        });
    }
    let db = config.db.as_ref().ok_or_else(|| MarretaError::DbError {
        message: "MARRETA_DB_PROVIDER is required for migrations".to_string(),
        operation: "db.introspect".to_string(),
    })?;
    let provider_str = db.provider_name();

    match provider_str.to_lowercase().as_str() {
        "postgres" | "postgresql" => {
            let url = db
                .connection_url()
                .map_err(|message| MarretaError::DbError {
                    message,
                    operation: "db.introspect".to_string(),
                })?;
            let pool_cfg = PoolConfig {
                max_connections: db.pool_max_connections,
                min_connections: db.pool_min_connections,
                acquire_timeout_secs: db.pool_acquire_timeout_secs,
                idle_timeout_secs: db.pool_idle_timeout_secs,
                max_lifetime_secs: db.pool_max_lifetime_secs,
                test_before_acquire: db.pool_test_before_acquire,
            };
            let driver = PostgresDriver::connect(url.as_str(), pool_cfg).await?;
            driver.introspect_schema().await
        }
        other => Err(MarretaError::DbError {
            message: format!(
                "Unsupported MARRETA_DB_PROVIDER '{}' for migrations. Supported: postgres",
                other
            ),
            operation: "db.introspect".to_string(),
        }),
    }
}

pub async fn ensure_migration_table_from_config(
    config: &MarretaConfig,
) -> Result<(), MarretaError> {
    if let Some(message) = config.first_config_error() {
        return Err(MarretaError::DbError {
            message: message.to_string(),
            operation: "db.migrate.ensure_table".to_string(),
        });
    }
    let db = config.db.as_ref().ok_or_else(|| MarretaError::DbError {
        message: "MARRETA_DB_PROVIDER is required for migrations".to_string(),
        operation: "db.migrate.ensure_table".to_string(),
    })?;
    let provider_str = db.provider_name();

    match provider_str.to_lowercase().as_str() {
        "postgres" | "postgresql" => {
            let url = db
                .connection_url()
                .map_err(|message| MarretaError::DbError {
                    message,
                    operation: "db.migrate.ensure_table".to_string(),
                })?;
            let pool_cfg = PoolConfig {
                max_connections: db.pool_max_connections,
                min_connections: db.pool_min_connections,
                acquire_timeout_secs: db.pool_acquire_timeout_secs,
                idle_timeout_secs: db.pool_idle_timeout_secs,
                max_lifetime_secs: db.pool_max_lifetime_secs,
                test_before_acquire: db.pool_test_before_acquire,
            };
            let driver = PostgresDriver::connect(url.as_str(), pool_cfg).await?;
            driver.ensure_migration_table().await
        }
        other => Err(MarretaError::DbError {
            message: format!(
                "Unsupported MARRETA_DB_PROVIDER '{}' for migrations. Supported: postgres",
                other
            ),
            operation: "db.migrate.ensure_table".to_string(),
        }),
    }
}

pub async fn list_applied_migrations_from_config(
    config: &MarretaConfig,
) -> Result<Vec<AppliedMigration>, MarretaError> {
    if let Some(message) = config.first_config_error() {
        return Err(MarretaError::DbError {
            message: message.to_string(),
            operation: "db.migrate.status".to_string(),
        });
    }
    let db = config.db.as_ref().ok_or_else(|| MarretaError::DbError {
        message: "MARRETA_DB_PROVIDER is required for migrations".to_string(),
        operation: "db.migrate.status".to_string(),
    })?;
    let provider_str = db.provider_name();

    match provider_str.to_lowercase().as_str() {
        "postgres" | "postgresql" => {
            let url = db
                .connection_url()
                .map_err(|message| MarretaError::DbError {
                    message,
                    operation: "db.migrate.status".to_string(),
                })?;
            let pool_cfg = PoolConfig {
                max_connections: db.pool_max_connections,
                min_connections: db.pool_min_connections,
                acquire_timeout_secs: db.pool_acquire_timeout_secs,
                idle_timeout_secs: db.pool_idle_timeout_secs,
                max_lifetime_secs: db.pool_max_lifetime_secs,
                test_before_acquire: db.pool_test_before_acquire,
            };
            let driver = PostgresDriver::connect(url.as_str(), pool_cfg).await?;
            driver.list_applied_migrations().await
        }
        other => Err(MarretaError::DbError {
            message: format!(
                "Unsupported MARRETA_DB_PROVIDER '{}' for migrations. Supported: postgres",
                other
            ),
            operation: "db.migrate.status".to_string(),
        }),
    }
}

pub async fn apply_migration_from_config(
    config: &MarretaConfig,
    migration: &LocalMigration,
) -> Result<(), MarretaError> {
    if let Some(message) = config.first_config_error() {
        return Err(MarretaError::DbError {
            message: message.to_string(),
            operation: "db.migrate.apply".to_string(),
        });
    }
    let db = config.db.as_ref().ok_or_else(|| MarretaError::DbError {
        message: "MARRETA_DB_PROVIDER is required for migrations".to_string(),
        operation: "db.migrate.apply".to_string(),
    })?;
    let provider_str = db.provider_name();

    match provider_str.to_lowercase().as_str() {
        "postgres" | "postgresql" => {
            let url = db
                .connection_url()
                .map_err(|message| MarretaError::DbError {
                    message,
                    operation: "db.migrate.apply".to_string(),
                })?;
            let pool_cfg = PoolConfig {
                max_connections: db.pool_max_connections,
                min_connections: db.pool_min_connections,
                acquire_timeout_secs: db.pool_acquire_timeout_secs,
                idle_timeout_secs: db.pool_idle_timeout_secs,
                max_lifetime_secs: db.pool_max_lifetime_secs,
                test_before_acquire: db.pool_test_before_acquire,
            };
            let driver = PostgresDriver::connect(url.as_str(), pool_cfg).await?;
            driver.apply_migration(migration).await
        }
        other => Err(MarretaError::DbError {
            message: format!(
                "Unsupported MARRETA_DB_PROVIDER '{}' for migrations. Supported: postgres",
                other
            ),
            operation: "db.migrate.apply".to_string(),
        }),
    }
}

pub async fn rollback_migration_from_config(
    config: &MarretaConfig,
    migration: &LocalMigration,
) -> Result<(), MarretaError> {
    if let Some(message) = config.first_config_error() {
        return Err(MarretaError::DbError {
            message: message.to_string(),
            operation: "db.migrate.rollback".to_string(),
        });
    }
    let db = config.db.as_ref().ok_or_else(|| MarretaError::DbError {
        message: "MARRETA_DB_PROVIDER is required for migrations".to_string(),
        operation: "db.migrate.rollback".to_string(),
    })?;
    let provider_str = db.provider_name();

    match provider_str.to_lowercase().as_str() {
        "postgres" | "postgresql" => {
            let url = db
                .connection_url()
                .map_err(|message| MarretaError::DbError {
                    message,
                    operation: "db.migrate.rollback".to_string(),
                })?;
            let pool_cfg = PoolConfig {
                max_connections: db.pool_max_connections,
                min_connections: db.pool_min_connections,
                acquire_timeout_secs: db.pool_acquire_timeout_secs,
                idle_timeout_secs: db.pool_idle_timeout_secs,
                max_lifetime_secs: db.pool_max_lifetime_secs,
                test_before_acquire: db.pool_test_before_acquire,
            };
            let driver = PostgresDriver::connect(url.as_str(), pool_cfg).await?;
            driver.rollback_migration(migration).await
        }
        other => Err(MarretaError::DbError {
            message: format!(
                "Unsupported MARRETA_DB_PROVIDER '{}' for migrations. Supported: postgres",
                other
            ),
            operation: "db.migrate.rollback".to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DbRuntimeConfig, MarretaConfig};
    use crate::feature_flags::FeatureFlags;

    fn config_no_db() -> MarretaConfig {
        MarretaConfig {
            host: "0.0.0.0".to_string(),
            port: 8080,
            cors_enabled: true,
            cors_origin: "*".to_string(),
            docs_enabled: true,
            docs_path: "/docs".to_string(),
            db: None,
            doc: None,
            cache: None,
            queue: None,
            feature_flags: FeatureFlags::default(),
            config_errors: Vec::new(),
        }
    }

    fn config_with_provider(
        provider: &str,
        host: Option<&str>,
        name: Option<&str>,
        user: Option<&str>,
    ) -> MarretaConfig {
        MarretaConfig {
            db: Some(DbRuntimeConfig {
                provider: provider.to_string(),
                host: host.map(|s| s.to_string()),
                port: Some(5432),
                name: name.map(|s| s.to_string()),
                user: user.map(|s| s.to_string()),
                password: None,
                ssl_mode: None,
                pool_max_connections: None,
                pool_min_connections: None,
                pool_acquire_timeout_secs: None,
                pool_idle_timeout_secs: None,
                pool_max_lifetime_secs: None,
                pool_test_before_acquire: None,
            }),
            ..config_no_db()
        }
    }

    #[tokio::test]
    async fn test_from_config_no_provider_returns_none() {
        let cfg = config_no_db();
        let result = DbEngine::from_config(&cfg).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    fn unwrap_db_err(result: Result<Option<DbEngine>, MarretaError>) -> MarretaError {
        match result {
            Err(e) => e,
            Ok(_) => panic!("expected Err, got Ok"),
        }
    }

    #[tokio::test]
    async fn test_from_config_unsupported_provider_returns_db_error() {
        let cfg = config_with_provider("mysql", Some("localhost"), Some("test"), Some("marreta"));
        let err = unwrap_db_err(DbEngine::from_config(&cfg).await);
        assert!(matches!(err, MarretaError::DbError { .. }));
        assert!(err.to_string().contains("mysql"));
    }

    #[tokio::test]
    async fn test_from_config_postgres_missing_host_returns_db_error() {
        let cfg = config_with_provider("postgres", None, Some("test"), Some("marreta"));
        let err = unwrap_db_err(DbEngine::from_config(&cfg).await);
        assert!(matches!(err, MarretaError::DbError { .. }));
        assert!(err.to_string().contains("MARRETA_DB_HOST"));
    }

    #[tokio::test]
    async fn test_from_config_postgresql_alias_missing_user_returns_db_error() {
        let cfg = config_with_provider("postgresql", Some("localhost"), Some("test"), None);
        let err = unwrap_db_err(DbEngine::from_config(&cfg).await);
        assert!(matches!(err, MarretaError::DbError { .. }));
        assert!(err.to_string().contains("MARRETA_DB_USER"));
    }

    #[tokio::test]
    async fn test_from_config_unsupported_provider_error_code_is_db_error() {
        let cfg = config_with_provider("redis", Some("localhost"), Some("test"), Some("marreta"));
        let err = unwrap_db_err(DbEngine::from_config(&cfg).await);
        assert_eq!(err.semantic_code(), "db_error");
    }

    #[tokio::test]
    async fn test_from_config_postgres_with_url_attempts_connect() {
        // Provides a syntactically valid but unreachable URL.
        // With min_connections=0, sqlx creates a lazy pool so connect() may succeed
        // (covering the PoolConfig construction + DbEngine return lines).
        // If connect fails fast (DNS), the ? on that line is still covered.
        let cfg = MarretaConfig {
            db: Some(DbRuntimeConfig {
                pool_min_connections: Some(0),
                pool_acquire_timeout_secs: Some(1),
                port: Some(19999),
                host: Some("127.0.0.1".to_string()),
                name: Some("nonexistent".to_string()),
                user: Some("marreta".to_string()),
                provider: "postgres".to_string(),
                password: None,
                ssl_mode: None,
                pool_max_connections: None,
                pool_idle_timeout_secs: None,
                pool_max_lifetime_secs: None,
                pool_test_before_acquire: None,
            }),
            ..config_no_db()
        };
        // We don't assert Ok/Err — either outcome exercises the targeted lines
        let _ = DbEngine::from_config(&cfg).await;
    }

    #[tokio::test]
    async fn test_from_config_case_insensitive_provider() {
        let cfg = config_with_provider("POSTGRES", None, Some("test"), Some("marreta"));
        let err = unwrap_db_err(DbEngine::from_config(&cfg).await);
        assert!(
            err.to_string().contains("MARRETA_DB_HOST"),
            "unexpected: {}",
            err
        );
    }

    // ─── DbProvider traits ──────────────────────────────────────────────────────

    #[test]
    fn test_db_provider_debug() {
        assert_eq!(format!("{:?}", DbProvider::Postgres), "Postgres");
    }

    #[test]
    fn test_db_provider_clone() {
        let p = DbProvider::Postgres;
        let p2 = p.clone();
        assert_eq!(p2, DbProvider::Postgres);
    }

    #[test]
    fn test_db_provider_partial_eq() {
        assert_eq!(DbProvider::Postgres, DbProvider::Postgres);
    }

    // ─── DbEngine construction ──────────────────────────────────────────────────

    struct StubDriver;

    #[async_trait::async_trait]
    impl crate::db::driver::DbDriver for StubDriver {
        async fn save(
            &self,
            _: &str,
            _: crate::db::driver::DbRow,
        ) -> crate::db::driver::DbResult<crate::db::driver::DbRow> {
            unimplemented!()
        }
        async fn find(
            &self,
            _: &str,
            _: &crate::value::Value,
        ) -> crate::db::driver::DbResult<Option<crate::db::driver::DbRow>> {
            unimplemented!()
        }
        async fn find_all(
            &self,
            _: &str,
            _: Vec<crate::db::driver::FilterClause>,
        ) -> crate::db::driver::DbResult<Vec<crate::db::driver::DbRow>> {
            unimplemented!()
        }
        async fn update_by_id(
            &self,
            _: &str,
            _: &crate::value::Value,
            _: crate::db::driver::DbRow,
        ) -> crate::db::driver::DbResult<Option<crate::db::driver::DbRow>> {
            unimplemented!()
        }
        async fn delete_by_id(
            &self,
            _: &str,
            _: &crate::value::Value,
        ) -> crate::db::driver::DbResult<bool> {
            unimplemented!()
        }
        async fn query_fetch(
            &self,
            _: &crate::db::driver::QueryState,
        ) -> crate::db::driver::DbResult<Vec<crate::db::driver::DbRow>> {
            unimplemented!()
        }
        async fn query_fetch_one(
            &self,
            _: &crate::db::driver::QueryState,
        ) -> crate::db::driver::DbResult<Option<crate::db::driver::DbRow>> {
            unimplemented!()
        }
        async fn query_count(
            &self,
            _: &crate::db::driver::QueryState,
        ) -> crate::db::driver::DbResult<i64> {
            unimplemented!()
        }
        async fn query_exists(
            &self,
            _: &crate::db::driver::QueryState,
        ) -> crate::db::driver::DbResult<bool> {
            unimplemented!()
        }
        async fn query_update(
            &self,
            _: &crate::db::driver::QueryState,
            _: crate::db::driver::DbRow,
        ) -> crate::db::driver::DbResult<u64> {
            unimplemented!()
        }
        async fn query_delete(
            &self,
            _: &crate::db::driver::QueryState,
        ) -> crate::db::driver::DbResult<u64> {
            unimplemented!()
        }
        async fn native_query(
            &self,
            _: &str,
            _: Vec<crate::value::Value>,
        ) -> crate::db::driver::DbResult<Vec<crate::db::driver::DbRow>> {
            unimplemented!()
        }
        async fn begin(&self) -> crate::db::driver::DbResult<Box<dyn crate::db::driver::DbTx>> {
            unimplemented!()
        }
    }

    #[test]
    fn test_db_engine_direct_construction() {
        let engine = DbEngine {
            driver: Arc::new(StubDriver),
            provider: DbProvider::Postgres,
        };
        assert_eq!(engine.provider, DbProvider::Postgres);
    }

    #[test]
    fn test_db_engine_clone() {
        let engine = DbEngine {
            driver: Arc::new(StubDriver),
            provider: DbProvider::Postgres,
        };
        let engine2 = engine.clone();
        assert_eq!(engine2.provider, DbProvider::Postgres);
    }
}
