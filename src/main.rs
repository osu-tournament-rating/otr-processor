use clap::Parser;
use otr_processor::{
    args::Args,
    database::db::DbClient,
    messaging::RabbitMqPublisher,
    model::{otr_model::OtrModel, rating_utils::create_initial_ratings},
    utils::test_utils::generate_country_mapping_players
};
use std::{collections::HashMap, time::Instant};
use tracing::{debug, error, info, warn};
use tracing_indicatif::IndicatifLayer;
use tracing_subscriber::{prelude::*, EnvFilter};
use uuid::Uuid;

#[tokio::main]
async fn main() {
    // Used for time tracking
    let start = Instant::now();

    // Initialize env vars
    let _ = dotenv::dotenv();

    // Parse args
    let args = Args::parse();

    // Initialize tracing with indicatif integration
    init_tracing(&args.log_level);

    if args.ignore_constraints {
        info!("Ignoring database constraints");
    }

    info!("Begin processing...");

    let client: DbClient = client(&args).await;

    // Initialize RabbitMQ publisher
    let mut rabbitmq_publisher = match initialize_rabbitmq().await {
        Ok(publisher) => Some(publisher),
        Err(e) => {
            warn!("Failed to initialize RabbitMQ: {}. Continuing without messaging.", e);
            None
        }
    };

    let mut transaction_guard = client.begin_transaction().await.expect("Failed to begin transaction");
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
            warn!("No matches found to process! Check that matches have verification_status=4");
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

        // 8. Emit messages for tournaments needing stats refresh
        if let Some(ref mut publisher) = rabbitmq_publisher {
            let all_tournament_ids: Vec<i32> = tournament_info.keys().cloned().collect();

            let tournaments_needing_refresh = client.get_tournaments_needing_stats_refresh(&all_tournament_ids).await;

            let skipped = tournament_info.len() - tournaments_needing_refresh.len();
            info!(
                "Enqueueing {} of {} tournaments for stats refresh ({} unchanged)",
                tournaments_needing_refresh.len(),
                tournament_info.len(),
                skipped
            );

            for tournament_id in tournaments_needing_refresh {
                if let Some(tournament_data) = tournament_info.get(&tournament_id) {
                    let correlation_id = Some(Uuid::new_v4().to_string());

                    if let Err(e) = publisher.ensure_connected().await {
                        error!("Failed to ensure RabbitMQ connection: {}", e);
                        continue;
                    }

                    match publisher.publish_tournament_stats(tournament_id, correlation_id).await {
                        Ok(_) => info!("Enqueued stats: [{}] {}", tournament_id, tournament_data.name),
                        Err(e) => error!("Failed to publish message for tournament {}: {}", tournament_id, e)
                    }
                }
            }
        }

        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;

    match process_result {
        Ok(()) => {
            // COMMIT TRANSACTION
            if let Err(e) = transaction_guard.commit().await {
                error!("Failed to commit transaction: {}", e);
                if let Err(rollback_err) = transaction_guard.rollback().await {
                    error!("Failed to rollback transaction after commit failure: {}", rollback_err);
                } else {
                    error!("Transaction rolled back due to commit failure");
                }
                cleanup_rabbitmq(&mut rabbitmq_publisher).await;
                std::process::exit(1);
            }
            info!("COMMIT TRANSACTION");
            let end = Instant::now();
            info!("Processing complete in {:.2?}", (end - start));
        }
        Err(e) => {
            // ROLLBACK TRANSACTION
            error!("Processing failed: {}", e);
            if let Err(rollback_err) = transaction_guard.rollback().await {
                error!("Failed to rollback transaction: {}", rollback_err);
                error!("WARNING: Transaction may be left in an inconsistent state");
            } else {
                error!("ROLLBACK TRANSACTION completed");
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
            error!("Failed to connect to database: {}", e);
            error!("Application cannot start without a valid database connection");
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
            error!("Failed to close RabbitMQ connection: {}", e);
        }
    }
}

fn init_tracing(log_level: &str) {
    let indicatif_layer = IndicatifLayer::new();
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(log_level));

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(indicatif_layer.get_stderr_writer()))
        .with(indicatif_layer)
        .init();
}
