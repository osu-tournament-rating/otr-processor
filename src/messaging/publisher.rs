use crate::messaging::config::RabbitMqConfig;
use chrono::{DateTime, Utc};
use lapin::{
    options::{BasicPublishOptions, ExchangeDeclareOptions},
    types::{AMQPValue, FieldTable, LongString, ShortString},
    BasicProperties, Channel, Connection, ConnectionProperties, ExchangeKind
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum PublisherError {
    #[error("Failed to connect to RabbitMQ: {0}")]
    ConnectionError(#[from] lapin::Error),

    #[error("Failed to serialize message: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Publisher not initialized")]
    NotInitialized
}

/// Message sent when a tournament has been processed
/// This format matches what the DWS TournamentProcessedConsumer expects
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TournamentProcessedMessage {
    pub tournament_id: i32,
    pub processed_at: DateTime<Utc>,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>
}

/// MassTransit message envelope structure
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MassTransitEnvelope<T> {
    message_id: String,
    conversation_id: String,
    correlation_id: Option<String>,
    source_address: String,
    destination_address: String,
    message_type: Vec<String>,
    message: T,
    sent_time: DateTime<Utc>
}

/// RabbitMQ publisher for sending tournament processing events
pub struct RabbitMqPublisher {
    connection: Option<Arc<Connection>>,
    channel: Option<Channel>,
    exchange: String,
    routing_key: String
}

impl RabbitMqPublisher {
    /// Creates a new RabbitMQ publisher instance
    pub fn new(exchange: String, routing_key: String) -> Self {
        Self {
            connection: None,
            channel: None,
            exchange,
            routing_key
        }
    }

    /// Creates a new RabbitMQ publisher from configuration
    pub fn from_config(config: &RabbitMqConfig) -> Self {
        Self::new(config.exchange.clone(), config.routing_key.clone())
    }

    /// Creates and connects a publisher from configuration
    pub async fn connect_from_config(config: &RabbitMqConfig) -> Result<Self, PublisherError> {
        let mut publisher = Self::from_config(config);
        publisher.connect(&config.connection_url()).await?;
        Ok(publisher)
    }

    /// Connects to RabbitMQ and initializes the publisher
    pub async fn connect(&mut self, rabbitmq_url: &str) -> Result<(), PublisherError> {
        let connection = Connection::connect(rabbitmq_url, ConnectionProperties::default()).await?;
        let connection = Arc::new(connection);

        let channel = connection.create_channel().await?;

        // Declare the exchange (fanout type for broadcasting)
        channel
            .exchange_declare(
                &self.exchange,
                ExchangeKind::Fanout,
                ExchangeDeclareOptions {
                    durable: true,
                    ..Default::default()
                },
                FieldTable::default()
            )
            .await?;

        self.connection = Some(connection);
        self.channel = Some(channel);

        log::info!("Connected to RabbitMQ at {}", rabbitmq_url);
        log::info!(
            "Exchange '{}' declared with routing key '{}'",
            self.exchange,
            self.routing_key
        );

        Ok(())
    }

    /// Publishes a tournament processed message
    pub async fn publish_tournament_processed(
        &self,
        tournament_id: i32,
        action: &str,
        correlation_id: Option<String>
    ) -> Result<(), PublisherError> {
        let channel = self.channel.as_ref().ok_or(PublisherError::NotInitialized)?;

        let message_id = Uuid::new_v4().to_string();
        let conversation_id = Uuid::new_v4().to_string();

        let message = TournamentProcessedMessage {
            tournament_id,
            processed_at: Utc::now(),
            action: action.to_string(),
            correlation_id: correlation_id.clone()
        };

        // Wrap in MassTransit envelope
        let envelope = MassTransitEnvelope {
            message_id: message_id.clone(),
            conversation_id: conversation_id.clone(),
            correlation_id: correlation_id.clone(),
            source_address: format!("rabbitmq://localhost/{}", self.exchange),
            destination_address: format!("rabbitmq://localhost/{}", self.routing_key),
            message_type: vec!["urn:message:DWS.Messages:TournamentProcessedMessage".to_string()],
            message,
            sent_time: Utc::now()
        };

        let payload = serde_json::to_vec(&envelope)?;

        // Create headers for MassTransit
        let mut headers = BTreeMap::new();
        headers.insert(
            ShortString::from("Content-Type"),
            AMQPValue::LongString(LongString::from("application/vnd.masstransit+json"))
        );

        channel
            .basic_publish(
                &self.exchange,
                &self.routing_key,
                BasicPublishOptions::default(),
                &payload,
                BasicProperties::default()
                    .with_content_type("application/vnd.masstransit+json".into())
                    .with_headers(FieldTable::from(headers))
                    .with_message_id(message_id.into())
                    .with_timestamp(Utc::now().timestamp() as u64)
            )
            .await?;

        log::debug!(
            "Published tournament processed message for tournament {} with action '{}' to exchange '{}' with routing key '{}'",
            tournament_id,
            action,
            self.exchange,
            self.routing_key
        );

        Ok(())
    }

    /// Checks if the publisher is connected
    pub fn is_connected(&self) -> bool {
        self.connection.is_some() && self.channel.is_some()
    }

    /// Closes the connection to RabbitMQ
    pub async fn close(&mut self) -> Result<(), PublisherError> {
        if let Some(channel) = self.channel.take() {
            channel.close(200, "Normal shutdown").await?;
        }

        if let Some(connection) = self.connection.take() {
            if let Ok(conn) = Arc::try_unwrap(connection) {
                conn.close(200, "Normal shutdown").await?;
            }
        }

        log::info!("RabbitMQ connection closed");
        Ok(())
    }
}

impl Drop for RabbitMqPublisher {
    fn drop(&mut self) {
        if self.is_connected() {
            log::warn!("RabbitMQ publisher dropped without proper closure");
        }
    }
}
