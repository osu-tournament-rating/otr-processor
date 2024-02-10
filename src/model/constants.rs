pub struct RatingConstants {
    pub multiplier: i32,
    pub default_mu: f64,
    pub default_sigma: f64,
    pub default_tau: f64,
    pub default_beta: f64,
    pub default_kappa: f64,
    pub decay_minimum: f64,
    pub decay_days: i64,
    pub decay_rate: f64,
    pub volatility_growth_rate: f64
}

pub fn default_constants() -> RatingConstants {
    let multiplier = 45;
    let sigma = (25.0 / 5.0) * multiplier as f64;

    RatingConstants {
        multiplier,
        default_mu: 15.0 * multiplier as f64,
        default_sigma: sigma,
        default_tau: sigma / 100.0,
        default_beta: sigma / 2.0,
        default_kappa: 0.0001,
        decay_minimum: 825.0,
        decay_days: 115,
        decay_rate: 0.06 * multiplier as f64,
        volatility_growth_rate: 0.08 * (i32::pow(multiplier, 2) as f64),
    }
}