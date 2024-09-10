// Model constants
pub const MULTIPLIER: f64 = 60.0;
pub const DEFAULT_VOLATILITY: f64 = 5.0 * MULTIPLIER;
pub const DEFAULT_RATING: f64 = 15.0 * MULTIPLIER;
pub const TAU: f64 = DEFAULT_VOLATILITY / 100.0;
pub const BETA: f64 = DEFAULT_VOLATILITY / 2.0;
pub const KAPPA: f64 = 0.0001;
pub const DECAY_DAYS: u64 = 115;
pub const DECAY_MINIMUM: f64 = 18.0 * MULTIPLIER;
pub const DECAY_RATE: f64 = 0.06 * MULTIPLIER;
pub const ABSOLUTE_RATING_FLOOR: f64 = 100.0;
// Default rating constants for osu!
pub const OSU_RATING_FLOOR: f64 = MULTIPLIER * 5.0;
pub const OSU_RATING_CEILING: f64 = MULTIPLIER * 30.0;
pub const VOLATILITY_GROWTH_RATE: f64 = 0.08 * (MULTIPLIER * MULTIPLIER);
pub const PERFORMANCE_SCALING_FACTOR: f64 = 0.3;
pub const WEIGHT_A: f64 = 0.9;
pub const WEIGHT_B: f64 = 1.0 - WEIGHT_A;
