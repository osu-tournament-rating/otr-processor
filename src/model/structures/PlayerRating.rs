use crate::model::structures::Mode::Mode;

pub struct PlayerRating {
    pub player_id: i32,
    pub mode: Mode,
    pub mu: f64,
    pub sigma: f64,
}