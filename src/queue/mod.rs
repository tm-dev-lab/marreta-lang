//! Queue abstraction layer — QueueDriver trait, RabbitMQ impl, engine.
//!
//! Consumers are declared with `on queue` / `on topic` in `.marreta` files.
//! The `QueueEngine` starts all consumers at server startup and routes
//! incoming deliveries through the interpreter.

pub mod driver;
pub mod rabbitmq;

use std::sync::Arc;

use crate::config::MarretaConfig;
use driver::{QueueDriver, QueueDriverError};
use rabbitmq::{RabbitMqConfig, RabbitMqDriver};

// ─── Provider ────────────────────────────────────────────────────────────────

/// Supported queue broker backends.
#[derive(Debug, Clone, Default)]
pub enum QueueProvider {
    #[default]
    RabbitMq,
}

impl std::str::FromStr for QueueProvider {
    type Err = QueueDriverError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_lowercase().as_str() {
            "rabbitmq" => Ok(Self::RabbitMq),
            other => Err(QueueDriverError::ConnectionFailed(format!(
                "unsupported MARRETA_QUEUE_PROVIDER '{}'. Supported: rabbitmq",
                other
            ))),
        }
    }
}

// ─── Engine ──────────────────────────────────────────────────────────────────

/// Owns the live broker connection and exposes it to the interpreter.
pub struct QueueEngine {
    pub driver: Arc<dyn QueueDriver>,
}

impl QueueEngine {
    /// Connect to the configured broker from the resolved Marreta config.
    pub async fn from_config(config: &MarretaConfig) -> Result<Option<Self>, QueueDriverError> {
        if let Some(message) = config.first_config_error() {
            return Err(QueueDriverError::ConnectionFailed(message.to_string()));
        }
        let queue = match &config.queue {
            Some(queue) => queue,
            None => return Ok(None),
        };
        let provider: QueueProvider = queue.provider.parse()?;
        let driver: Arc<dyn QueueDriver> = match provider {
            QueueProvider::RabbitMq => {
                let cfg = RabbitMqConfig {
                    url: queue
                        .connection_url()
                        .map_err(QueueDriverError::ConnectionFailed)?,
                    topic_exchange: queue.topic_exchange.clone(),
                    prefetch_count: queue.prefetch_count,
                    reconnect_max_retries: queue.reconnect_max_retries,
                };
                Arc::new(RabbitMqDriver::connect(cfg).await?)
            }
        };
        Ok(Some(Self { driver }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{MarretaConfig, QueueRuntimeConfig};
    use crate::feature_flags::FeatureFlags;

    fn base_config() -> MarretaConfig {
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

    #[tokio::test]
    async fn test_invalid_queue_provider_fails_fast() {
        let cfg = MarretaConfig {
            queue: Some(QueueRuntimeConfig {
                provider: "kafka".to_string(),
                host: Some("localhost".to_string()),
                port: Some(5672),
                user: Some("guest".to_string()),
                password: Some("guest".to_string()),
                vhost: None,
                topic_exchange: "marreta.topics".to_string(),
                prefetch_count: 10,
                reconnect_max_retries: 10,
            }),
            ..base_config()
        };

        let err = match QueueEngine::from_config(&cfg).await {
            Err(err) => err,
            Ok(_) => panic!("expected Err, got Ok"),
        };
        assert!(
            err.to_string()
                .contains("unsupported MARRETA_QUEUE_PROVIDER 'kafka'")
        );
    }

    #[tokio::test]
    async fn test_invalid_structured_queue_number_fails_before_connect() {
        let cfg = MarretaConfig {
            config_errors: vec![
                "invalid MARRETA_QUEUE_PREFETCH: expected integer, got 'ten'".into(),
            ],
            queue: Some(QueueRuntimeConfig {
                provider: "rabbitmq".to_string(),
                host: Some("localhost".to_string()),
                port: Some(5672),
                user: Some("guest".to_string()),
                password: Some("guest".to_string()),
                vhost: None,
                topic_exchange: "marreta.topics".to_string(),
                prefetch_count: 10,
                reconnect_max_retries: 10,
            }),
            ..base_config()
        };

        let err = match QueueEngine::from_config(&cfg).await {
            Err(err) => err,
            Ok(_) => panic!("expected Err, got Ok"),
        };
        assert!(
            err.to_string()
                .contains("invalid MARRETA_QUEUE_PREFETCH: expected integer, got 'ten'")
        );
    }
}
