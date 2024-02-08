use openskill::model::plackett_luce::PlackettLuce;
use openskill::rating::default_gamma;
use crate::model::constants::default_constants;

pub fn create_model() -> PlackettLuce {
    let constants = default_constants();
    PlackettLuce::new(constants.default_beta as f64,
                      constants.default_kappa as f64,
                      default_gamma)
}

pub fn mu_for_rank(rank: i32) -> f64 {
    let constants = default_constants();
    let multiplier = constants.multiplier as f64;
    let val = multiplier * (45.0 - (3.2 * (rank as f64).ln()));

    if val < multiplier * 5.0 {
        return multiplier * 5.0;
    }

    if val > multiplier * 30.0 {
        return multiplier * 30.0;
    }

    return val;
}

#[cfg(test)]
mod tests {
    use crate::model::model::mu_for_rank;

    #[test]
    fn mu_for_rank_returns_correct_min() {
        let rank = 1_000_000; // Some 7 digit player
        let expected = 225.0; // The minimum

        let value = mu_for_rank(rank);

        assert_eq!(expected, value);
    }

    #[test]
    fn mu_for_rank_returns_correct_max() {
        let rank = 1; // Some 7 digit player
        let expected = 1350.0; // The minimum

        let value = mu_for_rank(rank);

        assert_eq!(expected, value);
    }

    #[test]
    fn mu_for_rank_returns_correct_10k() {
        let rank = 10000; // Some 7 digit player
        let expected = 698.7109864354294; // The minimum

        let value = mu_for_rank(rank);

        assert!((expected - value).abs() < 0.000001);
    }

    #[test]
    fn mu_for_rank_returns_correct_500() {
        let rank = 500; // Some 7 digit player
        let expected = 1130.0964338272045; // The minimum

        let value = mu_for_rank(rank);

        assert!((expected - value).abs() < 0.000001);
    }
}