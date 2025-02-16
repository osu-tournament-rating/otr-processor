use env_logger::Env;
use log::{debug, info};
use otr_processor::{
    database::db::DbClient,
    model::{otr_model::OtrModel, rating_utils::create_initial_ratings},
    utils::test_utils::generate_country_mapping_players
};
use std::{collections::HashMap, env};

#[tokio::main]
async fn main() {
    // Initialize the logger
    let _ = dotenv::dotenv();
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    info!("Begin processing...");

    let client: DbClient = client().await;

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

    info!("Processing complete");
}

async fn client() -> DbClient {
    let connection_string = env::var("CONNECTION_STRING")
        .expect("Expected CONNECTION_STRING environment variable for otr-db PostgreSQL connection.");

    DbClient::connect(connection_string.as_str())
        .await
        .expect("Expected valid database connection")
}
