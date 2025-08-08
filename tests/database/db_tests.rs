use chrono::{FixedOffset, Utc};
use otr_processor::{
    database::{db::DbClient, db_structs::*},
    model::structures::{rating_adjustment_type::RatingAdjustmentType, ruleset::Ruleset}
};
use serial_test::serial;

use super::test_helpers::TestDatabase;
use crate::common::init_test_env;

#[tokio::test]
#[serial]
async fn test_get_players() {
    init_test_env();
    let test_db = TestDatabase::new().await.expect("Failed to create test database");
    test_db.seed_test_data().await.expect("Failed to seed test data");

    let db_client = DbClient::connect(&test_db.connection_string, false)
        .await
        .expect("Failed to connect");

    let players = db_client.get_players().await;

    assert_eq!(players.len(), 4);
    assert_eq!(players[0].username, Some("TestPlayer1".to_string()));
    assert_eq!(players[1].username, Some("TestPlayer2".to_string()));
    assert_eq!(players[2].username, Some("TestPlayer3".to_string()));
    assert_eq!(players[3].username, Some("TestPlayer4".to_string()));

    // Check player ruleset data
    assert!(players[0].ruleset_data.is_some());
    let ruleset_data = players[0].ruleset_data.as_ref().unwrap();
    assert_eq!(ruleset_data.len(), 1);
    assert_eq!(ruleset_data[0].ruleset, Ruleset::Osu);
    assert_eq!(ruleset_data[0].global_rank, 1000);
}

#[tokio::test]
#[serial]
async fn test_get_matches() {
    init_test_env();
    let test_db = TestDatabase::new().await.expect("Failed to create test database");
    test_db.seed_test_data().await.expect("Failed to seed test data");

    let db_client = DbClient::connect(&test_db.connection_string, false)
        .await
        .expect("Failed to connect");

    let matches = db_client.get_matches().await;

    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].id, 1);
    assert_eq!(matches[1].id, 2);

    // Check games
    assert_eq!(matches[0].games.len(), 2);
    assert_eq!(matches[1].games.len(), 1);

    // Check scores
    assert_eq!(matches[0].games[0].scores.len(), 4);
    assert_eq!(matches[0].games[1].scores.len(), 4);
    assert_eq!(matches[1].games[0].scores.len(), 2);
}

#[tokio::test]
#[serial]
async fn test_save_results() {
    init_test_env();
    let test_db = TestDatabase::new().await.expect("Failed to create test database");
    test_db.seed_test_data().await.expect("Failed to seed test data");

    let db_client = DbClient::connect(&test_db.connection_string, false)
        .await
        .expect("Failed to connect");

    // Create test data with proper timestamp
    let timestamp = Utc::now().with_timezone(&FixedOffset::east_opt(0).unwrap());

    let player_ratings = vec![
        PlayerRating {
            id: 1,
            player_id: 1,
            ruleset: Ruleset::Osu,
            rating: 1500.0,
            volatility: 200.0,
            percentile: 0.75,
            global_rank: 100,
            country_rank: 10,
            adjustments: vec![RatingAdjustment {
                player_id: 1,
                adjustment_type: RatingAdjustmentType::Match,
                ruleset: Ruleset::Osu,
                rating_before: 1200.0,
                volatility_before: 300.0,
                rating_after: 1500.0,
                volatility_after: 200.0,
                timestamp,
                match_id: Some(1)
            }]
        },
        PlayerRating {
            id: 2,
            player_id: 2,
            ruleset: Ruleset::Osu,
            rating: 1400.0,
            volatility: 250.0,
            percentile: 0.65,
            global_rank: 200,
            country_rank: 20,
            adjustments: vec![RatingAdjustment {
                player_id: 2,
                adjustment_type: RatingAdjustmentType::Initial,
                ruleset: Ruleset::Osu,
                rating_before: 1200.0,
                volatility_before: 300.0,
                rating_after: 1400.0,
                volatility_after: 250.0,
                timestamp,
                match_id: None
            }]
        },
    ];

    // Save results
    db_client.save_results(&player_ratings).await;

    // Verify data was saved
    let check_client = test_db.get_client().await.expect("Failed to get client");

    let rating_count: i64 = check_client
        .query_one("SELECT COUNT(*) FROM player_ratings", &[])
        .await
        .expect("Failed to query")
        .get(0);

    let adjustment_count: i64 = check_client
        .query_one("SELECT COUNT(*) FROM rating_adjustments", &[])
        .await
        .expect("Failed to query")
        .get(0);

    assert_eq!(rating_count, 2);
    assert_eq!(adjustment_count, 2); // We have 2 adjustments, one for each player rating
}

#[tokio::test]
#[serial]
async fn test_empty_database() {
    init_test_env();
    let test_db = TestDatabase::new().await.expect("Failed to create test database");
    // Don't seed data

    let db_client = DbClient::connect(&test_db.connection_string, false)
        .await
        .expect("Failed to connect");

    let players = db_client.get_players().await;
    let matches = db_client.get_matches().await;

    assert_eq!(players.len(), 0);
    assert_eq!(matches.len(), 0);
}
