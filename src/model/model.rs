use openskill::{
    constant::*,
    model::{model::Model, plackett_luce::PlackettLuce},
    rating::{default_gamma, Rating}
};
use std::collections::HashMap;

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

    /// Rates a Game. Returns a HashMap of <player_id, (new_rating, new_volatility)
    fn rate(&self, game: &Game, ruleset: Ruleset) -> HashMap<i32, (f64, f64)> {
        let (ratings, placements): (Vec<Option<&PlayerRating>>, Vec<usize>) = game
            .placements
            .iter()
            .map(|p| {
                (
                    self.rating_tracker.get_rating(p.player_id, ruleset),
                    p.placement as usize
                )
            })
            .collect();

        let mut teams = Vec::new();
        for r in &ratings {
            match r {
                Some(p_rating) => {
                    teams.push(vec![Rating {
                        mu: p_rating.rating,
                        sigma: p_rating.volatility
                    }]);
                }
                None => panic!("Expected player to have a rating!")
            }
        }

        if teams.len() != placements.len() {
            panic!("Expected rating and placement lengths to be identical!")
        }

        let results = self.model.rate(teams, placements);
        let mut changes = HashMap::new();

        for i in 0..ratings.len() {
            let p_rating = ratings.get(i).unwrap();
            let result = results.get(i).unwrap().get(0).unwrap();

            changes.insert(p_rating.unwrap().player_id, (result.mu, result.sigma));
        }

        changes
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

#[cfg(test)]
mod tests {
    use crate::api::api_structs::{Game, PlayerPlacement, PlayerRating};
    use crate::model::model::OtrModel;
    use crate::model::structures::rating_adjustment_type::RatingSource;
    use crate::model::structures::ruleset::Ruleset;

    #[test]
    fn test_rate() {
        let mut model = OtrModel::new();

        // Add 3 players to model
        model.rating_tracker.insert_or_update(
            &PlayerRating {
                player_id: 1,
                ruleset: Ruleset::Osu,
                rating: 1000.0,
                volatility: 100.0,
                percentile: 0.0,
                global_rank: 0,
                country_rank: 0,
                timestamp: Default::default(),
                source: RatingSource::Match,
                adjustments: Vec::new()
            },
            &"US".to_string()
        );
        model.rating_tracker.insert_or_update(
            &PlayerRating {
                player_id: 2,
                ruleset: Ruleset::Osu,
                rating: 1000.0,
                volatility: 100.0,
                percentile: 0.0,
                global_rank: 0,
                country_rank: 0,
                timestamp: Default::default(),
                source: RatingSource::Match,
                adjustments: Vec::new()
            },
            &"US".to_string()
        );
        model.rating_tracker.insert_or_update(
            &PlayerRating {
                player_id: 3,
                ruleset: Ruleset::Osu,
                rating: 1000.0,
                volatility: 100.0,
                percentile: 0.0,
                global_rank: 0,
                country_rank: 0,
                timestamp: Default::default(),
                source: RatingSource::Match,
                adjustments: Vec::new()
            },
            &"US".to_string()
        );

        let game = Game {
            id: 0,
            game_id: 0,
            start_time: Default::default(),
            end_time: None,
            placements: vec![
                PlayerPlacement {
                    player_id: 1,
                    placement: 2
                },
                PlayerPlacement {
                    player_id: 2,
                    placement: 1
                },
                PlayerPlacement {
                    player_id: 3,
                    placement: 3
                },
            ]
        };

        let rating_result = model.rate(&game, Ruleset::Osu);

        // Compare the 3 rating values, ensure order is 2, 1, 3
        let result_1 = rating_result.get(&1).unwrap().0;
        let result_2 = rating_result.get(&2).unwrap().0;
        let result_3 = rating_result.get(&3).unwrap().0;

        assert!(result_2 > result_1);
        assert!(result_1 > result_3);
    }
}
