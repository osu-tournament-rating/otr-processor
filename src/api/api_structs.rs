use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};

use crate::model::structures::{match_type::MatchType, mode::Mode, scoring_type::ScoringType, team_type::TeamType};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RatingAdjustment {
    pub player_id: i32,
    pub mode: Mode,
    pub rating_adjustment_amount: f64,
    pub volatility_adjustment_amount: f64,
    pub rating_before: f64,
    pub rating_after: f64,
    pub volatility_before: f64,
    pub volatility_after: f64,
    pub rating_adjustment_type: i32,
    pub timestamp: DateTime<FixedOffset>
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlayerMatchStats {
    pub player_id: i32,
    pub match_id: i32,
    pub won: bool,
    pub average_score: f64,
    pub average_misses: f64,
    pub average_accuracy: f64,
    pub average_placement: f64,
    pub games_won: i32,
    pub games_lost: i32,
    pub games_played: i32,
    pub teammate_ids: Vec<i32>,
    pub opponent_ids: Vec<i32>
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchRatingStats {
    pub player_id: i32,
    pub match_id: i32,
    pub match_cost: f64,
    pub rating_before: f64,
    pub rating_after: f64,
    pub rating_change: f64,
    pub volatility_before: f64,
    pub volatility_after: f64,
    pub volatility_change: f64,
    pub global_rank_before: i32,
    pub global_rank_after: i32,
    pub global_rank_change: i32,
    pub country_rank_before: i32,
    pub country_rank_after: i32,
    pub country_rank_change: i32,
    pub percentile_before: f64,
    pub percentile_after: f64,
    pub percentile_change: f64,
    pub average_teammate_rating: Option<f64>,
    pub average_opponent_rating: Option<f64>
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseStatsPost {
    pub player_id: i32,
    pub match_cost_average: f64,
    pub rating: f64,
    pub volatility: f64,
    pub mode: i32,
    pub percentile: f64,
    pub global_rank: i32,
    pub country_rank: i32
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GameWinRecord {
    pub game_id: i32,
    pub winners: Vec<i32>,
    pub losers: Vec<i32>,
    pub winner_team: i32,
    pub loser_team: i32
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MatchWinRecord {
    pub match_id: i32,
    pub loser_roster: Vec<i32>,
    pub winner_roster: Vec<i32>,
    pub loser_points: i32,
    pub winner_points: i32,
    pub winner_team: Option<i32>,
    pub loser_team: Option<i32>,
    pub match_type: Option<MatchType>
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Match {
    pub id: i32,
    pub match_id: i64,
    pub name: Option<String>,
    pub mode: Mode,
    pub start_time: Option<DateTime<FixedOffset>>,
    pub end_time: Option<DateTime<FixedOffset>>,
    pub games: Vec<Game>
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchIdMapping {
    pub id: i32,
    pub osu_match_id: i64
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerCountryMapping {
    pub player_id: i32,
    pub country: Option<String>
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    pub id: i32,
    pub ruleset: Mode,
    pub scoring_type: ScoringType,
    pub team_type: TeamType,
    pub mods: i32,
    pub game_id: i64,
    pub start_time: DateTime<FixedOffset>,
    pub end_time: Option<DateTime<FixedOffset>>,
    pub beatmap: Option<Beatmap>,
    pub match_scores: Vec<MatchScore>
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchScore {
    pub player_id: i32,
    pub team: i32,
    pub score: i32,
    pub enabled_mods: Option<i32>,
    pub misses: i32,
    pub accuracy_standard: f64,
    pub accuracy_taiko: f64,
    pub accuracy_catch: f64,
    pub accuracy_mania: f64
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Beatmap {
    pub artist: String,
    pub beatmap_id: i64,
    pub bpm: Option<f64>,
    pub mapper_id: i64,
    pub mapper_name: String,
    pub sr: f64,
    pub cs: f64,
    pub ar: f64,
    pub hp: f64,
    pub od: f64,
    pub drain_time: f64,
    pub length: f64,
    pub title: String,
    pub diff_name: Option<String>
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Player {
    pub id: i32,
    pub osu_id: i64,
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
