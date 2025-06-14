use clap::Parser;
use env_logger::Env;
use log::{debug, info};
use otr_processor::{
    args::Args,
    database::db::DbClient,
    model::{otr_model::OtrModel, rating_utils::create_initial_ratings},
    utils::test_utils::generate_country_mapping_players
};
use std::{collections::HashMap, time::Instant};

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

    // BEGIN TRANSACTION - executed on the connection
    client
        .client()
        .execute("BEGIN", &[])
        .await
        .expect("Failed to begin transaction");
    info!("BEGIN TRANSACTION");

    // Execute all operations
    let process_result = async {
        // 1. Rollback processing statuses of matches & tournaments
        client.rollback_processing_statuses().await;
        info!("Rollback processing statuses completed.");

        // 2. Fetch matches and players for processing
        let matches = client.get_matches().await;
        let players = client.get_players().await;
        debug!("Fetched {} matches and {} players.", matches.len(), players.len());

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

        // 8. Update all match processing statuses
        client.roll_forward_processing_statuses(&matches).await;
        info!("Processing statuses updated.");

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
            std::process::exit(1);
        }
    }
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
