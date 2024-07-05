use std::collections::HashMap;

use crate::{
    api::api_structs::{Game, Match, PlayerRating},
    model::{
        constants::PERFORMANCE_SCALING_FACTOR, decay::DecayTracker, rating_tracker::RatingTracker,
        structures::ruleset::Ruleset
    },
    utils::progress_utils::progress_bar
};
use openskill::{
    constant::*,
    model::{model::Model, plackett_luce::PlackettLuce},
    rating::{default_gamma, Rating}
};
use statrs::statistics::Statistics;
use crate::model::structures::rating_adjustment_type::RatingAdjustmentType;

pub struct OtrModel {
    pub model: PlackettLuce,
    pub rating_tracker: RatingTracker,
    pub decay_tracker: DecayTracker
}

impl OtrModel {
    pub fn new(initial_player_ratings: &[PlayerRating], country_mapping: &HashMap<i32, String>) -> OtrModel {
        let mut tracker = RatingTracker::new();

        tracker.insert_or_update(initial_player_ratings, country_mapping, None);

        OtrModel {
            rating_tracker: tracker,
            decay_tracker: DecayTracker,
            model: PlackettLuce::new(DEFAULT_BETA, KAPPA, default_gamma)
        }
    }

    pub fn process(&mut self, matches: &[Match]) {
        let progress_bar = progress_bar(matches.len() as u64, "Processing match data".to_string());
        for m in matches {
            self.process_match(m);
            progress_bar.inc(1);
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
    /// 3. Iterate through all games and compute a rating change based on the results from 1 & 2.
    /// Although ratings are computed at a per-game level, they actually are not applied until the
    /// end of the match.
    fn process_match(&mut self, m: &Match) {
        // Apply decay to all players
        self.apply_decay(&m);

        let n_games = m.games.len() as i32;
        let mut rating_changes: HashMap<i32, Vec<Rating>> = HashMap::new();
        let mut country_mapping: HashMap<i32, String> = HashMap::new();
        for game in &m.games {
            let game_ratings = self.rate(&game, m.ruleset);

            for (player_id, rating) in game_ratings {
                let player_ratings = rating_changes.entry(player_id).or_insert(Vec::new());
                player_ratings.push(rating);
                country_mapping.insert(
                    player_id,
                    self.rating_tracker.get_country(player_id).unwrap().to_string()
                );
            }
        }

        let mut ratings_to_update = Vec::new();
        for p_id in rating_changes.keys() {
            let mut prior_rating = self.rating_tracker.get_rating(*p_id, m.ruleset).unwrap().clone();
            let performances = rating_changes
                .get(p_id)
                .expect(format!("Expected player {} to have performances for this match {}!", p_id, m.id).as_str());

            let n_performances = performances.len() as i32;

            // A scaling factor based on game performance
            let performance_frequency = (n_performances / n_games) as f64;

            // Calculate differences from the baseline rating
            // This works because we only update the ratings in the leaderboard once per match
            // (these performances are from the game level)
            let baseline_mu_change: f64 = performances.iter().map(|f| f.mu - prior_rating.rating).mean();
            let baseline_volatility_change: f64 = performances.iter().map(|f| f.sigma - prior_rating.volatility).mean();

            // Average the sigma changes
            let scaled_mu = (prior_rating.rating + baseline_mu_change) * performance_frequency;
            let scaled_volatility: f64 = (prior_rating.volatility + baseline_volatility_change) * performance_frequency;

            let mu_delta = scaled_mu - prior_rating.rating;

            prior_rating.rating = scaled_mu;
            prior_rating.volatility = scaled_volatility;
            prior_rating.adjustment_type = RatingAdjustmentType::Match;

            Self::apply_performance_scaling(
                &mut prior_rating,
                mu_delta,
                n_performances,
                n_games,
                PERFORMANCE_SCALING_FACTOR
            );

            ratings_to_update.push(prior_rating);
        }

        self.rating_tracker
            .insert_or_update(&ratings_to_update.as_slice(), &country_mapping, Some(m.id))
    }

    /// Applies decay to all players who participated in this match.
    fn apply_decay(&mut self, m: &Match) {
        let player_ids: Vec<i32> = m
            .games
            .iter()
            .map(|g| g.placements.iter().map(|p| p.player_id).collect::<Vec<i32>>())
            .flatten()
            .collect();

        for p_id in player_ids {
            let country = self.rating_tracker.get_country(p_id).unwrap().clone();
            self.decay_tracker.decay(
                &mut self.rating_tracker,
                p_id,
                country.as_str(),
                m.ruleset,
                m.start_time.unwrap()
            );
        }
    }

    /// Rates a Game. Returns a HashMap of <player_id, (new_rating, new_volatility)
    fn rate(&self, game: &Game, ruleset: Ruleset) -> HashMap<i32, Rating> {
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

        // Building teams to feed into the model
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

        let results: Vec<Rating> = self.model.rate(teams, placements).into_iter().flatten().collect();
        let mut new_ratings = HashMap::new();

        for i in 0..ratings.len() {
            let p_rating = ratings.get(i).unwrap();
            let result = results.get(i).unwrap().clone();

            new_ratings.insert(p_rating.unwrap().player_id, result);
        }

        new_ratings
    }

    /// Applies a scaled performance penalty to negative changes in rating.
    fn apply_performance_scaling(
        rating: &mut PlayerRating,
        rating_diff: f64,
        games_played: i32,
        games_total: i32,
        scaling: f64
    ) {
        if rating_diff >= 0.0 {
            return
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
            structures::{rating_adjustment_type::RatingAdjustmentType, ruleset::Ruleset}
        },
        utils::test_utils::*
    };
    use approx::assert_abs_diff_eq;
    use criterion::Bencher;

    #[test]
    fn test_rate() {
        // Add 3 players to model
        let player_ratings = vec![
            generate_player_rating(1, 1000.0, 100.0, RatingAdjustmentType::Initial, None),
            generate_player_rating(2, 1000.0, 100.0, RatingAdjustmentType::Initial, None),
            generate_player_rating(3, 1000.0, 100.0, RatingAdjustmentType::Initial, None),
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
        let result_1 = rating_result.get(&1).unwrap();
        let result_2 = rating_result.get(&2).unwrap();
        let result_3 = rating_result.get(&3).unwrap();

        assert!(result_2.mu > result_1.mu);
        assert!(result_1.mu > result_3.mu);
    }

    #[test]
    fn test_process() {
        // Add 4 players to model
        let player_ratings = vec![
            generate_player_rating(1, 1000.0, 100.0, RatingAdjustmentType::Initial, None),
            generate_player_rating(2, 1000.0, 100.0, RatingAdjustmentType::Initial, None),
            generate_player_rating(3, 1000.0, 100.0, RatingAdjustmentType::Initial, None),
            generate_player_rating(4, 1000.0, 100.0, RatingAdjustmentType::Initial, None),
        ];

        let countries = generate_country_mapping(player_ratings.as_slice(), "US");

        let mut model = OtrModel::new(player_ratings.as_slice(), &countries);

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

        let matches = vec![generate_match(
            1,
            Ruleset::Osu,
            &games,
            Some(chrono::Utc::now().fixed_offset())
        )];
        model.process(&matches);

        let rating_1 = model.rating_tracker.get_rating(1, Ruleset::Osu).unwrap();
        let rating_2 = model.rating_tracker.get_rating(2, Ruleset::Osu).unwrap();
        let rating_3 = model.rating_tracker.get_rating(3, Ruleset::Osu).unwrap();
        let rating_4 = model.rating_tracker.get_rating(4, Ruleset::Osu).unwrap();

        let adjustments_1 = model
            .rating_tracker
            .get_rating_adjustments(1, Ruleset::Osu)
            .expect("Expected player 1 to have adjustments");
        let adjustments_2 = model
            .rating_tracker
            .get_rating_adjustments(2, Ruleset::Osu)
            .expect("Expected player 1 to have adjustments");
        let adjustments_3 = model
            .rating_tracker
            .get_rating_adjustments(3, Ruleset::Osu)
            .expect("Expected player 1 to have adjustments");
        let adjustments_4 = model
            .rating_tracker
            .get_rating_adjustments(4, Ruleset::Osu)
            .expect("Expected player 1 to have adjustments");

        assert!(rating_4.rating > rating_3.rating);
        assert!(rating_3.rating > rating_2.rating);
        assert!(rating_2.rating > rating_1.rating);
        assert!(rating_1.rating < 1000.0);

        // Assert global ranks
        assert_eq!(rating_4.global_rank, 1);
        assert_eq!(rating_3.global_rank, 2);
        assert_eq!(rating_2.global_rank, 3);
        assert_eq!(rating_1.global_rank, 4);

        // Assert country ranks
        assert_eq!(rating_4.country_rank, 1);
        assert_eq!(rating_3.country_rank, 2);
        assert_eq!(rating_2.country_rank, 3);
        assert_eq!(rating_1.country_rank, 4);

        // Assert adjustments
        assert_eq!(adjustments_1.len(), 2);
        assert_eq!(adjustments_2.len(), 2);
        assert_eq!(adjustments_3.len(), 2);
        assert_eq!(adjustments_4.len(), 2);

        // Assert rating changes
        assert!(adjustments_1[1].rating_delta < 0.0);
        assert!(adjustments_2[1].rating_delta < 0.0);
        assert!(adjustments_3[1].rating_delta > 0.0);
        assert!(adjustments_4[1].rating_delta > 0.0);
    }

    #[test]
    fn test_negative_performance_scaling() {
        let mut rating = generate_player_rating(1, 1000.0, 100.0, RatingAdjustmentType::Initial, None);
        let rating_diff = -100.0;
        let games_played = 1;
        let games_total = 10;
        let scaling = 1.0;

        // User should lose 10% of what they would have lost as they only participated in 1/10 of the maps.

        OtrModel::apply_performance_scaling(&mut rating, rating_diff, games_played, games_total, scaling);

        assert_abs_diff_eq!(rating.rating, 990.0);
    }

    // #[bench]
    fn bench_match_processing(b: &mut Bencher) {
        let initial_ratings = generate_default_initial_ratings(1000);
        let matches = generate_matches(100, initial_ratings.as_slice());
        let country_mapping = generate_country_mapping(initial_ratings.as_slice(), "US");

        let mut model = OtrModel::new(initial_ratings.as_slice(), &country_mapping);

        b.iter(|| model.process(matches.as_slice()));
    }
}
