use crate::api::api_structs::{MatchRatingStats, RatingAdjustment};
use crate::model::structures::player_rating::PlayerRating;

pub struct RatingCalculationResult {
    pub base_ratings: Vec<PlayerRating>,
    pub rating_stats: Vec<MatchRatingStats>,
    pub adjustments: Vec<RatingAdjustment>
}