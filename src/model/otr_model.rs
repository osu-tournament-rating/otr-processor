use crate::{
    database::db_structs::{Game, GameScore, Match, PlayerRating, RatingAdjustment},
    model::{
        constants::{WEIGHT_A, WEIGHT_B},
        decay::DecayTracker,
        rating_tracker::RatingTracker,
        structures::rating_adjustment_type::RatingAdjustmentType
    },
    utils::progress_utils::progress_bar
};
use itertools::Itertools;
use openskill::{
    constant::*,
    model::{model::Model, plackett_luce::PlackettLuce},
    rating::{Rating, TeamRating}
};
use std::collections::HashMap;

pub struct OtrModel {
    pub model: PlackettLuce,
    pub rating_tracker: RatingTracker,
    pub decay_tracker: DecayTracker
}

impl OtrModel {
    fn default_gamma_2(_: f64, k: f64, _: &TeamRating) -> f64 {
        0.5 / k
    }

    pub fn new(initial_player_ratings: &[PlayerRating], country_mapping: &HashMap<i32, String>) -> OtrModel {
        let mut tracker = RatingTracker::new();

        tracker.set_country_mapping(country_mapping.clone());
        tracker.insert_or_update(initial_player_ratings);

        OtrModel {
            rating_tracker: tracker,
            decay_tracker: DecayTracker,
            model: PlackettLuce::new(DEFAULT_BETA, KAPPA, Self::default_gamma_2)
        }
    }

    /// # o!TR Match Processing
    ///
    /// This function processes a single match but serves as the heart of where all rating changes
    /// occur.
    ///
    /// Steps:
    /// 1. Apply decay if necessary to all players. Decayed ratings will become the new foundation
    ///     by which this player is rated in this match.
    /// 2. Iterate through the games and identify changes in rating at a per-game level, per player.
    /// 3. Iterate through all games and compute a rating change based on the results from 1 & 2.
    ///     Although ratings are computed at a per-game level, they actually are not applied until the
    ///     end of the match.
    pub fn process(&mut self, matches: &[Match]) -> Vec<PlayerRating> {
        let progress_bar = progress_bar(matches.len() as u64, "Processing match data".to_string());

        for m in matches {
            self.process_match(m);

            if let Some(pb) = &progress_bar {
                pb.inc(1);
            }
        }

        if let Some(pb) = &progress_bar {
            pb.finish();
        }

        self.rating_tracker.sort();
        self.rating_tracker.get_all_ratings()
    }

    fn process_match(&mut self, match_: &Match) {
        // Apply decay to all players
        self.apply_decay(match_);

        // 1. Generate ratings
        let ratings_a = self.generate_ratings(match_);
        let ratings_b = self.generate_ratings_b(match_);

        // 2. Do math
        let calc_a = self.calc_a(ratings_a, match_);
        let calc_b = self.calc_b(ratings_b, match_);
        let final_results = self.calc_weighted_rating(&calc_a, &calc_b);

        // 3. Update values in the rating tracker
        self.apply_results(match_, &final_results)
    }

    /// Generates ratings for each player in the match
    fn generate_ratings(&self, match_: &Match) -> HashMap<i32, Vec<Rating>> {
        let mut map: HashMap<i32, Vec<Rating>> = HashMap::new();
        for game in &match_.games {
            let game_rating_result = self.rate(game);
            for (k, v) in game_rating_result {
                // Push to the vector of ratings in map for each player
                map.entry(k).or_default().push(v);
            }
        }

        map
    }

    // TODO: Document
    fn generate_ratings_b(&self, match_: &Match) -> HashMap<i32, Vec<Rating>> {
        let mut cloned_match = match_.clone();
        // 1. Identify all players who played
        // 2. For each game, if any player did not play,
        //  create a new score with the placement equal to the minimum for that game.

        let participants = self.participants(&mut cloned_match);
        self.apply_tie_for_last_scores(&mut cloned_match, &participants);

        self.generate_ratings(&cloned_match)
    }

    /// Returns a vector containing all player ids which participated in this match
    fn participants(&self, match_: &mut Match) -> Vec<i32> {
        let scores = match_
            .games
            .iter()
            .flat_map(|g| g.scores.iter().map(|s| s.player_id))
            .collect::<Vec<i32>>();
        scores.iter().unique().copied().collect()
    }

    fn apply_tie_for_last_scores(&self, match_: &mut Match, ids: &[i32]) {
        // Iterate through all games etc.
        for g in match_.games.iter_mut() {
            // max = worst placement (1 = best)
            let worst_placement = g.scores.iter().map(|f| f.placement).max().unwrap();

            // We use + 1 here because players who did not participate
            // are classified as placing worse than the worst player in this lobby
            let tie_for_last_placement = worst_placement + 1;

            let ids_not_present = ids
                .iter()
                .filter(|id| !g.scores.iter().any(|s| s.player_id == **id))
                .collect::<Vec<&i32>>();
            for id in ids_not_present {
                // Create the new score
                g.scores.push(GameScore {
                    id: 0,
                    player_id: *id,
                    game_id: g.id,
                    score: 0,
                    placement: tie_for_last_placement
                })
            }
        }
    }

    // Rates a game in OpenSkill returning a HashMap<player_id, Rating>
    fn rate(&self, game: &Game) -> HashMap<i32, Rating> {
        // We use vectors instead of HashMaps because
        // the input indices directly correlate to the
        // OpenSkill output indices.
        //
        // If we used HashMaps instead, we would lose the ability
        // to track inputs with the OpenSkill outputs.
        let mut player_ratings = Vec::new();
        let mut placements = Vec::new();

        for score in &game.scores {
            player_ratings.push(
                self.rating_tracker
                    .get_rating(score.player_id, game.ruleset)
                    .unwrap_or_else(|| {
                        panic!(
                            "Expected player {:?} to have a rating for ruleset {:?}",
                            score.player_id, game.ruleset
                        )
                    })
            );
            placements.push(score.placement as usize);
        }

        let model_input = player_ratings
            .iter()
            .map(|r| {
                vec![Rating {
                    mu: r.rating,
                    sigma: r.volatility
                }]
            })
            .collect_vec();

        let model_result = self.model.rate(model_input, placements);

        // At this point, the model_result output indices are aligned to
        // our vectors above.
        let mut map = HashMap::new();
        for (i, r) in player_ratings.iter().enumerate() {
            map.insert(r.player_id, model_result[i].first().unwrap().clone());
        }

        map
    }

    /// Performs method A calculation, combining a vector of ratings for a player
    /// into an unweighted rating value
    fn calc_a(&self, rating_map: HashMap<i32, Vec<Rating>>, match_: &Match) -> HashMap<i32, Rating> {
        let total_game_count = match_.games.len();
        let mut result_map: HashMap<i32, Rating> = HashMap::new();
        for (k, v) in rating_map {
            let current_player_rating = self.rating_tracker.get_rating(k, match_.ruleset).unwrap();
            result_map.insert(
                k,
                Self::calc_rating_a(
                    &v,
                    current_player_rating.rating,
                    current_player_rating.volatility,
                    total_game_count
                )
            );
        }

        result_map
    }

    /// Performs method B calculation, combining a vector of ratings for a player
    /// into an unweighted rating value
    fn calc_b(&self, rating_map: HashMap<i32, Vec<Rating>>, match_: &Match) -> HashMap<i32, Rating> {
        let total_game_count = match_.games.len();
        let mut result_map: HashMap<i32, Rating> = HashMap::new();
        for (k, v) in rating_map {
            result_map.insert(k, Self::calc_rating_b(&v, total_game_count));
        }

        result_map
    }

    /// Calculates the weighted rating for all players present in a and b
    fn calc_weighted_rating(&self, map_a: &HashMap<i32, Rating>, map_b: &HashMap<i32, Rating>) -> HashMap<i32, Rating> {
        let mut final_map: HashMap<i32, Rating> = HashMap::new();
        for k in map_a.keys() {
            if !map_b.contains_key(k) {
                panic!("Expected key {:?} to be present in both maps", k);
            }

            let result_a = map_a.get(k).unwrap();
            let result_b = map_b.get(k).unwrap();

            let rating_final = WEIGHT_A * result_a.mu + WEIGHT_B * result_b.mu;
            let volatility_final = (WEIGHT_A * result_a.sigma.powf(2.0) + WEIGHT_B * result_b.sigma.powf(2.0)).sqrt();

            final_map.insert(
                *k,
                Rating {
                    mu: rating_final,
                    sigma: volatility_final
                }
            );
        }

        final_map
    }

    fn calc_rating_a(
        ratings: &[Rating],
        current_rating: f64,
        current_volatility: f64,
        total_game_count: usize
    ) -> Rating {
        let rating_sum: f64 = ratings.iter().map(|f| f.mu).sum();

        let count_games_unplayed = total_game_count as f64 - ratings.len() as f64;
        let adjusted_rating_sum = rating_sum + count_games_unplayed * current_rating;
        let rating_a = adjusted_rating_sum / total_game_count as f64;

        let volatility_sum: f64 = ratings.iter().map(|f| f.sigma.powf(2.0)).sum();
        let adjusted_volatility_sum: f64 = volatility_sum + count_games_unplayed * current_volatility.powf(2.0);
        let volatility_a = (adjusted_volatility_sum / total_game_count as f64).sqrt();

        Rating {
            mu: rating_a,
            sigma: volatility_a
        }
    }

    fn calc_rating_b(ratings: &[Rating], total_game_count: usize) -> Rating {
        let rating_sum: f64 = ratings.iter().map(|f| f.mu).sum();
        let rating_b = rating_sum / total_game_count as f64;

        let volatility_sum: f64 = ratings.iter().map(|f| f.sigma.powf(2.0)).sum();
        let volatility_b = (volatility_sum / total_game_count as f64).sqrt();

        Rating {
            mu: rating_b,
            sigma: volatility_b
        }
    }

    /// Applies decay to all players who participated in this match.
    fn apply_decay(&mut self, match_: &Match) {
        let player_ids: Vec<i32> = match_
            .games
            .iter()
            .flat_map(|g| g.scores.iter().map(|score| score.player_id).collect::<Vec<i32>>())
            .collect();

        for p_id in player_ids {
            self.decay_tracker
                .decay(&mut self.rating_tracker, p_id, match_.ruleset, match_.start_time);
        }
    }

    /// Updates the RatingTracker with the results of the rating calculation
    fn apply_results(&mut self, match_: &Match, rating_calc_result: &HashMap<i32, Rating>) {
        for (k, v) in rating_calc_result {
            // Get their current rating
            let mut player_rating = self.rating_tracker.get_rating(*k, match_.ruleset).unwrap().clone();

            // Create the adjustment
            let adjustment = RatingAdjustment {
                player_id: *k,
                match_id: Some(match_.id),
                rating_before: player_rating.rating,
                rating_after: v.mu,
                volatility_before: player_rating.volatility,
                volatility_after: v.sigma,
                timestamp: match_.start_time,
                adjustment_type: RatingAdjustmentType::Match
            };

            player_rating.adjustments.push(adjustment);

            // Update the player_rating values
            player_rating.rating = v.mu;
            player_rating.volatility = v.sigma;

            // Save
            self.rating_tracker.insert_or_update(&[player_rating])
        }
    }

    /// Applies a scaled performance penalty to negative changes in rating.
    fn performance_scaled_rating(
        current_rating: f64,
        rating_diff: f64,
        performance_frequency: f64,
        scaling: f64
    ) -> f64 {
        if rating_diff >= 0.0 {
            return current_rating;
        }

        // Rating differential is used with a scaling factor
        // to determine final rating change
        current_rating - (scaling * (rating_diff.abs() * performance_frequency))
    }
}

#[cfg(test)]
mod tests {
    pub use crate::utils::test_utils::*;
    use crate::{
        database::db_structs::PlayerRating,
        model::{otr_model::OtrModel, structures::ruleset::Ruleset::Osu}
    };
    use approx::assert_abs_diff_eq;
    use chrono::Utc;

    #[test]
    fn test_rate() {
        // Add 3 players to model
        let player_ratings = vec![
            generate_player_rating(1, Osu, 1000.0, 100.0, 1),
            generate_player_rating(2, Osu, 1000.0, 100.0, 1),
            generate_player_rating(3, Osu, 1000.0, 100.0, 1),
        ];

        let countries = generate_country_mapping_player_ratings(player_ratings.as_slice(), "US");

        let model = OtrModel::new(player_ratings.as_slice(), &countries);

        let placements = vec![
            generate_placement(1, 2),
            generate_placement(2, 1),
            generate_placement(3, 3),
        ];

        let game = generate_game(1, &placements);

        let rating_result = model.rate(&game);

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
            generate_player_rating(1, Osu, 1000.0, 100.0, 1),
            generate_player_rating(2, Osu, 1000.0, 100.0, 1),
            generate_player_rating(3, Osu, 1000.0, 100.0, 1),
            generate_player_rating(4, Osu, 1000.0, 100.0, 1),
        ];

        let countries = generate_country_mapping_player_ratings(player_ratings.as_slice(), "US");
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

        let matches = vec![generate_match(1, Osu, &games, Utc::now().fixed_offset())];
        model.process(&matches);
        model.rating_tracker.sort();

        let rating_1 = model.rating_tracker.get_rating(1, Osu).unwrap();
        let rating_2: &PlayerRating = model.rating_tracker.get_rating(2, Osu).unwrap();
        let rating_3 = model.rating_tracker.get_rating(3, Osu).unwrap();
        let rating_4 = model.rating_tracker.get_rating(4, Osu).unwrap();

        let adjustments_1 = model
            .rating_tracker
            .get_rating_adjustments(1, Osu)
            .expect("Expected player 1 to have adjustments");
        let adjustments_2 = model
            .rating_tracker
            .get_rating_adjustments(2, Osu)
            .expect("Expected player 1 to have adjustments");
        let adjustments_3 = model
            .rating_tracker
            .get_rating_adjustments(3, Osu)
            .expect("Expected player 1 to have adjustments");
        let adjustments_4 = model
            .rating_tracker
            .get_rating_adjustments(4, Osu)
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

        // There are 2 adjustments for each player.
        // The first one is generated by generate_player_rating,
        // the second one is generated by the match processing.
        assert_eq!(adjustments_1.len(), 2);
        assert_eq!(adjustments_2.len(), 2);
        assert_eq!(adjustments_3.len(), 2);
        assert_eq!(adjustments_4.len(), 2);
    }

    #[test]
    fn test_negative_performance_scaling() {
        let rating: PlayerRating = generate_player_rating(1, Osu, 1000.0, 100.0, 1);
        let rating_diff = -100.0;
        let games_played = 1;
        let games_total = 10;
        let scaling = 1.0;
        let frequency = 1.0 / 10.0;

        // User should lose 10% of what they would have lost as they only participated in 1/10 of the maps.

        let scaled_rating = OtrModel::performance_scaled_rating(rating.rating, rating_diff, frequency, scaling);
        assert_abs_diff_eq!(scaled_rating, 990.0);
    }
}
