use openskill::rating::Rating;
use crate::model::structures::Mode::Mode;

#[derive(Debug)]
pub struct PlayerRating {
    pub player_id: i32,
    pub mode: Mode,
    pub rating: Rating
}
