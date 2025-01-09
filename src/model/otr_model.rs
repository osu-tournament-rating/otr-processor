use crate::{
    database::db_structs::{Game, GameScore, Match, PlayerRating, RatingAdjustment},
    model::{
        constants::{ABSOLUTE_RATING_FLOOR, DEFAULT_VOLATILITY, WEIGHT_A, WEIGHT_B},
        rating_tracker::RatingTracker,
        structures::{rating_adjustment_type::RatingAdjustmentType, ruleset::Ruleset}
    },
    utils::progress_utils::progress_bar
};
use chrono::{DateTime, FixedOffset, Utc};
use itertools::Itertools;
use openskill::{
    constant::*,
    model::{model::Model, plackett_luce::PlackettLuce},
    rating::{Rating, TeamRating}
};
use std::collections::HashMap;
use strum::IntoEnumIterator;

use super::decay::DecaySystem;

/// o!TR Model Implementation
///
/// This file handles the core rating calculations for the o!TR system.
/// It uses a modified PlackettLuce rating model combined with a custom decay system to provide
/// accurate tournament performance ratings.
///  
/// # Rating Process
/// 1. **Match Processing**: Each match is processed chronologically
///    - Players' ratings are decayed if inactive
///    - Individual game performances are calculated
///    - Final match rating changes are computed using two methods (A and B)
///    - Ratings are updated in the tracker
///
/// 2. **Game Ratings**: Each game in a match contributes to the final rating
///    - Method A: Accounts for games played vs not played
///    - Method B: Assumes last place for missed games
///    - Final rating is a weighted combination of both methods
///
/// 3. **Decay System**: Inactive players' ratings decay over time
///    - Applied before processing new matches
///    - Applied as a final pass to ensure current ratings
pub struct OtrModel {
    /// The underlying PlackettLuce rating model
    pub model: PlackettLuce,
    /// Tracks and maintains all player ratings
    pub rating_tracker: RatingTracker
}

impl OtrModel {
    /// Creates a new o!TR model instance with initial player ratings and country mappings.
    ///
    /// The model is initialized with:
    /// - A custom gamma function for volatility control
    /// - Default beta and kappa values from OpenSkill
    /// - Initial player ratings loaded into the tracker
    pub fn new(initial_player_ratings: &[PlayerRating], country_mapping: &HashMap<i32, String>) -> OtrModel {
        let mut tracker = RatingTracker::new();
        tracker.set_country_mapping(country_mapping.clone());
        tracker.insert_or_update(initial_player_ratings);

        OtrModel {
            rating_tracker: tracker,
            model: PlackettLuce::new(DEFAULT_BETA, KAPPA, Self::gamma_override)
        }
    }

    /// Custom volatility control function for the PlackettLuce model.
    ///
    /// This function determines how quickly player volatility changes based on performance.
    /// A higher gamma means volatility changes more slowly.
    fn gamma_override(_: f64, k: f64, _: &TeamRating) -> f64 {
        1.0 / k
    }

    /// Processes a batch of matches chronologically, updating player ratings.
    ///
    /// # Processing Steps
    /// 1. Process each match individually, updating ratings
    /// 2. Apply final decay pass to all players
    /// 3. Sort ratings and return the complete rating list
    ///
    /// # Returns
    /// Returns a vector of all PlayerRatings after processing
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

        self.final_decay_pass();
        self.rating_tracker.sort();
        self.rating_tracker.get_all_ratings()
    }

    // Match Processing Methods

    /// Processes a single match, calculating and applying rating changes for all participants.
    ///
    /// # Processing Steps
    /// 1. Apply decay to all participating players
    /// 2. Calculate ratings using both methods:
    ///    - Method A: Considers only played games
    ///    - Method B: Assumes last place for unplayed games
    /// 3. Combine results using weighted average
    /// 4. Update player ratings in the tracker
    fn process_match(&mut self, match_: &Match) {
        self.apply_decay(match_);

        let ratings_standard = self.generate_ratings(match_);
        let ratings_with_penalties = self.generate_penalized_ratings(match_);

        let calc_standard = self.calc_a(ratings_standard, match_);
        let calc_penalized = self.calc_b(ratings_with_penalties, match_);
        let final_results = self.calc_weighted_rating(&calc_standard, &calc_penalized);

        self.apply_results(match_, &final_results)
    }

    /// Generates ratings for each player based on their actual game performances.
    ///
    /// This method only considers games that players actually participated in,
    /// providing a "pure" performance rating for each game played.
    fn generate_ratings(&self, match_: &Match) -> HashMap<i32, Vec<Rating>> {
        let mut map: HashMap<i32, Vec<Rating>> = HashMap::new();
        for game in &match_.games {
            let game_rating_result = self.rate(game);
            for (k, v) in game_rating_result {
                map.entry(k).or_default().push(v);
            }
        }
        map
    }

    /// Generates ratings with penalties for missed games.
    ///
    /// This method assumes players who missed games would have placed last,
    /// providing a "worst-case" rating scenario for players who don't participate
    /// in all games of a match.
    fn generate_penalized_ratings(&self, match_: &Match) -> HashMap<i32, Vec<Rating>> {
        let mut cloned_match = match_.clone();
        let participants = self.get_match_participants(&mut cloned_match);
        self.apply_tie_for_last_scores(&mut cloned_match, &participants);
        self.generate_ratings(&cloned_match)
    }

    /// Gets a unique list of all players who participated in any game of the match.
    fn get_match_participants(&self, match_: &Match) -> Vec<i32> {
        match_
            .games
            .iter()
            .flat_map(|g| g.scores.iter().map(|s| s.player_id))
            .unique()
            .collect()
    }

    /// Adds last-place scores for players who missed specific games.
    ///
    /// For each game, players who didn't participate are given a score with:
    /// - Placement one worse than the last-place finisher
    /// - Score of 0
    fn apply_tie_for_last_scores(&self, match_: &mut Match, ids: &[i32]) {
        for game in &mut match_.games {
            let worst_placement = game.scores.iter().map(|f| f.placement).max().unwrap();
            let tie_for_last_placement = worst_placement + 1;

            let missing_players = ids
                .iter()
                .filter(|&id| !game.scores.iter().any(|s| s.player_id == *id))
                .copied()
                .collect::<Vec<i32>>();

            for player_id in missing_players {
                game.scores.push(GameScore {
                    id: 0,
                    player_id,
                    game_id: game.id,
                    score: 0,
                    placement: tie_for_last_placement
                });
            }
        }
    }

    /// Calculates ratings for a single game using the PlackettLuce model.
    ///
    /// # Returns
    /// Returns a mapping of player IDs to their calculated ratings for this game.
    ///
    /// # Panics
    /// Panics if a player doesn't have an existing rating for the game's ruleset.
    fn rate(&self, game: &Game) -> HashMap<i32, Rating> {
        let mut player_ratings = Vec::new();
        let mut placements = Vec::new();

        // Build input vectors maintaining index correlation
        for score in &game.scores {
            let rating = self
                .rating_tracker
                .get_rating(score.player_id, game.ruleset)
                .unwrap_or_else(|| {
                    panic!(
                        "Player {}: No rating found for ruleset {:?}",
                        score.player_id, game.ruleset
                    )
                });

            player_ratings.push(rating);
            placements.push(score.placement as usize);
        }

        // Convert to OpenSkill format
        let model_input = player_ratings
            .iter()
            .map(|r| {
                vec![Rating {
                    mu: r.rating,
                    sigma: r.volatility
                }]
            })
            .collect_vec();

        // Calculate new ratings
        let model_result = self.model.rate(model_input, placements);

        // Map results back to player IDs
        player_ratings
            .iter()
            .enumerate()
            .map(|(i, r)| (r.player_id, model_result[i][0].clone()))
            .collect()
    }

    // Rating Calculation Methods

    /// Calculates the standard rating (Method A) for all players in a match.
    ///
    /// Method A handles missing games by using the player's current rating,
    /// providing a more conservative rating change for partially played matches.
    ///
    /// # Arguments
    /// * `rating_map` - Map of player IDs to their per-game ratings
    /// * `match_` - The match being processed
    fn calc_a(&self, rating_map: HashMap<i32, Vec<Rating>>, match_: &Match) -> HashMap<i32, Rating> {
        let total_games = match_.games.len();
        rating_map
            .into_iter()
            .map(|(player_id, ratings)| {
                let current = self
                    .rating_tracker
                    .get_rating(player_id, match_.ruleset)
                    .expect("Player rating should exist");

                (
                    player_id,
                    Self::calc_rating_a(&ratings, current.rating, current.volatility, total_games)
                )
            })
            .collect()
    }

    /// Calculates the penalized rating (Method B) for all players in a match.
    ///
    /// Method B uses the actual ratings calculated with missed games counted as losses,
    /// providing a more punitive rating change for partially played matches.
    fn calc_b(&self, rating_map: HashMap<i32, Vec<Rating>>, match_: &Match) -> HashMap<i32, Rating> {
        let total_games = match_.games.len();
        rating_map
            .into_iter()
            .map(|(player_id, ratings)| (player_id, Self::calc_rating_b(&ratings, total_games)))
            .collect()
    }

    /// Combines Method A and B ratings using weighted average.
    ///
    /// The final rating is calculated as:
    /// - Rating = (WEIGHT_A × Method A) + (WEIGHT_B × Method B)
    /// - Volatility = √(WEIGHT_A × σ²_A + WEIGHT_B × σ²_B)
    ///
    /// Ensures the final rating stays within system bounds:
    /// - Rating ≥ ABSOLUTE_RATING_FLOOR
    /// - Volatility ≤ DEFAULT_VOLATILITY
    fn calc_weighted_rating(&self, map_a: &HashMap<i32, Rating>, map_b: &HashMap<i32, Rating>) -> HashMap<i32, Rating> {
        map_a
            .keys()
            .map(|&player_id| {
                let result_a = map_a.get(&player_id).expect("Player should have Method A rating");
                let result_b = map_b.get(&player_id).expect("Player should have Method B rating");

                let rating = WEIGHT_A * result_a.mu + WEIGHT_B * result_b.mu;
                let volatility = (WEIGHT_A * result_a.sigma.powf(2.0) + WEIGHT_B * result_b.sigma.powf(2.0)).sqrt();

                (
                    player_id,
                    Rating {
                        mu: rating.max(ABSOLUTE_RATING_FLOOR),
                        sigma: volatility.min(DEFAULT_VOLATILITY)
                    }
                )
            })
            .collect()
    }

    /// Calculates Method A rating for a player.
    fn calc_rating_a(ratings: &[Rating], current_rating: f64, current_volatility: f64, total_games: usize) -> Rating {
        let played_games = ratings.len();
        let unplayed_games = total_games - played_games;

        let rating_sum: f64 = ratings.iter().map(|r| r.mu).sum();
        let rating = (rating_sum + current_rating * unplayed_games as f64) / total_games as f64;

        let volatility_sum: f64 = ratings.iter().map(|r| r.sigma.powf(2.0)).sum();
        let volatility =
            ((volatility_sum + current_volatility.powf(2.0) * unplayed_games as f64) / total_games as f64).sqrt();

        Rating {
            mu: rating,
            sigma: volatility
        }
    }

    /// Calculates Method B rating for a player.
    ///
    /// Note: Missing games are pre-calculated as losses in `generate_penalized_ratings`
    fn calc_rating_b(ratings: &[Rating], total_games: usize) -> Rating {
        let rating = ratings.iter().map(|r| r.mu).sum::<f64>() / total_games as f64;
        let volatility = (ratings.iter().map(|r| r.sigma.powf(2.0)).sum::<f64>() / total_games as f64).sqrt();

        Rating {
            mu: rating,
            sigma: volatility
        }
    }

    // Decay Handling Methods

    /// Applies the final decay pass to all players across all rulesets.
    ///
    /// This ensures that all player ratings are properly decayed to the current time,
    /// even if they haven't participated in recent matches.
    fn final_decay_pass(&mut self) {
        let current_time = Utc::now().fixed_offset();
        let decay_system = DecaySystem::new(current_time);

        let leaderboards: Vec<Vec<PlayerRating>> = Ruleset::iter()
            .map(|ruleset| self.rating_tracker.get_leaderboard(ruleset))
            .filter(|lb| !lb.is_empty())
            .collect();

        for leaderboard in leaderboards {
            let ruleset = leaderboard
                .first()
                .map(|r| r.ruleset)
                .expect("Leaderboard should not be empty");

            let progress = progress_bar(leaderboard.len() as u64, format!("Applying decay: [{:?}]", ruleset));

            let mut updated_ratings = Vec::new();
            for rating in leaderboard {
                let mut current = rating.clone();
                if let Ok(Some(updated)) = decay_system.decay(&mut current) {
                    updated_ratings.push(updated.clone());
                }

                if let Some(pb) = &progress {
                    pb.inc(1);
                }
            }

            if let Some(pb) = &progress {
                pb.finish();
            }

            if !updated_ratings.is_empty() {
                self.rating_tracker.insert_or_update(&updated_ratings);
            }
        }
    }

    /// Applies decay to all players in a match before processing their results.
    fn apply_decay(&mut self, match_: &Match) {
        let decay_system = DecaySystem::new(match_.start_time);
        let player_ids: Vec<i32> = self.get_match_participants(match_);

        for player_id in player_ids {
            if let Some(rating) = self.rating_tracker.get_rating(player_id, match_.ruleset) {
                let mut current = rating.clone();
                if let Ok(Some(updated)) = decay_system.decay(&mut current) {
                    self.rating_tracker.insert_or_update(&[updated.clone()]);
                }
            } else {
                log::warn!(
                    "No rating found for player [Id: {} | Ruleset: {:?}]",
                    player_id,
                    match_.ruleset
                );
            }
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
                ruleset: player_rating.ruleset,
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
        database::db_structs::{Game, GameScore, PlayerPlacement, PlayerRating},
        model::{
            constants::{ABSOLUTE_RATING_FLOOR, DEFAULT_VOLATILITY},
            otr_model::OtrModel,
            structures::{rating_adjustment_type::RatingAdjustmentType, ruleset::Ruleset::Osu}
        }
    };
    use approx::assert_abs_diff_eq;
    use chrono::Utc;

    #[test]
    fn test_rate() {
        // Add 3 players to model
        let player_ratings = vec![
            generate_player_rating(1, Osu, 1000.0, 100.0, 1, None, None),
            generate_player_rating(2, Osu, 1000.0, 100.0, 1, None, None),
            generate_player_rating(3, Osu, 1000.0, 100.0, 1, None, None),
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
        // Add 4 players to model - but now only with Initial adjustments
        let player_ratings = vec![
            generate_player_rating(1, Osu, 1000.0, 100.0, 1, None, None), // Changed from 2 to 1
            generate_player_rating(2, Osu, 1000.0, 100.0, 1, None, None),
            generate_player_rating(3, Osu, 1000.0, 100.0, 1, None, None),
            generate_player_rating(4, Osu, 1000.0, 100.0, 1, None, None),
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

        // Get final ratings and adjustments
        let rating_1 = model.rating_tracker.get_rating(1, Osu).unwrap();
        let rating_2 = model.rating_tracker.get_rating(2, Osu).unwrap();
        let rating_3 = model.rating_tracker.get_rating(3, Osu).unwrap();
        let rating_4 = model.rating_tracker.get_rating(4, Osu).unwrap();

        // Verify adjustments
        for (player_id, rating) in [(1, rating_1), (2, rating_2), (3, rating_3), (4, rating_4)] {
            let adjustments = model
                .rating_tracker
                .get_rating_adjustments(player_id, Osu)
                .expect("Expected player to have adjustments");

            // Each player should have exactly 2 adjustments:
            // 1. Initial adjustment from generate_player_rating
            // 2. Match adjustment from match processing
            assert_eq!(
                adjustments.len(),
                2,
                "Player {} should have exactly 2 adjustments (Initial + Match)",
                player_id
            );

            // Verify adjustment types
            assert_eq!(
                adjustments[0].adjustment_type,
                RatingAdjustmentType::Initial,
                "First adjustment should be Initial"
            );

            assert_eq!(
                adjustments[1].adjustment_type,
                RatingAdjustmentType::Match,
                "Second adjustment should be Match"
            );

            assert_eq!(
                adjustments[1].rating_before, adjustments[0].rating_after,
                "Player {}: Match adjustment 'before' rating should equal Initial adjustment 'after' rating",
                player_id
            );

            assert!(
                adjustments[1].rating_before != adjustments[1].rating_after,
                "Player {}: Match adjustment should change the rating",
                player_id
            );

            assert!(
                adjustments[1].volatility_before != adjustments[1].volatility_after,
                "Player {}: Match adjustment should change the volatility",
                player_id
            );
        }

        // Verify rating order
        assert!(rating_4.rating > rating_3.rating);
        assert!(rating_3.rating > rating_2.rating);
        assert!(rating_2.rating > rating_1.rating);
        assert!(rating_1.rating < 1000.0);

        // Verify ranks
        assert_eq!(rating_4.global_rank, 1);
        assert_eq!(rating_3.global_rank, 2);
        assert_eq!(rating_2.global_rank, 3);
        assert_eq!(rating_1.global_rank, 4);

        assert_eq!(rating_4.country_rank, 1);
        assert_eq!(rating_3.country_rank, 2);
        assert_eq!(rating_2.country_rank, 3);
        assert_eq!(rating_1.country_rank, 4);
    }

    /// Tests that the performance scaling system correctly reduces rating changes
    /// based on participation frequency.
    #[test]
    fn test_performance_scaling_partial_participation() {
        // Setup test scenario: player participates in 1/10 maps
        let initial_rating = 1000.0;
        let rating_change = -100.0;
        let participation_frequency = 0.1; // 1 out of 10 maps
        let scaling_factor = 1.0;

        let scaled_rating =
            OtrModel::performance_scaled_rating(initial_rating, rating_change, participation_frequency, scaling_factor);

        // Player should lose 10% of the normal rating change
        // 1000 - (100 * 0.1) = 990
        assert_abs_diff_eq!(scaled_rating, 990.0, epsilon = 0.001);
    }

    /// Tests that positive rating changes are not affected by performance scaling.
    #[test]
    fn test_performance_scaling_positive_change() {
        let initial_rating = 1000.0;
        let rating_change = 100.0;
        let participation_frequency = 0.1;
        let scaling_factor = 1.0;

        let scaled_rating =
            OtrModel::performance_scaled_rating(initial_rating, rating_change, participation_frequency, scaling_factor);

        // Positive changes should not be scaled
        assert_abs_diff_eq!(scaled_rating, initial_rating, epsilon = 0.001);
    }

    #[test]
    fn test_initial_rating_not_generated_when_no_match_data() {
        let player_rating = generate_player_rating(1, Osu, 1000.0, 100.0, 1, None, None);
    }

    /// Tests that the rating system correctly handles matches with players
    /// starting at the rating floor and high volatility.
    #[test]
    fn test_rating_bounds_enforcement() {
        let time = Utc::now().fixed_offset();

        // Create 4 players at rating floor with high volatility
        let player_ratings: Vec<PlayerRating> = (1..=4)
            .map(|id| {
                generate_player_rating(
                    id,
                    Osu,
                    ABSOLUTE_RATING_FLOOR,
                    DEFAULT_VOLATILITY * 10.0,
                    1,
                    Some(time),
                    Some(time)
                )
            })
            .collect();

        let countries = generate_country_mapping_player_ratings(&player_ratings, "US");
        let mut model = OtrModel::new(&player_ratings, &countries);

        // Create a match where players maintain their position
        let placements: Vec<PlayerPlacement> = (1..=4).map(|id| generate_placement(id, id)).collect();

        let games: Vec<Game> = (1..=3).map(|id| generate_game(id, &placements)).collect();

        let matches = vec![generate_match(1, Osu, &games, time)];
        model.process(&matches);

        // Verify rating bounds are enforced
        for player_id in 1..=4 {
            let rating = model
                .rating_tracker
                .get_rating(player_id, Osu)
                .expect("Player rating should exist");

            assert!(
                rating.rating >= ABSOLUTE_RATING_FLOOR,
                "Player {} rating {} below floor {}",
                player_id,
                rating.rating,
                ABSOLUTE_RATING_FLOOR
            );
            assert!(
                rating.volatility <= DEFAULT_VOLATILITY,
                "Player {} volatility {} above maximum {}",
                player_id,
                rating.volatility,
                DEFAULT_VOLATILITY
            );
        }
    }
}
