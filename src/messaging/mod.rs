pub mod config;
pub mod publisher;

#[cfg(test)]
mod tests;

pub use config::RabbitMqConfig;
pub use publisher::{PublisherError, RabbitMqPublisher, TournamentProcessedMessage};
