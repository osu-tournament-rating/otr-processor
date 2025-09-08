use clap::Parser;
use env_logger::Env;
use log::{debug, info};
use otr_processor::{
    args::Args,
    database::db::DbClient,
    messaging::RabbitMqPublisher,
    model::{otr_model::OtrModel, rating_utils::create_initial_ratings},
    utils::test_utils::generate_country_mapping_players
};
use std::{collections::HashMap, time::Instant};
use uuid::Uuid;

#[tokio::main]
async fn main() {
    // Used for time tracking
    let start = Instant::now();

    // Initialize env vars
    let _ = dotenv::dotenv();

    // Parse args
    let args = Args::parse();

    // Initialize logger
    env_logger::Builder::from_env(Env::default().default_filter_or(&args.log_level)).init();

    if args.ignore_constraints {
        info!("Ignoring database constraints");
    }

    info!("Begin processing...");

    let client: DbClient = client(&args).await;

    // Initialize RabbitMQ publisher
    let mut rabbitmq_publisher = match initialize_rabbitmq().await {
        Ok(publisher) => Some(publisher),
        Err(e) => {
            log::warn!("Failed to initialize RabbitMQ: {}. Continuing without messaging.", e);
            None
        }
    };

    // BEGIN TRANSACTION - executed on the connection
    client
        .client()
        .execute("BEGIN", &[])
        .await
        .expect("Failed to begin transaction");
    info!("BEGIN TRANSACTION");

    // Execute all operations
    let process_result = async {
        // 1. Calculate and update game score placements
        // This must happen before data fetching and rating processing
        client.calculate_and_update_game_score_placements().await;
        info!("Game score placements calculated and updated.");

        // 2. Fetch matches and players for processing
        let matches = client.get_matches().await;
        let players = client.get_players().await;

        if matches.is_empty() {
            log::warn!("No matches found to process! Check that matches have verification_status=4");
        } else {
            info!("Found {} matches ready for processing", matches.len());
        }

        debug!("Fetched {} matches and {} players.", matches.len(), players.len());

        // Fetch tournament information for processed matches
        let tournament_info = client.get_tournament_info_for_matches(&matches).await;
        info!(
            "Fetched tournament information for {} tournaments.",
            tournament_info.len()
        );

        // 3. Generate initial ratings
        let initial_ratings = create_initial_ratings(&players, &matches);
        info!("Initial ratings generated.");

        // 4. Generate country mapping and set
        let country_mapping: HashMap<i32, String> = generate_country_mapping_players(&players);
        info!("Country mapping generated.");

        // 5. Create the model
        let mut model = OtrModel::new(&initial_ratings, &country_mapping);
        info!("OTR model created.");

        // 6. Process matches
        let results = model.process(&matches);
        info!("Matches processed.");

        // 7. Save results in database
        client.save_results(&results).await;
        info!("Results saved to database.");

        // 8. Emit messages for processed tournaments
        if let Some(ref mut publisher) = rabbitmq_publisher {
            for (tournament_id, tournament_data) in &tournament_info {
                let correlation_id = Some(Uuid::new_v4().to_string());

                // Ensure connection is healthy before publishing
                if let Err(e) = publisher.ensure_connected().await {
                    log::error!("Failed to ensure RabbitMQ connection: {}", e);
                    continue;
                }

                match publisher.publish_tournament_stats(*tournament_id, correlation_id).await {
                    Ok(_) => debug!(
                        "Published tournament stats message for tournament {}: {}",
                        tournament_id, tournament_data.name
                    ),
                    Err(e) => log::error!("Failed to publish message for tournament {}: {}", tournament_id, e)
                }
            }
        }

        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;

    match process_result {
        Ok(()) => {
            // COMMIT TRANSACTION
            match client.client().execute("COMMIT", &[]).await {
                Ok(_) => {
                    info!("COMMIT TRANSACTION");
                    let end = Instant::now();
                    info!("Processing complete in {:.2?}", (end - start));
                }
                Err(e) => {
                    log::error!("Failed to commit transaction: {}", e);
                    // Attempt to rollback the transaction before exiting
                    match client.client().execute("ROLLBACK", &[]).await {
                        Ok(_) => log::error!("Transaction rolled back due to commit failure"),
                        Err(rollback_err) => log::error!("Failed to rollback transaction: {}", rollback_err)
                    }
                    cleanup_rabbitmq(&mut rabbitmq_publisher).await;
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            // ROLLBACK TRANSACTION
            log::error!("Processing failed: {}", e);
            match client.client().execute("ROLLBACK", &[]).await {
                Ok(_) => log::error!("ROLLBACK TRANSACTION completed"),
                Err(rollback_err) => {
                    log::error!("Failed to rollback transaction: {}", rollback_err);
                    log::error!("WARNING: Transaction may be left in an inconsistent state");
                }
            }
            cleanup_rabbitmq(&mut rabbitmq_publisher).await;
            std::process::exit(1);
        }
    }

    // Clean up RabbitMQ connection
    cleanup_rabbitmq(&mut rabbitmq_publisher).await;
}

async fn client(args: &Args) -> DbClient {
    let connection_string =
        std::env::var("CONNECTION_STRING").expect("CONNECTION_STRING environment variable must be set");

    match DbClient::connect(&connection_string, args.ignore_constraints).await {
        Ok(client) => client,
        Err(e) => {
            log::error!("Failed to connect to database: {}", e);
            log::error!("Application cannot start without a valid database connection");
            std::process::exit(1);
        }
    }
}

async fn initialize_rabbitmq() -> Result<RabbitMqPublisher, Box<dyn std::error::Error>> {
    let rabbitmq_url =
        std::env::var("RABBITMQ_URL").unwrap_or_else(|_| "amqp://admin:admin@localhost:5672".to_string());

    let routing_key =
        std::env::var("RABBITMQ_ROUTING_KEY").unwrap_or_else(|_| "processing.stats.tournaments".to_string());

    let mut publisher = RabbitMqPublisher::new(routing_key.clone(), routing_key);
    publisher.connect(&rabbitmq_url).await?;

    Ok(publisher)
}

async fn cleanup_rabbitmq(publisher: &mut Option<RabbitMqPublisher>) {
    if let Some(mut publisher) = publisher.take() {
        if let Err(e) = publisher.close().await {
            log::error!("Failed to close RabbitMQ connection: {}", e);
        }
    }
}
