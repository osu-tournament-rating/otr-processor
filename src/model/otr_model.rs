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
use crate::model::structures::rating_adjustment_type::RatingAdjustmentType;

pub struct OtrModel {
    pub model: PlackettLuce,
    pub rating_tracker: RatingTracker,
    pub decay_tracker: DecayTracker
}

impl OtrModel {
    pub fn new(initial_player_ratings: &[PlayerRating], country_mapping: &HashMap<i32, String>) -> OtrModel {
        let mut tracker = RatingTracker::new();

        for p in initial_player_ratings {
            match p.adjustment_type {
                RatingAdjustmentType::Initial => (),
                _ => panic!("Expected rating adjustment type to be Initial!")
            }

            tracker.insert_or_update(
                p,
                country_mapping
                    .get(&p.player_id)
                    .expect("Player must have a country mapping!"),
                None
            );
        }

        tracker.sort();

        OtrModel {
            rating_tracker: tracker,
            decay_tracker: DecayTracker,
            model: PlackettLuce::new(DEFAULT_BETA, KAPPA, default_gamma)
        }
    }

    pub fn process(&self, matches: &[Match]) {
        for m in matches {
            self.process_match(m);
        }
    }

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
            let result = results.get(i).unwrap().first().unwrap();

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
        let prior_rating = rating.rating;
        rating.rating = prior_rating - (scaling * (rating_diff.abs() * (games_played as f64 / games_total as f64)))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        model::{
            otr_model::OtrModel,
            structures::{ruleset::Ruleset}
        },
        utils::test_utils::*
    };
    use approx::assert_abs_diff_eq;
    use std::collections::HashMap;
    use crate::model::structures::rating_adjustment_type::RatingAdjustmentType;

    #[test]
    fn test_rate() {
        // Add 3 players to model
        let player_ratings = vec![
            generate_player_rating(1, 1000.0, 100.0, RatingAdjustmentType::Initial),
            generate_player_rating(2, 1000.0, 100.0, RatingAdjustmentType::Initial),
            generate_player_rating(3, 1000.0, 100.0, RatingAdjustmentType::Initial),
        ];

        let countries = generate_country_mapping(player_ratings.as_slice(), "US");

        let model = OtrModel::new(player_ratings.as_slice(), &countries);

        let placements = vec![
            generate_placement(1, 2),
            generate_placement(2, 1),
            generate_placement(3, 3),
        ];

        let game = generate_game(1, &placements);

        let rating_result = model.rate(&game, Ruleset::Osu);

        // Compare the 3 rating values, ensure order is 2, 1, 3
        let result_1 = rating_result.get(&1).unwrap().0;
        let result_2 = rating_result.get(&2).unwrap().0;
        let result_3 = rating_result.get(&3).unwrap().0;

        assert!(result_2 > result_1);
        assert!(result_1 > result_3);
    }

    #[test]
    fn test_rate_match() {
        // Add 4 players to model
        let player_ratings = vec![
            generate_player_rating(1, 1000.0, 100.0, RatingAdjustmentType::Initial),
            generate_player_rating(2, 1000.0, 100.0, RatingAdjustmentType::Initial),
            generate_player_rating(3, 1000.0, 100.0, RatingAdjustmentType::Initial),
            generate_player_rating(4, 1000.0, 100.0, RatingAdjustmentType::Initial),
        ];

        let countries = generate_country_mapping(player_ratings.as_slice(), "US");

        let model = OtrModel::new(player_ratings.as_slice(), &countries);

        let placements = vec![
            generate_placement(1, 4),
            generate_placement(2, 3),
            generate_placement(3, 2),
            generate_placement(4, 1),
        ];

        let games = vec![
            generate_game(1, &placements),
            generate_game(2, &placements),
            generate_game(3, &placements),
        ];

        let game_results: Vec<HashMap<i32, (f64, f64)>> = games.iter().map(|g| model.rate(g, Ruleset::Osu)).collect();
    }

    #[test]
    fn test_negative_performance_scaling() {
        let mut rating = generate_player_rating(1, 1000.0, 100.0, RatingAdjustmentType::Initial);
        let rating_diff = -100.0;
        let games_played = 1;
        let games_total = 10;
        let scaling = 1.0;

        // User should lose 10% of what they would have lost as they only participated in 1/10 of the maps.

        OtrModel::apply_negative_performance_scaling(&mut rating, rating_diff, games_played, games_total, scaling);

        assert_abs_diff_eq!(rating.rating, 990.0);
    }
}
