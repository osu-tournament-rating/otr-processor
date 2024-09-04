use otr_processor::model::db::DbClient;
use std::env;

#[tokio::main]
async fn main() {
    dotenv::dotenv().unwrap();
    
    let connection_string = env::var("CONNECTION_STRING")
        .expect("Expected CONNECTION_STRING environment variable for otr-db PostgreSQL connection.");
    
    let db_client = DbClient::connect(connection_string.as_str()).await.expect("Expected valid database connection");
    db_client.get_matches().await;
}
