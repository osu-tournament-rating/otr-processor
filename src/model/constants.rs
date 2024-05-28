// Model constants
pub static MULTIPLIER: f64 = 45.0;
pub static SIGMA: f64 = 5.0 * MULTIPLIER;
pub static MU: f64 = 15.0 * MULTIPLIER;
pub static TAU: f64 = SIGMA / 100.0;
pub static BETA: f64 = SIGMA / 2.0;
pub static KAPPA: f64 = 0.0001;
pub static DECAY_DAYS: u64 = 115;
pub static DECAY_MINIMUM: f64 = 18.0 * MULTIPLIER;
pub static DECAY_RATE: f64 = 0.06 * MULTIPLIER;
pub static ABSOLUTE_RATING_FLOOR: f64 = 100.0;
// Default rating constants for osu!
pub static OSU_RATING_FLOOR: f64 = 5.0;
pub static OSU_RATING_CEILING: f64 = 30.0;
pub static OSU_RATING_INTERCEPT: f64 = 45.0;
pub static OSU_RATING_SLOPE: f64 = 3.2;

// Computed constants
lazy_static! {
    pub static ref VOLATILITY_GROWTH_RATE: f64 = 0.08 * (f64::powf(MULTIPLIER, 2.0));
}

pub const BLUE_TEAM_ID: i32 = 1;
pub const RED_TEAM_ID: i32 = 2;
