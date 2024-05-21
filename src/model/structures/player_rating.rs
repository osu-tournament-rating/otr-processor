use crate::model::structures::ruleset::Ruleset;
use openskill::rating::Rating;

#[derive(Debug, Clone)]
pub struct PlayerRating {
    pub player_id: i32,
    pub mode: Ruleset,
    pub rating: Rating,
    pub global_ranking: u32,
    pub country_ranking: u32,
    pub country: String
}
