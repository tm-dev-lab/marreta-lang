use async_trait::async_trait;
use futures_util::stream::BoxStream;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

use crate::value::Value;

// ─── Ack decision ─────────────────────────────────────────────────────────────

/// Sent back to the broker consumer loop to perform the actual ack/nack on the
/// correct consumer channel. `None` = ack; `Some(requeue)` = nack.
pub type AckDecision = Option<bool>;

// ─── Message ────────────────────────────────────────────────────────────────

/// Internal broker message shape.
///
/// `payload` is the user-visible Marreta value. `metadata` is transport-layer
/// data such as W3C Trace Context and is intentionally not exposed to user code.
/// Metadata values are string-only by design; binary metadata is out of scope
/// until a concrete driver requires a richer metadata value type.
#[derive(Debug, Clone, PartialEq)]
pub struct QueueMessage {
    pub payload: Value,
    pub metadata: HashMap<String, String>,
}

impl QueueMessage {
    pub fn new(payload: Value) -> Self {
        Self {
            payload,
            metadata: HashMap::new(),
        }
    }
}

// ─── Delivery ────────────────────────────────────────────────────────────────

/// A single message delivered by the broker.
///
/// Call `delivery.ack()` or `delivery.nack(requeue)` after processing.
/// The broker implementation waits for this signal and performs the wire-level
/// ack/nack on the channel that originally received the delivery.
#[derive(Debug)]
pub struct QueueDelivery {
    /// Broker-assigned delivery tag used for ack/nack.
    pub tag: u64,
    /// Deserialized JSON payload.
    pub payload: Value,
    /// Transport-layer metadata such as W3C Trace Context.
    pub metadata: HashMap<String, String>,
    /// Routing key the message was published with.
    pub routing_key: String,
    /// Exchange the message came from (empty string for default exchange).
    pub exchange: String,
    /// Sends the ack decision back to the consumer loop. `Arc<Mutex<Option<…>>>`
    /// so `QueueDelivery` can be cloned (only the first send wins).
    pub(crate) ack_tx: Arc<Mutex<Option<oneshot::Sender<AckDecision>>>>,
}

impl QueueDelivery {
    /// Acknowledge the delivery — message removed from queue.
    pub fn ack(self) {
        if let Ok(mut guard) = self.ack_tx.lock()
            && let Some(tx) = guard.take()
        {
            let _ = tx.send(None);
        }
    }

    /// Negative-acknowledge the delivery.
    /// `requeue = true` puts the message back on the queue.
    /// `requeue = false` discards it (or routes to DLX if configured).
    pub fn nack(self, requeue: bool) {
        if let Ok(mut guard) = self.ack_tx.lock()
            && let Some(tx) = guard.take()
        {
            let _ = tx.send(Some(requeue));
        }
    }
}

impl Clone for QueueDelivery {
    fn clone(&self) -> Self {
        Self {
            tag: self.tag,
            payload: self.payload.clone(),
            metadata: self.metadata.clone(),
            routing_key: self.routing_key.clone(),
            exchange: self.exchange.clone(),
            // Cloning shares the same sender slot — only the first call to ack/nack wins
            ack_tx: Arc::clone(&self.ack_tx),
        }
    }
}

// ─── Error ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum QueueDriverError {
    ConnectionFailed(String),
    PublishFailed(String),
    ConsumeFailed(String),
    AckFailed(String),
    SerializationError(String),
    DeclarationFailed(String),
}

impl std::fmt::Display for QueueDriverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConnectionFailed(m) => write!(f, "connection failed: {}", m),
            Self::PublishFailed(m) => write!(f, "publish failed: {}", m),
            Self::ConsumeFailed(m) => write!(f, "consume failed: {}", m),
            Self::AckFailed(m) => write!(f, "ack failed: {}", m),
            Self::SerializationError(m) => write!(f, "serialization error: {}", m),
            Self::DeclarationFailed(m) => write!(f, "declaration failed: {}", m),
        }
    }
}

pub type QueueResult<T> = Result<T, QueueDriverError>;

pub fn topic_contains_wildcard(topic: &str) -> bool {
    topic.contains('*') || topic.contains('#')
}

// ─── QueueDriver trait ───────────────────────────────────────────────────────

/// Abstraction over message broker providers.
/// Implementations: RabbitMQ (v0.8). Future: Kafka, SQS.
#[async_trait]
pub trait QueueDriver: Send + Sync {
    /// Declare a durable queue (idempotent — creates if not exists).
    async fn declare_queue(&self, name: &str) -> QueueResult<()>;

    /// Prepare to consume an exact topic.
    /// The driver translates the topic to the broker's native pub/sub primitive
    /// (RabbitMQ: bind to shared topic exchange; Kafka: topic subscription; etc.).
    /// Returns an opaque handle used by `consume_topic`.
    async fn bind_topic(&self, topic: &str) -> QueueResult<String>;

    /// Publish a message to a named queue (point-to-point).
    async fn push(&self, queue: &str, message: &QueueMessage) -> QueueResult<()>;

    /// Publish a message to a topic.
    /// The topic is a dot-separated string (e.g. `"payments.approved"`).
    /// The driver translates it to the broker's native pub/sub primitive.
    async fn publish(&self, topic: &str, message: &QueueMessage) -> QueueResult<()>;

    /// Start consuming from a named queue.
    /// Returns a stream of deliveries; each must be ack'd or nack'd.
    async fn consume_queue(&self, queue: &str) -> QueueResult<BoxStream<'static, QueueDelivery>>;

    /// Start consuming from a topic exchange binding.
    /// `queue_name` is the server-generated name returned by `bind_topic`.
    async fn consume_topic(
        &self,
        queue_name: &str,
    ) -> QueueResult<BoxStream<'static, QueueDelivery>>;

    /// Acknowledge a delivery — message removed from queue.
    async fn ack(&self, tag: u64) -> QueueResult<()>;

    /// Negative-acknowledge a delivery.
    /// `requeue = true` puts the message back on the queue.
    /// `requeue = false` discards it (or routes to DLX if configured).
    async fn nack(&self, tag: u64, requeue: bool) -> QueueResult<()>;
}

// ─── Mock driver for unit tests ───────────────────────────────────────────────

#[cfg(test)]
pub mod mock {
    use super::*;
    use futures_util::stream;
    use std::sync::{Arc, Mutex};

    /// Records all push/publish/ack/nack calls for assertion in unit tests.
    #[derive(Debug, Default)]
    pub struct MockQueueDriver {
        pub pushed: Mutex<Vec<(String, QueueMessage)>>,
        /// (topic, message)
        pub published: Mutex<Vec<(String, QueueMessage)>>,
        pub acked: Mutex<Vec<u64>>,
        pub nacked: Mutex<Vec<(u64, bool)>>,
        pub declared_queues: Mutex<Vec<String>>,
        /// Pre-loaded deliveries to return from consume_queue/consume_topic.
        pub deliveries: Mutex<Vec<QueueDelivery>>,
        /// If set, push/publish return this error.
        pub fail_publish: Mutex<bool>,
    }

    impl MockQueueDriver {
        pub fn new() -> Arc<Self> {
            Arc::new(Self::default())
        }

        pub fn with_deliveries(deliveries: Vec<QueueDelivery>) -> Arc<Self> {
            let m = Self::default();
            *m.deliveries.lock().unwrap() = deliveries;
            Arc::new(m)
        }
    }

    #[async_trait]
    impl QueueDriver for MockQueueDriver {
        async fn declare_queue(&self, name: &str) -> QueueResult<()> {
            self.declared_queues.lock().unwrap().push(name.to_string());
            Ok(())
        }

        async fn bind_topic(&self, topic: &str) -> QueueResult<String> {
            if topic_contains_wildcard(topic) {
                return Err(QueueDriverError::DeclarationFailed(format!(
                    "topic '{}' must be exact; '*' and '#' wildcards are not allowed",
                    topic
                )));
            }
            Ok(format!("mock.topic.{}", topic))
        }

        async fn push(&self, queue: &str, message: &QueueMessage) -> QueueResult<()> {
            if *self.fail_publish.lock().unwrap() {
                return Err(QueueDriverError::PublishFailed("mock failure".to_string()));
            }
            self.pushed
                .lock()
                .unwrap()
                .push((queue.to_string(), message.clone()));
            Ok(())
        }

        async fn publish(&self, topic: &str, message: &QueueMessage) -> QueueResult<()> {
            if *self.fail_publish.lock().unwrap() {
                return Err(QueueDriverError::PublishFailed("mock failure".to_string()));
            }
            self.published
                .lock()
                .unwrap()
                .push((topic.to_string(), message.clone()));
            Ok(())
        }

        async fn consume_queue(
            &self,
            _queue: &str,
        ) -> QueueResult<BoxStream<'static, QueueDelivery>> {
            let deliveries = self.deliveries.lock().unwrap().clone();
            Ok(Box::pin(stream::iter(deliveries)))
        }

        async fn consume_topic(
            &self,
            _queue_name: &str,
        ) -> QueueResult<BoxStream<'static, QueueDelivery>> {
            let deliveries = self.deliveries.lock().unwrap().clone();
            Ok(Box::pin(stream::iter(deliveries)))
        }

        async fn ack(&self, tag: u64) -> QueueResult<()> {
            self.acked.lock().unwrap().push(tag);
            Ok(())
        }

        async fn nack(&self, tag: u64, requeue: bool) -> QueueResult<()> {
            self.nacked.lock().unwrap().push((tag, requeue));
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mock::MockQueueDriver;

    fn delivery(tag: u64) -> QueueDelivery {
        let (tx, _rx) = tokio::sync::oneshot::channel();
        QueueDelivery {
            tag,
            payload: Value::Null,
            metadata: HashMap::new(),
            routing_key: "test.key".to_string(),
            exchange: "test".to_string(),
            ack_tx: std::sync::Arc::new(std::sync::Mutex::new(Some(tx))),
        }
    }

    #[tokio::test]
    async fn test_mock_push_recorded() {
        let driver = MockQueueDriver::new();
        driver
            .push("orders", &QueueMessage::new(Value::Null))
            .await
            .unwrap();
        let pushed = driver.pushed.lock().unwrap();
        assert_eq!(pushed.len(), 1);
        assert_eq!(pushed[0].0, "orders");
    }

    #[tokio::test]
    async fn test_mock_publish_recorded() {
        let driver = MockQueueDriver::new();
        driver
            .publish("payments.approved", &QueueMessage::new(Value::Null))
            .await
            .unwrap();
        let published = driver.published.lock().unwrap();
        assert_eq!(published.len(), 1);
        assert_eq!(published[0].0, "payments.approved");
    }

    #[tokio::test]
    async fn test_mock_ack_recorded() {
        let driver = MockQueueDriver::new();
        driver.ack(42).await.unwrap();
        assert_eq!(*driver.acked.lock().unwrap(), vec![42u64]);
    }

    #[tokio::test]
    async fn test_mock_nack_no_requeue_recorded() {
        let driver = MockQueueDriver::new();
        driver.nack(7, false).await.unwrap();
        assert_eq!(*driver.nacked.lock().unwrap(), vec![(7u64, false)]);
    }

    #[tokio::test]
    async fn test_mock_nack_requeue_recorded() {
        let driver = MockQueueDriver::new();
        driver.nack(7, true).await.unwrap();
        assert_eq!(*driver.nacked.lock().unwrap(), vec![(7u64, true)]);
    }

    #[tokio::test]
    async fn test_mock_declare_queue_recorded() {
        let driver = MockQueueDriver::new();
        driver.declare_queue("my_queue").await.unwrap();
        assert!(
            driver
                .declared_queues
                .lock()
                .unwrap()
                .contains(&"my_queue".to_string())
        );
    }

    #[tokio::test]
    async fn test_mock_bind_topic_returns_name() {
        let driver = MockQueueDriver::new();
        let name = driver.bind_topic("payment.approved").await.unwrap();
        assert_eq!(name, "mock.topic.payment.approved");
    }

    #[tokio::test]
    async fn test_mock_bind_topic_rejects_wildcards() {
        let driver = MockQueueDriver::new();
        let err = driver.bind_topic("payment.*").await.unwrap_err();
        assert!(err.to_string().contains("wildcards are not allowed"));

        let err = driver.bind_topic("payment.#").await.unwrap_err();
        assert!(err.to_string().contains("wildcards are not allowed"));
    }

    #[tokio::test]
    async fn test_mock_consume_queue_streams_deliveries() {
        use futures_util::StreamExt;
        let driver = MockQueueDriver::with_deliveries(vec![delivery(1), delivery(2)]);
        let mut stream = driver.consume_queue("q").await.unwrap();
        assert_eq!(stream.next().await.unwrap().tag, 1);
        assert_eq!(stream.next().await.unwrap().tag, 2);
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn test_mock_fail_publish() {
        let driver = MockQueueDriver::new();
        *driver.fail_publish.lock().unwrap() = true;
        let result = driver.push("q", &QueueMessage::new(Value::Null)).await;
        assert!(matches!(result, Err(QueueDriverError::PublishFailed(_))));
    }

    #[test]
    fn test_queue_driver_error_display() {
        assert!(
            QueueDriverError::ConnectionFailed("x".into())
                .to_string()
                .contains("connection failed")
        );
        assert!(
            QueueDriverError::PublishFailed("x".into())
                .to_string()
                .contains("publish failed")
        );
        assert!(
            QueueDriverError::ConsumeFailed("x".into())
                .to_string()
                .contains("consume failed")
        );
        assert!(
            QueueDriverError::AckFailed("x".into())
                .to_string()
                .contains("ack failed")
        );
        assert!(
            QueueDriverError::SerializationError("x".into())
                .to_string()
                .contains("serialization error")
        );
        assert!(
            QueueDriverError::DeclarationFailed("x".into())
                .to_string()
                .contains("declaration failed")
        );
    }
}
