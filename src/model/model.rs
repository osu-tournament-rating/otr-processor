use openskill::constant::*;
use openskill::model::model::Model;
use openskill::model::plackett_luce::PlackettLuce;
use openskill::rating::{default_gamma, Rating};

use crate::api::api_structs::Match;
use crate::model::decay::DecayTracker;
use crate::model::rating_tracker::RatingTracker;

pub struct OtrModel {
    pub model: PlackettLuce,
    pub rating_tracker: RatingTracker,
    pub decay_tracker: DecayTracker
}

impl OtrModel {
    pub fn new() -> OtrModel {
        OtrModel {
            rating_tracker: RatingTracker::new(),
            decay_tracker: DecayTracker,
            model: PlackettLuce::new(DEFAULT_BETA, KAPPA, default_gamma)
        }
    }

    pub fn process(matches: &Vec<Match>) {

    }

    fn process_match(m: &Match) {

    }

    fn rate(&self, teams: Vec<Vec<Rating>>, placements: Vec<usize>) {
        self.model.rate(teams, placements)
    }
}