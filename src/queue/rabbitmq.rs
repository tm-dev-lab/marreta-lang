use std::sync::Arc;

use async_trait::async_trait;
use futures_util::stream::{BoxStream, StreamExt};
use lapin::{
    Channel, Connection, ConnectionProperties, ExchangeKind,
    options::{
        BasicAckOptions, BasicConsumeOptions, BasicNackOptions, BasicPublishOptions,
        ExchangeDeclareOptions, QueueBindOptions, QueueDeclareOptions,
    },
    protocol::basic::AMQPProperties,
    types::{AMQPValue, FieldTable},
};
use tokio_stream::wrappers::ReceiverStream;

use crate::queue::driver::{
    QueueDelivery, QueueDriver, QueueDriverError, QueueMessage, QueueResult,
    topic_contains_wildcard,
};
use crate::value::Value;

// ─── Pool config ─────────────────────────────────────────────────────────────

/// Connection and QoS configuration for the RabbitMQ driver.
#[derive(Debug, Clone)]
pub struct RabbitMqConfig {
    pub url: String,
    /// Name of the single durable topic exchange used for all pub/sub.
    /// Configurable via `MARRETA_TOPIC_EXCHANGE` (default: `marreta.topics`).
    pub topic_exchange: String,
    /// Max unacked messages per consumer channel (0 = unlimited, not recommended).
    pub prefetch_count: u16,
    /// Max reconnect attempts before giving up.
    pub reconnect_max_retries: u32,
}

impl RabbitMqConfig {}

// ─── Driver ──────────────────────────────────────────────────────────────────

/// RabbitMQ driver.
///
/// Design: **no shared producer channel**. Every publish creates a fresh channel,
/// publishes, and explicitly closes it. This eliminates a class of channel-state
/// corruption bugs caused by sharing a lapin `Channel` across concurrent async
/// tasks — in lapin 4, channel state is not safely composable across tasks when
/// the ConfirmationFuture from `basic_publish` is dropped without awaiting.
///
/// The overhead of creating a channel per publish is ~1 AMQP round-trip. For a
/// REST API this is dominated by the HTTP handler itself and is negligible.
///
/// Consumers each own their own long-lived channel (created in `consume_queue` /
/// `consume_topic` and moved into the consumer task). Ack/nack is delivery-owned:
/// the `QueueDelivery` carries a oneshot back to the consumer loop, which then
/// issues `basic_ack` / `basic_nack` on its own channel.
pub struct RabbitMqDriver {
    connection: Arc<Connection>,
    config: RabbitMqConfig,
}

impl RabbitMqDriver {
    /// Connect to RabbitMQ and declare the shared topic exchange.
    pub async fn connect(config: RabbitMqConfig) -> Result<Self, QueueDriverError> {
        let conn = Connection::connect(&config.url, ConnectionProperties::default())
            .await
            .map_err(|e| QueueDriverError::ConnectionFailed(e.to_string()))?;

        // Declare the shared topic exchange on a temporary channel, then close it.
        let admin_ch = conn
            .create_channel()
            .await
            .map_err(|e| QueueDriverError::ConnectionFailed(e.to_string()))?;
        admin_ch
            .exchange_declare(
                config.topic_exchange.as_str().into(),
                ExchangeKind::Topic,
                ExchangeDeclareOptions {
                    durable: true,
                    auto_delete: false,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await
            .map_err(|e| QueueDriverError::ConnectionFailed(e.to_string()))?;
        // Explicit close so the channel.close handshake completes before we drop.
        let _ = admin_ch.close(200, "setup complete".into()).await;

        Ok(Self {
            connection: Arc::new(conn),
            config,
        })
    }

    /// Create a fresh publisher channel (no QoS, no confirms).
    async fn publisher_channel(&self) -> QueueResult<Channel> {
        self.connection
            .create_channel()
            .await
            .map_err(|e| QueueDriverError::PublishFailed(e.to_string()))
    }

    /// Create a fresh consumer channel with QoS applied.
    async fn consumer_channel(&self) -> QueueResult<Channel> {
        let ch = self
            .connection
            .create_channel()
            .await
            .map_err(|e| QueueDriverError::ConnectionFailed(e.to_string()))?;
        ch.basic_qos(
            self.config.prefetch_count,
            lapin::options::BasicQosOptions { global: false },
        )
        .await
        .map_err(|e| QueueDriverError::ConnectionFailed(e.to_string()))?;
        Ok(ch)
    }
}

fn message_properties(message: &QueueMessage) -> AMQPProperties {
    let mut properties = AMQPProperties::default().with_content_type("application/json".into());
    if !message.metadata.is_empty() {
        let mut headers = FieldTable::default();
        for (key, value) in &message.metadata {
            headers.insert(
                key.as_str().into(),
                AMQPValue::LongString(value.as_str().into()),
            );
        }
        properties = properties.with_headers(headers);
    }
    properties
}

fn delivery_metadata(properties: &AMQPProperties) -> std::collections::HashMap<String, String> {
    let mut metadata = std::collections::HashMap::new();
    let Some(headers) = properties.headers().as_ref() else {
        return metadata;
    };

    for (key, value) in headers {
        let value = match value {
            AMQPValue::ShortString(value) => value.to_string(),
            AMQPValue::LongString(value) => value.to_string(),
            AMQPValue::ByteArray(value) => match std::str::from_utf8(value.as_slice()) {
                Ok(value) => value.to_string(),
                Err(_) => continue,
            },
            _ => continue,
        };
        metadata.insert(key.to_string(), value);
    }
    metadata
}

#[async_trait]
impl QueueDriver for RabbitMqDriver {
    async fn declare_queue(&self, name: &str) -> QueueResult<()> {
        // Ephemeral admin channel — explicitly closed after use.
        let ch = self
            .connection
            .create_channel()
            .await
            .map_err(|e| QueueDriverError::DeclarationFailed(e.to_string()))?;
        let declare_result = ch
            .queue_declare(
                name.into(),
                QueueDeclareOptions {
                    durable: true,
                    exclusive: false,
                    auto_delete: false,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await;
        let _ = ch.close(200, "declared".into()).await;
        declare_result.map_err(|e| QueueDriverError::DeclarationFailed(e.to_string()))?;
        Ok(())
    }

    async fn bind_topic(&self, topic: &str) -> QueueResult<String> {
        if topic_contains_wildcard(topic) {
            return Err(QueueDriverError::DeclarationFailed(format!(
                "topic '{}' must be exact; '*' and '#' wildcards are not allowed",
                topic
            )));
        }
        // No-op: topics are exact strings, and the real subscribe work (declare
        // exclusive queue + bind + basic_consume) happens inside `consume_topic`
        // on the same channel that will receive deliveries.
        Ok(topic.to_string())
    }

    async fn push(&self, queue: &str, message: &QueueMessage) -> QueueResult<()> {
        let body = serde_json::to_vec(&crate::value::value_to_json(&message.payload))
            .map_err(|e| QueueDriverError::SerializationError(e.to_string()))?;

        self.declare_queue(queue).await?;

        // Fresh channel per publish — no shared state to corrupt.
        let ch = self.publisher_channel().await?;
        let publish_result = ch
            .basic_publish(
                "".into(),    // default exchange for point-to-point
                queue.into(), // routing key = queue name
                BasicPublishOptions::default(),
                &body,
                message_properties(message),
            )
            .await;
        // Close the channel explicitly regardless of publish outcome, so the
        // channel.close handshake completes and the channel isn't leaked.
        let _ = ch.close(200, "publish complete".into()).await;

        // The returned PublisherConfirm is intentionally dropped without being
        // awaited — publisher confirms are not enabled, so awaiting it would
        // block forever (or corrupt state in some lapin versions).
        let _confirm =
            publish_result.map_err(|e| QueueDriverError::PublishFailed(e.to_string()))?;
        Ok(())
    }

    async fn publish(&self, topic: &str, message: &QueueMessage) -> QueueResult<()> {
        let body = serde_json::to_vec(&crate::value::value_to_json(&message.payload))
            .map_err(|e| QueueDriverError::SerializationError(e.to_string()))?;

        let ch = self.publisher_channel().await?;
        let publish_result = ch
            .basic_publish(
                self.config.topic_exchange.as_str().into(), // shared topic exchange
                topic.into(),                               // topic string as routing key
                BasicPublishOptions::default(),
                &body,
                message_properties(message),
            )
            .await;
        let _ = ch.close(200, "publish complete".into()).await;

        let _confirm =
            publish_result.map_err(|e| QueueDriverError::PublishFailed(e.to_string()))?;
        Ok(())
    }

    async fn consume_queue(&self, queue: &str) -> QueueResult<BoxStream<'static, QueueDelivery>> {
        let ch = self.consumer_channel().await?;
        let consumer = ch
            .basic_consume(
                queue.into(),
                "".into(),
                BasicConsumeOptions::default(),
                FieldTable::default(),
            )
            .await
            .map_err(|e| QueueDriverError::ConsumeFailed(e.to_string()))?;

        let (tx, rx) = tokio::sync::mpsc::channel(32);
        tokio::spawn(async move {
            let mut consumer = consumer;
            while let Some(delivery_result) = consumer.next().await {
                match delivery_result {
                    Ok(delivery) => {
                        let payload = serde_json::from_slice::<serde_json::Value>(&delivery.data)
                            .map(|v| crate::value::json_to_value(&v))
                            .unwrap_or(Value::Null);
                        let metadata = delivery_metadata(&delivery.properties);

                        // Each delivery gets its own oneshot channel so the handler
                        // can send the ack decision back to this loop, which then
                        // performs basic_ack / basic_nack on the correct consumer channel.
                        let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
                        let d = QueueDelivery {
                            tag: delivery.delivery_tag,
                            payload,
                            metadata,
                            routing_key: delivery.routing_key.to_string(),
                            exchange: delivery.exchange.to_string(),
                            ack_tx: std::sync::Arc::new(std::sync::Mutex::new(Some(ack_tx))),
                        };

                        if tx.send(d).await.is_err() {
                            // Receiver dropped — nack and stop
                            let _ = ch
                                .basic_nack(
                                    delivery.delivery_tag,
                                    BasicNackOptions {
                                        requeue: true,
                                        ..Default::default()
                                    },
                                )
                                .await;
                            break;
                        }

                        // Wait for the handler to call delivery.ack() or delivery.nack()
                        match ack_rx.await {
                            Ok(None) => {
                                let _ = ch
                                    .basic_ack(delivery.delivery_tag, BasicAckOptions::default())
                                    .await;
                            }
                            Ok(Some(requeue)) => {
                                let _ = ch
                                    .basic_nack(
                                        delivery.delivery_tag,
                                        BasicNackOptions {
                                            requeue,
                                            ..Default::default()
                                        },
                                    )
                                    .await;
                            }
                            Err(_) => {
                                // Handler dropped delivery without ack — nack no requeue
                                let _ = ch
                                    .basic_nack(delivery.delivery_tag, BasicNackOptions::default())
                                    .await;
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            let _ = ch.close(200, "consumer stopped".into()).await;
        });

        Ok(Box::pin(ReceiverStream::new(rx)))
    }

    async fn consume_topic(&self, topic: &str) -> QueueResult<BoxStream<'static, QueueDelivery>> {
        if topic_contains_wildcard(topic) {
            return Err(QueueDriverError::ConsumeFailed(format!(
                "topic '{}' must be exact; '*' and '#' wildcards are not allowed",
                topic
            )));
        }
        // Declare the exclusive auto-delete queue AND subscribe, all on the same
        // consumer channel. The exclusive queue lives exactly as long as the
        // consumer channel, which is the canonical AMQP pub/sub pattern.
        let ch = self.consumer_channel().await?;

        let queue = ch
            .queue_declare(
                "".into(), // server-generated name
                QueueDeclareOptions {
                    durable: false,
                    exclusive: true,
                    auto_delete: true,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await
            .map_err(|e| QueueDriverError::ConsumeFailed(e.to_string()))?;

        let queue_name = queue.name().to_string();

        ch.queue_bind(
            queue_name.as_str().into(),
            self.config.topic_exchange.as_str().into(),
            topic.into(),
            QueueBindOptions::default(),
            FieldTable::default(),
        )
        .await
        .map_err(|e| QueueDriverError::ConsumeFailed(e.to_string()))?;

        let consumer = ch
            .basic_consume(
                queue_name.as_str().into(),
                "".into(),
                BasicConsumeOptions::default(),
                FieldTable::default(),
            )
            .await
            .map_err(|e| QueueDriverError::ConsumeFailed(e.to_string()))?;

        let (tx, rx) = tokio::sync::mpsc::channel(32);
        tokio::spawn(async move {
            let mut consumer = consumer;
            while let Some(delivery_result) = consumer.next().await {
                match delivery_result {
                    Ok(delivery) => {
                        let payload = serde_json::from_slice::<serde_json::Value>(&delivery.data)
                            .map(|v| crate::value::json_to_value(&v))
                            .unwrap_or(Value::Null);
                        let metadata = delivery_metadata(&delivery.properties);

                        let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
                        let d = QueueDelivery {
                            tag: delivery.delivery_tag,
                            payload,
                            metadata,
                            routing_key: delivery.routing_key.to_string(),
                            exchange: delivery.exchange.to_string(),
                            ack_tx: std::sync::Arc::new(std::sync::Mutex::new(Some(ack_tx))),
                        };

                        if tx.send(d).await.is_err() {
                            let _ = ch
                                .basic_nack(
                                    delivery.delivery_tag,
                                    BasicNackOptions {
                                        requeue: true,
                                        ..Default::default()
                                    },
                                )
                                .await;
                            break;
                        }

                        match ack_rx.await {
                            Ok(None) => {
                                let _ = ch
                                    .basic_ack(delivery.delivery_tag, BasicAckOptions::default())
                                    .await;
                            }
                            Ok(Some(requeue)) => {
                                let _ = ch
                                    .basic_nack(
                                        delivery.delivery_tag,
                                        BasicNackOptions {
                                            requeue,
                                            ..Default::default()
                                        },
                                    )
                                    .await;
                            }
                            Err(_) => {
                                let _ = ch
                                    .basic_nack(delivery.delivery_tag, BasicNackOptions::default())
                                    .await;
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            let _ = ch.close(200, "consumer stopped".into()).await;
        });

        Ok(Box::pin(ReceiverStream::new(rx)))
    }

    async fn ack(&self, _tag: u64) -> QueueResult<()> {
        // Ack is handled per-delivery via QueueDelivery::ack() in consumers.
        Ok(())
    }

    async fn nack(&self, _tag: u64, _requeue: bool) -> QueueResult<()> {
        // Nack is handled per-delivery via QueueDelivery::nack() in consumers.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delivery_metadata_accepts_utf8_byte_array_headers() {
        let traceparent = "00-11111111111111111111111111111111-2222222222222222-01";
        let mut headers = FieldTable::default();
        headers.insert(
            "traceparent".into(),
            AMQPValue::ByteArray(traceparent.as_bytes().into()),
        );

        let metadata = delivery_metadata(&AMQPProperties::default().with_headers(headers));

        assert_eq!(
            metadata.get("traceparent").map(String::as_str),
            Some(traceparent)
        );
    }

    #[test]
    fn delivery_metadata_drops_non_utf8_byte_array_headers() {
        let mut headers = FieldTable::default();
        headers.insert(
            "traceparent".into(),
            AMQPValue::ByteArray(vec![0xff, 0xfe].into()),
        );

        let metadata = delivery_metadata(&AMQPProperties::default().with_headers(headers));

        assert!(!metadata.contains_key("traceparent"));
    }
}
