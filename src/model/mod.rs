mod constants;
mod decay;
mod rating_tracker;
pub mod structures;

use openskill::{
    constant::{DEFAULT_BETA, KAPPA},
    model::plackett_luce::PlackettLuce,
    rating::default_gamma
};

pub fn model() -> PlackettLuce {
    PlackettLuce::new(DEFAULT_BETA, KAPPA, default_gamma)
}
