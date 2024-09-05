use otr_processor::model::db::DbClient;
use std::env;
use otr_processor::model::rating_utils::initial_ratings;

#[tokio::main]
async fn main() {
    let mut client: DbClient = client().await;

    // Fetch matches and players for processing
    let matches = client.get_matches().await;
    let players = client.get_players().await;

    // 1. Generate initial ratings
    let ratings = initial_ratings(&players);
}

async fn client() -> DbClient {
    dotenv::dotenv().unwrap();

    let connection_string = env::var("CONNECTION_STRING")
        .expect("Expected CONNECTION_STRING environment variable for otr-db PostgreSQL connection.");

    DbClient::connect(connection_string.as_str())
        .await
        .expect("Expected valid database connection")
}