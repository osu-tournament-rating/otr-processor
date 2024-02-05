use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RatingAdjustmentDTO {
    pub player_id: i32,
    pub mode: i32,
    pub rating_adjustment_amount: f64,
    pub volatility_adjustment_amount: f64,
    pub rating_before: f64,
    pub rating_after: f64,
    pub volatility_before: f64,
    pub volatility_after: f64,
    pub rating_adjustment_type: i32,
    pub timestamp: DateTime<FixedOffset>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerMatchStatsDTO {
    pub player_id: i32,
    pub match_id: i32,
    pub won: bool,
    pub average_score: i32,
    pub average_misses: f64,
    pub average_accuracy: f64,
    pub average_placement: f64,
    pub games_won: i32,
    pub games_lost: i32,
    pub games_played: i32,
    pub teammate_ids: Vec<i32>,
    pub opponent_ids: Vec<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchRatingStatsDTO {
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
    pub average_opponent_rating: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseStatsPostDTO {
    pub player_id: i32,
    pub match_cost_average: f64,
    pub rating: f64,
    pub volatility: f64,
    pub mode: i32,
    pub percentile: f64,
    pub global_rank: i32,
    pub country_rank: i32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameWinRecordDTO {
    pub game_id: i32,
    pub winners: Vec<i32>,
    pub losers: Vec<i32>,
    pub winner_team: i32,
    pub loser_team: i32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchWinRecordDTO {
    pub match_id: i32,
    pub team_blue: Vec<i32>,
    pub team_red: Vec<i32>,
    pub blue_points: i32,
    pub red_points: i32,
    pub winner_team: Option<i32>,
    pub loser_team: Option<i32>,
    pub match_type: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchDTO {
    pub id: i32,
    pub match_id: i64,
    pub name: Option<String>,
    pub start_time: Option<DateTime<FixedOffset>>,
    pub end_time: Option<DateTime<FixedOffset>>,
    pub games: Vec<GameDTO>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameDTO {
    pub id: i32,
    pub play_mode: i32,
    pub scoring_type: i32,
    pub team_type: i32,
    pub mods: i32,
    pub game_id: i64,
    pub start_time: DateTime<FixedOffset>,
    pub end_time: Option<DateTime<FixedOffset>>,
    pub beatmap: Option<BeatmapDTO>,
    pub match_scores: Vec<MatchScoreDTO>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchScoreDTO {
    pub player_id: i32,
    pub team: i32,
    pub score: i64,
    pub enabled_mods: Option<i32>,
    pub misses: i32,
    pub accuracy_standard: f64,
    pub accuracy_taiko: f64,
    pub accuracy_catch: f64,
    pub accuracy_mania: f64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BeatmapDTO {
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
    pub diff_name: Option<String>,
}
