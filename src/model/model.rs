use openskill::{constant::*, model::plackett_luce::PlackettLuce, rating::default_gamma};

use crate::{
    api::api_structs::{Game, Match, PlayerRating},
    model::{decay::DecayTracker, rating_tracker::RatingTracker, structures::ruleset::Ruleset}
};

pub struct OtrModel {
    pub model: PlackettLuce,
    pub rating_tracker: RatingTracker,
    pub decay_tracker: DecayTracker
}

impl Default for OtrModel {
    fn default() -> Self {
        Self::new()
    }
}

impl OtrModel {
    pub fn new() -> OtrModel {
        OtrModel {
            rating_tracker: RatingTracker::new(),
            decay_tracker: DecayTracker,
            model: PlackettLuce::new(DEFAULT_BETA, KAPPA, default_gamma)
        }
    }

    pub fn process(&self, matches: &[Match]) {}

    /// # o!TR Match Processing
    ///
    /// This function processes a single match but serves as the heart of where all rating changes
    /// occur.
    ///
    /// Steps:
    /// 1. Apply decay if necessary to all players. Decayed ratings will become the new foundation
    /// by which this player is rated in this match.
    /// 2. Iterate through the games and identify changes in rating at a per-game level, per player.
    /// 3. Iterate through all games and compute a rating change based on the results from 1 & 1b.
    /// Although ratings are computed at a per-game level, they actually are not
    /// 4. Generate a list of 'teams' (every single player is its own team), along with a sorted vector of
    /// rankings. This gets fed into the PlackettLuce model.
    /// 5. Update the RatingTracker after the match is processed.
    fn process_match(&self, m: &Match) {}

    fn rate(&self, g: &Game, ruleset: Ruleset) {
        let tuple: (Vec<Option<&PlayerRating>>, Vec<i32>) = g
            .placements
            .iter()
            .map(|p| (self.rating_tracker.get_rating(p.player_id, ruleset), p.placement))
            .collect();
    }

    /// Applies a scaled performance penalty to negative changes in rating.
    fn apply_negative_performance_scaling(
        rating: &mut PlayerRating,
        rating_diff: f64,
        games_played: i32,
        games_total: i32,
        scaling: f64
    ) {
        if rating_diff >= 0.0 {
            panic!("Rating difference cannot be positive.")
        }

        // Rating differential is used with a scaling factor
        // to determine final rating change
        rating.rating = scaling * (rating_diff * (games_played as f64 / games_total as f64))
    }
}
