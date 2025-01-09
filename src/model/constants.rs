/// The absolute minimum rating any player can have, regardless of performance or decay
pub const ABSOLUTE_RATING_FLOOR: f64 = 100.0;

/// Beta parameter for the PlackettLuce rating model
/// Controls how quickly ratings change based on expected vs actual performance
pub const BETA: f64 = DEFAULT_VOLATILITY / 2.0;

/// Number of days a player can be inactive before their rating begins to decay
pub const DECAY_DAYS: u64 = 121; // Approximately 4 months

/// Minimum rating that any player can decay to, based on their peak rating
pub const DECAY_MINIMUM: f64 = 15.0 * MULTIPLIER; // 900.0

/// Amount of rating lost per decay cycle (weekly after DECAY_DAYS)
pub const DECAY_RATE: f64 = 0.06 * MULTIPLIER; // 3.6 per week

/// Base volatility for new players
/// Higher values indicate more uncertainty in the rating
pub const DEFAULT_VOLATILITY: f64 = 5.0 * MULTIPLIER; // 300.0

/// Fallback default rating used when rating cannot be identified from osu! rank information
pub const FALLBACK_DEFAULT_RATING: f64 = 15.0 * MULTIPLIER; // 900.0

/// Arbitrary regularization parameter
pub const KAPPA: f64 = 0.0001;

/// Base multiplier used throughout the system to scale ratings
/// This brings ratings into a more readable range (e.g., 900 instead of 15)
pub const MULTIPLIER: f64 = 60.0;

/// Maximum possible initial rating in the osu! ruleset
pub const OSU_INITIAL_RATING_CEILING: f64 = MULTIPLIER * 30.0; // 1800.0

/// Minimum possible initial rating in the osu! ruleset before decay
pub const OSU_INITIAL_RATING_FLOOR: f64 = MULTIPLIER * 5.0; // 300.0

/// Scaling factor applied to rating changes based on performance frequency
/// Lower values reduce the impact of infrequent participation
pub const PERFORMANCE_SCALING_FACTOR: f64 = 0.3;

/// Tau parameter for the PlackettLuce rating model
/// Controls the system's confidence in new ratings
pub const TAU: f64 = DEFAULT_VOLATILITY / 100.0;

/// Rate at which volatility increases during decay periods
/// Squared due to working with variance rather than standard deviation
pub const DECAY_VOLATILITY_GROWTH_RATE: f64 = 0.08 * (MULTIPLIER * MULTIPLIER);

/// Weight applied to Method A in the final rating calculation
/// Method A: Uses current rating for unplayed games
pub const WEIGHT_A: f64 = 0.9;

/// Weight applied to Method B in the final rating calculation
/// Method B: Assumes last place for unplayed games
/// Always equals 1 - WEIGHT_A to ensure weights sum to 1
pub const WEIGHT_B: f64 = 1.0 - WEIGHT_A;
