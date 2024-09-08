use otr_processor::{
    model::{db::DbClient, otr_model::OtrModel, rating_utils::initial_ratings},
    utils::test_utils::generate_country_mapping_players
};
use std::{collections::HashMap, env};

#[tokio::main]
async fn main() {
    let client: DbClient = client().await;

    // 1. Fetch matches and players for processing
    let matches = client.get_matches().await;
    let players = client.get_players().await;

    // 2. Generate initial ratings
    let initial_ratings = initial_ratings(&players);

    // 3. Generate country mapping and set
    let country_mapping: HashMap<i32, String> = generate_country_mapping_players(&players);

    // 4. Create the model
    let mut model = OtrModel::new(&initial_ratings, &country_mapping);

    // 5. Process matches
    let results = model.process(&matches);

    // 6. Save results in database
    client.save_results(&results).await;
}

async fn client() -> DbClient {
    dotenv::dotenv().unwrap();

    let connection_string = env::var("CONNECTION_STRING")
        .expect("Expected CONNECTION_STRING environment variable for otr-db PostgreSQL connection.");

    DbClient::connect(connection_string.as_str())
        .await
        .expect("Expected valid database connection")
}
