use serde::{Deserialize, Serialize};
use std::env;

/// Configuration for RabbitMQ connection and messaging
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RabbitMqConfig {
    /// RabbitMQ host address
    pub host: String,
    /// RabbitMQ username for authentication
    pub username: String,
    /// RabbitMQ password for authentication
    pub password: String,
    /// Virtual host to use (default: "/")
    pub vhost: String,
    /// Port number (default: 5672)
    pub port: u16,
    /// Exchange name for tournament processing events
    pub exchange: String,
    /// Routing key for tournament processed messages
    pub routing_key: String,
}

impl RabbitMqConfig {
    /// Creates a new RabbitMQ configuration from environment variables
    pub fn from_env() -> Result<Self, env::VarError> {
        let routing_key = env::var("RABBITMQ_ROUTING_KEY")
            .unwrap_or_else(|_| "processing.ratings.tournaments".to_string());
        
        Ok(Self {
            host: env::var("RABBITMQ_HOST").unwrap_or_else(|_| "localhost".to_string()),
            username: env::var("RABBITMQ_USERNAME")?,
            password: env::var("RABBITMQ_PASSWORD")?,
            vhost: env::var("RABBITMQ_VHOST").unwrap_or_else(|_| "/".to_string()),
            port: env::var("RABBITMQ_PORT")
                .unwrap_or_else(|_| "5672".to_string())
                .parse()
                .unwrap_or(5672),
            exchange: routing_key.clone(),
            routing_key,
        })
    }

    /// Builds the AMQP connection URL from the configuration
    pub fn connection_url(&self) -> String {
        format!(
            "amqp://{}:{}@{}:{}/{}",
            self.username,
            self.password,
            self.host,
            self.port,
            self.vhost.replace('/', "%2F")
        )
    }
}

impl Default for RabbitMqConfig {
    fn default() -> Self {
        let routing_key = "processing.ratings.tournaments".to_string();
        Self {
            host: "localhost".to_string(),
            username: "admin".to_string(),
            password: "admin".to_string(),
            vhost: "/".to_string(),
            port: 5672,
            exchange: routing_key.clone(),
            routing_key,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_url() {
        let config = RabbitMqConfig {
            host: "rabbitmq.example.com".to_string(),
            username: "user".to_string(),
            password: "pass".to_string(),
            vhost: "/".to_string(),
            port: 5672,
            exchange: "test.exchange".to_string(),
            routing_key: "test.key".to_string(),
        };

        assert_eq!(
            config.connection_url(),
            "amqp://user:pass@rabbitmq.example.com:5672/%2F"
        );
    }

    #[test]
    fn test_connection_url_with_custom_vhost() {
        let config = RabbitMqConfig {
            host: "localhost".to_string(),
            username: "admin".to_string(),
            password: "secret".to_string(),
            vhost: "/myapp".to_string(),
            port: 5673,
            exchange: "events".to_string(),
            routing_key: "app.events".to_string(),
        };

        assert_eq!(
            config.connection_url(),
            "amqp://admin:secret@localhost:5673/%2Fmyapp"
        );
    }
}