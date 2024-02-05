pub struct RatingConstants {
    pub multiplier: i32,
    pub default_mu: f32,
    pub default_sigma: f32,
    pub default_tau: f32,
    pub default_beta: f32,
    pub default_kappa: f32,
    pub decay_minimum: i32,
    pub decay_days: i32,
    pub decay_rate: f32,
    pub volatility_growth_rate: f32
}

pub fn default_constants() -> RatingConstants {
    let multiplier = 45;
    let sigma = (25.0 / 5.0) * multiplier as f32;

    RatingConstants {
        multiplier,
        default_mu: 15.0 * multiplier as f32,
        default_sigma: sigma,
        default_tau: sigma / 100.0,
        default_beta: sigma / 2.0,
        default_kappa: 0.0001,
        decay_minimum: 825,
        decay_days: 115,
        decay_rate: 0.06 * multiplier as f32,
        volatility_growth_rate: 0.08 * (i32::pow(multiplier, 2) as f32),
    }
}