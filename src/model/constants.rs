// Model constants
pub const MULTIPLIER: f64 = 45.0;
pub const SIGMA: f64 = 5.0 * MULTIPLIER;
pub const MU: f64 = 15.0 * MULTIPLIER;
pub const TAU: f64 = SIGMA / 100.0;
pub const BETA: f64 = SIGMA / 2.0;
pub const KAPPA: f64 = 0.0001;
pub const DECAY_DAYS: u64 = 115;
pub const DECAY_MINIMUM: f64 = 18.0 * MULTIPLIER;
pub const DECAY_RATE: f64 = 0.06 * MULTIPLIER;
pub const ABSOLUTE_RATING_FLOOR: f64 = 100.0;
// Default rating constants for osu!
pub const OSU_RATING_FLOOR: f64 = 5.0;
pub const OSU_RATING_CEILING: f64 = 30.0;
pub const OSU_RATING_INTERCEPT: f64 = 45.0;
pub const OSU_RATING_SLOPE: f64 = 3.2;
pub const VOLATILITY_GROWTH_RATE: f64 = 0.08 * (MULTIPLIER * MULTIPLIER);
pub const BLUE_TEAM_ID: i32 = 1;
pub const RED_TEAM_ID: i32 = 2;
