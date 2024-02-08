use crate::model::constants::default_constants;

pub fn is_decay_possible(mu: f64) -> bool {
    let constants = default_constants();

    mu > constants.decay_minimum as f64
}

pub fn decay_sigma(sigma: f64) -> f64 {
    let constants = default_constants();
    let new_sigma = (sigma.powi(2) + constants.volatility_growth_rate).sqrt();

    return new_sigma.min(constants.default_sigma);
}

pub fn decay_mu(mu: f64) -> f64 {
    let constants = default_constants();
    let new_mu = mu - constants.decay_rate;

    return new_mu.max(constants.decay_minimum);
}


#[cfg(test)]
mod tests {
    use crate::model::constants::default_constants;
    use crate::model::decay::{decay_mu, decay_sigma, is_decay_possible};

    #[test]
    fn test_decay_possible() {
        let mu = 500.0;
        let decay_min = default_constants().decay_minimum;

        let decay_possible = mu > (decay_min as f64);

        let result = is_decay_possible(mu);

        assert_eq!(result, decay_possible)
    }

    #[test]
    fn test_decay_sigma_standard() {
        let constants = default_constants();

        let sigma = 200.1;
        let new_sigma = decay_sigma(sigma);
        let expected = (sigma.powi(2) + constants.volatility_growth_rate).sqrt();

        assert_eq!(new_sigma, expected);
    }

    #[test]
    fn test_decay_sigma_maximum_default() {
        let constants = default_constants();

        let sigma = 999.0;
        let new_sigma = decay_sigma(sigma);
        let expected = constants.default_sigma;

        assert_eq!(new_sigma, expected);
    }

    #[test]
    fn test_decay_mu_standard() {
        let constants = default_constants();

        let mu = 1100.0;
        let new_mu = decay_mu(mu);
        let expected = mu - constants.decay_rate;

        assert_eq!(new_mu, expected);
    }

    #[test]
    fn test_decay_mu_min_decay() {
        let constants = default_constants();

        let mu = 825.0;
        let new_mu = decay_mu(mu);
        let expected = 825.0;

        assert_eq!(new_mu, expected);
    }
}