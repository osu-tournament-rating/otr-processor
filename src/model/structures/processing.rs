use openskill::rating::Rating;

use crate::{
    api::api_structs::{MatchRatingStats, RatingAdjustment},
    model::structures::player_rating::PlayerRating
};

use super::mode::Mode;

#[derive(Debug)]
pub struct RatingCalculationResult {
    /// List of Players (leaderboard in some sense) with applied
    /// all matches changes
    pub base_ratings: Vec<PlayerRating>,
    pub rating_stats: Vec<MatchRatingStats>,
    pub adjustments: Vec<RatingAdjustment>
}

/// User data after one match
#[derive(Clone, Debug)]
pub struct PlayerMatchData {
    pub player_id: i32,
    pub match_cost: f64,
    pub old_rating: Rating,
    pub new_rating: Rating,

    pub average_opponent_rating: Option<f64>,
    pub average_teammate_rating: Option<f64>,

    // Gets filled after
    pub old_global_ranking: u32,
    pub new_global_ranking: u32,

    pub old_country_ranking: u32,
    pub new_country_ranking: u32
}

#[derive(Clone, Debug, Default)]
pub struct ProcessedMatchData {
    pub match_id: i32,
    pub mode: Mode,
    pub players_stats: Vec<PlayerMatchData>
}

