use std::collections::HashMap;
use crate::model::structures::{rating_adjustment_type::RatingAdjustmentType, ruleset::Ruleset};
use chrono::{DateTime, FixedOffset, Utc};
use serde::{Deserialize, Serialize};
use tokio_postgres::types::Timestamp;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthResponse {
    #[serde(rename = "accessToken")]
    pub token: String,

    #[serde(rename = "refreshToken")]
    pub refresh_token: String,

    /// Expire time in seconds
    #[serde(rename = "accessExpiration")]
    pub expire_in: u64
}

// POSTS data to the API.
// Simultaneously used as a record of a player's rating, regardless of source.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlayerRating {
    pub player_id: i32,
    pub ruleset: Ruleset,
    pub rating: f64,
    pub volatility: f64,
    pub percentile: f64,
    pub global_rank: i32,
    pub country_rank: i32,
    #[serde(skip_serializing)]
    pub timestamp: DateTime<FixedOffset>,
    #[serde(skip_serializing)]
    pub adjustment_type: RatingAdjustmentType
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RatingAdjustment {
    pub adjustment_type: RatingAdjustmentType,
    pub match_id: Option<i32>,
    pub rating_delta: f64,
    pub rating_before: f64,
    pub rating_after: f64,
    pub volatility_delta: f64,
    pub volatility_before: f64,
    pub volatility_after: f64,
    pub percentile_delta: f64,
    pub percentile_before: f64,
    pub percentile_after: f64,
    pub global_rank_delta: i32,
    pub global_rank_before: i32,
    pub global_rank_after: i32,
    pub country_rank_delta: i32,
    pub country_rank_before: i32,
    pub country_rank_after: i32,
    pub timestamp: DateTime<FixedOffset>
}

/// Used for POSTing rating data to the o!TR API
pub struct RatingPost {
    pub rating: PlayerRating,
    pub adjustments: Vec<RatingAdjustment>
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Match {
    pub id: i32,
    pub ruleset: Ruleset,
    pub start_time: Option<DateTime<FixedOffset>>,
    pub end_time: Option<DateTime<FixedOffset>>,
    pub games: Vec<Game>
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    pub id: i32,
    pub game_id: i64,
    pub start_time: DateTime<FixedOffset>,
    pub end_time: Option<DateTime<FixedOffset>>,
    pub placements: Vec<PlayerPlacement>
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlayerPlacement {
    pub player_id: i32,
    pub placement: i32
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Player {
    pub id: i32,
    pub username: Option<String>,
    pub country: Option<String>,
    pub rank_standard: Option<i32>,
    pub rank_taiko: Option<i32>,
    pub rank_catch: Option<i32>,
    pub rank_mania: Option<i32>,
    pub earliest_osu_global_rank: Option<i32>,
    pub earliest_osu_global_rank_date: Option<DateTime<FixedOffset>>,
    pub earliest_taiko_global_rank: Option<i32>,
    pub earliest_taiko_global_rank_date: Option<DateTime<FixedOffset>>,
    pub earliest_catch_global_rank: Option<i32>,
    pub earliest_catch_global_rank_date: Option<DateTime<FixedOffset>>,
    pub earliest_mania_global_rank: Option<i32>,
    pub earliest_mania_global_rank_date: Option<DateTime<FixedOffset>>
}

#[derive(Debug, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MatchPagedResult {
    /// Link to the next potential page of results
    pub next: Option<String>,
    /// Link to the previous potential page of results
    pub previous: Option<String>,
    /// Number of results included
    pub count: i32,
    /// List of resulting data
    pub results: Vec<Match>
}

#[derive(Debug, Clone, Serialize)]
pub struct NewPlayer {
    pub id: i32,
    pub username: Option<String>,
    pub country: Option<String>,
    pub ruleset_data: Vec<RulesetData>
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RulesetData {
    pub ruleset: Ruleset,
    pub global_rank: Option<i32>,
    pub earliest_global_rank: Option<i32>
}

#[derive(Debug, Clone, Serialize)]
pub struct NewMatch {
    pub id: i32,
    pub name: String,
    pub start_time: DateTime<FixedOffset>,
    pub end_time: DateTime<FixedOffset>,
    // Populated in the db query (uses the tournament's ruleset)
    pub ruleset: Ruleset,
    pub games: Vec<NewGame>
}

#[derive(Debug, Clone, Serialize)]
pub struct NewGame {
    pub id: i32,
    pub ruleset: Ruleset,
    pub start_time: DateTime<FixedOffset>,
    pub end_time: DateTime<FixedOffset>,
    pub scores: Vec<NewGameScore>
}

#[derive(Debug, Clone, Serialize)]
pub struct NewGameScore {
    pub id: i32,
    pub player_id: i32,
    pub game_id: i32,
    pub score: i32,
    pub placement: i32
}

#[derive(Debug, Clone, Serialize)]
pub struct NewPlayerRating {
    /// Unknown until insertion
    pub id: i32,
    pub player_id: i32,
    pub ruleset: Ruleset,
    pub rating: f64,
    pub volatility: f64,
    /// Updated once at the very end of processing
    pub percentile: f64,
    /// Updated once at the very end of processing
    pub global_rank: i32,
    /// Updated once at the very end of processing
    pub country_rank: i32,
    /// The adjustments that led to this rating object
    pub adjustments: Vec<NewRatingAdjustment>
}

#[derive(Debug, Clone, Serialize)]
pub struct NewRatingAdjustment {
    pub player_id: i32,
    /// Unknown until parent is inserted
    pub player_rating_id: i32,
    pub match_id: Option<i32>,
    pub rating_before: f64,
    pub rating_after: f64,
    pub volatility_before: f64,
    pub volatility_after: f64,
    pub timestamp: DateTime<FixedOffset>,
    pub adjustment_type: RatingAdjustmentType
}
