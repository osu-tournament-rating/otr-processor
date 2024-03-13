use crate::model::structures::mode::Mode;
use openskill::rating::Rating;

#[derive(Debug, Clone)]
pub struct PlayerRating {
    pub player_id: i32,
    pub mode: Mode,
    pub rating: Rating
}
