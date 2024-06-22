pub(crate) mod structures;
mod constants;

use openskill::constant::{DEFAULT_BETA, KAPPA};
use openskill::model::plackett_luce::PlackettLuce;
use openskill::rating::default_gamma;


pub fn model() -> PlackettLuce {
    PlackettLuce::new(DEFAULT_BETA, KAPPA, default_gamma)
}

