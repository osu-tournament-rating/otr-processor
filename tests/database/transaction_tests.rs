use otr_processor::{
    database::db::DbClient,
    model::{otr_model::OtrModel, rating_utils::create_initial_ratings},
    utils::test_utils::generate_country_mapping_players
};
use serial_test::serial;
use std::collections::HashMap;
use tokio;

use super::test_helpers::TestDatabase;

#[tokio::test]
#[serial]
async fn test_transaction_rollback_on_processing_failure() {
    let test_db = TestDatabase::new().await.expect("Failed to create test database");
    test_db.seed_test_data().await.expect("Failed to seed test data");

    let client = DbClient::connect(&test_db.connection_string, false)
        .await
        .expect("Failed to connect");

    // Begin transaction
    client
        .client()
        .execute("BEGIN", &[])
        .await
        .expect("Failed to begin transaction");

    // Perform some operations
    client.rollback_processing_statuses().await;

    // Get the current state to verify rollback later
    let check_client = test_db.get_client().await.expect("Failed to get client");
    let initial_match_status: i32 = check_client
        .query_one("SELECT processing_status FROM matches WHERE id = 1", &[])
        .await
        .expect("Failed to query")
        .get(0);

    assert_eq!(initial_match_status, 4); // Should be rolled back to 4

    // Now simulate a failure by rolling back
    client
        .client()
        .execute("ROLLBACK", &[])
        .await
        .expect("Failed to rollback");

    // Verify that changes were rolled back
    let post_rollback_status: i32 = check_client
        .query_one("SELECT processing_status FROM matches WHERE id = 1", &[])
        .await
        .expect("Failed to query")
        .get(0);

    // Status should still be 4 since we didn't actually change it in the seed data
    assert_eq!(post_rollback_status, 4);
}

#[tokio::test]
#[serial]
async fn test_full_processing_transaction_commit() {
    let test_db = TestDatabase::new().await.expect("Failed to create test database");
    test_db.seed_test_data().await.expect("Failed to seed test data");

    let client = DbClient::connect(&test_db.connection_string, false)
        .await
        .expect("Failed to connect");

    // Begin transaction
    client
        .client()
        .execute("BEGIN", &[])
        .await
        .expect("Failed to begin transaction");

    // Run the full processing pipeline
    client.rollback_processing_statuses().await;
    let matches = client.get_matches().await;
    let players = client.get_players().await;

    let initial_ratings = create_initial_ratings(&players, &matches);
    let country_mapping: HashMap<i32, String> = generate_country_mapping_players(&players);
    let mut model = OtrModel::new(&initial_ratings, &country_mapping);
    let results = model.process(&matches);

    client.save_results(&results).await;
    client.roll_forward_processing_statuses(&matches).await;

    // Commit transaction
    client.client().execute("COMMIT", &[]).await.expect("Failed to commit");

    // Verify data was saved
    let check_client = test_db.get_client().await.expect("Failed to get client");

    let rating_count: i64 = check_client
        .query_one("SELECT COUNT(*) FROM player_ratings", &[])
        .await
        .expect("Failed to query")
        .get(0);

    assert!(rating_count > 0, "Ratings should have been saved");

    let match_status: i32 = check_client
        .query_one("SELECT processing_status FROM matches WHERE id = 1", &[])
        .await
        .expect("Failed to query")
        .get(0);

    assert_eq!(match_status, 5, "Match status should be updated to 5");
}

#[tokio::test]
#[serial]
async fn test_partial_processing_rollback() {
    let test_db = TestDatabase::new().await.expect("Failed to create test database");
    test_db.seed_test_data().await.expect("Failed to seed test data");

    let client = DbClient::connect(&test_db.connection_string, false)
        .await
        .expect("Failed to connect");

    // Begin transaction
    client
        .client()
        .execute("BEGIN", &[])
        .await
        .expect("Failed to begin transaction");

    // Perform partial operations
    client.rollback_processing_statuses().await;
    let matches = client.get_matches().await;
    let players = client.get_players().await;

    // Create initial ratings but don't save
    let initial_ratings = create_initial_ratings(&players, &matches);
    let country_mapping: HashMap<i32, String> = generate_country_mapping_players(&players);
    let mut model = OtrModel::new(&initial_ratings, &country_mapping);
    let results = model.process(&matches);

    // Save results (this would normally commit data)
    client.save_results(&results).await;

    // Check that data exists within transaction
    let tx_rating_count: i64 = client
        .client()
        .query_one("SELECT COUNT(*) FROM player_ratings", &[])
        .await
        .expect("Failed to query")
        .get(0);

    assert!(tx_rating_count > 0, "Ratings should exist within transaction");

    // Rollback instead of commit
    client
        .client()
        .execute("ROLLBACK", &[])
        .await
        .expect("Failed to rollback");

    // Verify data was NOT saved
    let check_client = test_db.get_client().await.expect("Failed to get client");

    let rating_count: i64 = check_client
        .query_one("SELECT COUNT(*) FROM player_ratings", &[])
        .await
        .expect("Failed to query")
        .get(0);

    assert_eq!(rating_count, 0, "No ratings should exist after rollback");

    let match_status: i32 = check_client
        .query_one("SELECT processing_status FROM matches WHERE id = 1", &[])
        .await
        .expect("Failed to query")
        .get(0);

    assert_eq!(match_status, 4, "Match status should remain at 4 after rollback");
}
