use openskill::rating::Rating;

use crate::{
    api::api_structs::{
        BaseStats, GameWinRecord, MatchRatingStats, MatchWinRecord, PlayerMatchStats, RatingAdjustment
    },
    model::structures::player_rating::PlayerRating
};

use super::ruleset::Ruleset;

#[derive(Debug)]
pub struct RatingCalculationResult {
    /// List of Players (leaderboard in some sense) with applied
    /// all matches changes
    pub player_ratings: Vec<PlayerRating>,
    pub base_stats: Vec<BaseStats>,
    pub rating_stats: Vec<MatchRatingStats>,
    pub adjustments: Vec<RatingAdjustment>,
    pub processed_data: Vec<ProcessedMatchData>,
    pub game_win_records: Vec<GameWinRecord>,
    pub match_win_records: Vec<MatchWinRecord>,
    pub player_match_stats: Vec<PlayerMatchStats>
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
    pub mode: Ruleset,
    pub players_stats: Vec<PlayerMatchData>
}
