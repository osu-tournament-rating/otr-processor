mod model_data;

use openskill;
use openskill::model::plackett_luce::*;
use openskill::rating::*;
use crate::constants::{default_constants};

pub fn create_model() -> PlackettLuce {
    let constants = default_constants();
    PlackettLuce::new(constants.default_beta as f64,
                                  constants.default_kappa as f64,
                                  default_gamma)
}