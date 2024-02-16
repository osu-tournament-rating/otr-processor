// Model constants
pub static MULTIPLIER: f64 = 45.0;
pub static SIGMA: f64 = (25.0 / 5.0) * MULTIPLIER;
pub static MU: f64 = 15.0 * MULTIPLIER;
pub static TAU: f64 = SIGMA / 100.0;
pub static BETA: f64 = SIGMA / 2.0;
pub static KAPPA: f64 = 0.0001;
pub static DECAY_DAYS: u64 = 115;
pub static DECAY_MINIMUM: f64 = MULTIPLIER * 18.0;
pub static DECAY_RATE: f64 = 0.06 * MULTIPLIER;

// Initial rating constants
pub static RATING_INTERCEPT: f64 = 45.0;
pub static RATING_SLOPE: f64 = 3.2;
pub static RATING_FLOOR: f64 = 5.0;
pub static RATING_CIELING: f64 = 30.0;

// Computed constants
lazy_static! {
    pub static ref VOLATILITY_GROWTH_RATE: f64 = 0.08 * (f64::powf(MULTIPLIER, 2.0));
}
