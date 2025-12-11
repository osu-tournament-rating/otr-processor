use super::{
    constants::{BETA, FALLBACK_RATING, KAPPA},
    decay::UnifiedDecaySystem
};
use crate::{
    database::db_structs::{Game, GameScore, Match, PlayerRating, RatingAdjustment},
    model::{
        constants::{
            ABSOLUTE_RATING_FLOOR, DEFAULT_VOLATILITY, GAME_CORRECTION_CONSTANT, STANDARD_MATCH_LENGTH, WEIGHT_A,
            WEIGHT_B
        },
        rating_tracker::RatingTracker,
        structures::{rating_adjustment_type::RatingAdjustmentType, ruleset::Ruleset}
    },
    utils::progress_utils::progress_span
};
use chrono::{Duration, Utc};
use itertools::Itertools;
use openskill::{
    model::{model::Model, plackett_luce::PlackettLuce},
    rating::{Rating, TeamRating}
};
use std::{collections::HashMap, ops::Sub, slice::from_ref};
use strum::IntoEnumIterator;
use tracing::{debug, error, info, warn};
use tracing_indicatif::span_ext::IndicatifSpanExt;

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
    pub rating_tracker: RatingTracker,
    /// Unified system for rating and volatility decay at Wednesday 12:00 UTC
    decay_system: UnifiedDecaySystem
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
            model: PlackettLuce::new(BETA, KAPPA, Self::gamma_override),
            decay_system: UnifiedDecaySystem::new()
        }
    }

    /// Custom volatility control function for the PlackettLuce model.
    ///
    /// This function determines how quickly player volatility changes based on performance.
    /// A higher gamma means volatility changes more slowly.
    fn gamma_override(_: f64, _k: f64, _: &TeamRating) -> f64 {
        2.0
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
        let span = progress_span(matches.len() as u64, "Processing matches");
        let _guard = span.enter();

        info!(match_count = matches.len(), "Starting to process matches");

        for m in matches.iter() {
            if m.games.is_empty() {
                error!(match_id = m.id, match_name = %m.name, "Skipping match - no games found");
                span.pb_inc(1);
                continue;
            }

            let empty_games: Vec<_> = m.games.iter().filter(|g| g.scores.is_empty()).map(|g| g.id).collect();

            if !empty_games.is_empty() {
                warn!(
                    match_id = m.id,
                    match_name = %m.name,
                    empty_game_count = empty_games.len(),
                    "Match has games with no scores"
                );
            }

            self.process_match(m);
            span.pb_inc(1);
        }

        info!("Finished processing matches, applying final decay pass");
        self.final_decay_pass();
        self.rating_tracker.sort();
        self.rating_tracker.get_all_ratings()
    }

    // Match Processing Methods

    /// Processes a single match, calculating and applying rating changes for all participants.
    ///
    /// # Processing Steps
    /// 1. Apply unified decay (rating + volatility) at Wednesday 12:00 UTC timestamps
    /// 2. Ensure any new players have initial ratings
    /// 3. Calculate ratings using both methods:
    ///    - Method A: Considers only played games
    ///    - Method B: Assumes last place for unplayed games
    /// 4. Combine results using weighted average
    /// 5. Update player ratings in the tracker
    fn process_match(&mut self, match_: &Match) {
        self.apply_unified_decay(match_.start_time);
        self.ensure_player_ratings(match_);

        let ratings_a = self.generate_ratings_a(match_);
        let ratings_b = self.generate_ratings_b(match_);

        let final_results = self.calc_new_ratings(ratings_a, ratings_b, match_);

        self.apply_results(match_, &final_results)
    }

    /// Generates ratings for each player based on their actual game performances.
    ///
    /// This method only considers games that players actually participated in,
    /// providing a "pure" performance rating for each game played.
    fn generate_ratings_a(&self, match_: &Match) -> HashMap<i32, Vec<Rating>> {
        let mut map: HashMap<i32, Vec<Rating>> = HashMap::new();
        for game in &match_.games {
            let game_rating_result = self.rate(game, &match_.ruleset);
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
    fn generate_ratings_b(&self, match_: &Match) -> HashMap<i32, Vec<Rating>> {
        let mut cloned_match = match_.clone();
        let participants = self.get_match_participants(&cloned_match);
        self.apply_tie_for_last_scores(&mut cloned_match, &participants);
        self.generate_ratings_a(&cloned_match)
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

    /// # Rate Game
    /// Calculates ratings for a single game using the PlackettLuce model.
    /// We pass in the Ruleset to avoid situations where the Game's ruleset
    /// is different from the tournament's Ruleset.
    ///
    /// ## Returns
    /// Returns a mapping of player IDs to their calculated ratings for this game.
    ///
    /// ## Note
    /// If a player doesn't have an existing rating for the game's ruleset,
    /// we use the fallback initial rating to allow them to participate.
    fn rate(&self, game: &Game, ruleset: &Ruleset) -> HashMap<i32, Rating> {
        let mut player_ratings = Vec::new();
        let mut placements = Vec::new();
        let mut player_ids = Vec::new();

        if game.scores.is_empty() {
            warn!(game_id = game.id, "Game has no scores, returning empty rating result");
            return HashMap::new();
        }

        for score in &game.scores {
            let rating = match self.rating_tracker.get_rating(score.player_id, *ruleset) {
                Some(r) => r.clone(),
                None => {
                    // Player is present in the score, but not in the rating tracker.
                    // This means they are in the system but have not yet been picked up
                    // by the DataWorkerService. Alternatively, they may not have any
                    // ruleset data, due to inactivity.
                    debug!(
                        game_id = game.id,
                        player_id = score.player_id,
                        ruleset = ?ruleset,
                        "Using fallback rating for player"
                    );
                    PlayerRating {
                        id: 0,
                        player_id: score.player_id,
                        ruleset: *ruleset,
                        rating: FALLBACK_RATING,
                        volatility: DEFAULT_VOLATILITY,
                        percentile: 0.0,
                        global_rank: 0,
                        country_rank: 0,
                        adjustments: vec![]
                    }
                }
            };

            player_ratings.push(rating);
            placements.push(score.placement as usize);
            player_ids.push(score.player_id);
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
        player_ids
            .iter()
            .enumerate()
            .map(|(i, &player_id)| (player_id, model_result[i][0].clone()))
            .collect()
    }

    // Rating Calculation Methods

    /// Calculate new ratings for all players in a given match, returning a map of each player to
    /// their new rating.
    fn calc_new_ratings(
        &self,
        rating_map_a: HashMap<i32, Vec<Rating>>,
        rating_map_b: HashMap<i32, Vec<Rating>>,
        match_: &Match
    ) -> HashMap<i32, Rating> {
        let total_games = match_.games.len();
        rating_map_a
            .into_iter()
            .map(|(player_id, ratings)| {
                let (current_rating, current_volatility) =
                    match self.rating_tracker.get_rating(player_id, match_.ruleset) {
                        Some(r) => (r.rating, r.volatility),
                        None => {
                            // Player was just created with fallback rating
                            (FALLBACK_RATING, DEFAULT_VOLATILITY)
                        }
                    };
                (
                    player_id,
                    Self::calc_weighted_rating(
                        &ratings,
                        &rating_map_b[&player_id],
                        current_rating,
                        current_volatility,
                        total_games
                    )
                )
            })
            .collect()
    }

    /// Calculate a new weighted, game-corrected rating for a player given ratings generated by
    /// methods A and B for a specific match.
    ///
    /// More information about the A and B ratings can be found here:
    /// https://docs.otr.stagec.xyz/Rating-Framework/Rating-Calculation/Rating-Calculation-Overview#ranking--rating-calculation
    fn calc_weighted_rating(
        ratings_a: &[Rating],
        ratings_b: &[Rating],
        current_rating: f64,
        current_volatility: f64,
        total_games: usize
    ) -> Rating {
        // calculate the total rating and volatility changes
        let rating_change_a: f64 = ratings_a.iter().map(|r| r.mu - current_rating).sum();
        let rating_change_b: f64 = ratings_b.iter().map(|r| r.mu - current_rating).sum();
        let volatility_change_a: f64 = ratings_a
            .iter()
            .map(|r| 1.0 - (r.sigma / current_volatility).powi(2))
            .sum();
        let volatility_change_b: f64 = ratings_b
            .iter()
            .map(|r| 1.0 - (r.sigma / current_volatility).powi(2))
            .sum();

        // calculate weighted rating changes based on A and B weights
        let weighted_rating_change: f64 =
            (WEIGHT_A * rating_change_a + WEIGHT_B * rating_change_b) / (total_games as f64);
        let weighted_volatility_change: f64 =
            (WEIGHT_A * volatility_change_a + WEIGHT_B * volatility_change_b) / (total_games as f64);

        // apply game-correction factor
        let game_correction_multiplier = (total_games as f64 / STANDARD_MATCH_LENGTH).powf(GAME_CORRECTION_CONSTANT);
        let corrected_rating_change: f64 = weighted_rating_change * game_correction_multiplier;
        let corrected_volatility_change: f64 = weighted_volatility_change * game_correction_multiplier;

        let new_rating: f64 = current_rating + corrected_rating_change;
        let new_volatility: f64 = current_volatility * (1.0 - corrected_volatility_change).max(KAPPA).sqrt();

        Rating {
            mu: new_rating.max(ABSOLUTE_RATING_FLOOR),
            sigma: new_volatility.min(DEFAULT_VOLATILITY)
        }
    }

    // Decay Handling Methods

    /// Applies the final decay pass to all players across all rulesets.
    ///
    /// This ensures that all player ratings are properly decayed to the current time,
    /// even if they haven't participated in recent matches.
    fn final_decay_pass(&mut self) {
        let current_time = Utc::now().fixed_offset();
        self.apply_unified_decay(current_time);
    }

    /// Applies unified decay (rating + volatility) to all players across all rulesets
    /// up to the given time.
    ///
    /// This applies decay adjustments at every Wednesday 12:00 UTC between the last
    /// processed time and the given `up_to` time:
    /// - For inactive players (>184 days): both rating and volatility decay
    /// - For active players: volatility decay only
    fn apply_unified_decay(&mut self, up_to: chrono::DateTime<chrono::FixedOffset>) {
        // Early exit if no Wednesdays to process (avoids expensive leaderboard cloning)
        if !self.decay_system.has_pending_wednesdays(up_to) {
            return;
        }

        let leaderboards: Vec<Vec<PlayerRating>> = Ruleset::iter()
            .map(|ruleset| self.rating_tracker.get_leaderboard(ruleset))
            .filter(|lb| !lb.is_empty())
            .collect();

        for leaderboard in leaderboards {
            let mut ratings = leaderboard;
            let count = self.decay_system.apply_decay(&mut ratings, up_to);

            if count > 0 {
                self.rating_tracker.insert_or_update(&ratings);
            }
        }
    }

    /// Ensures all match participants have ratings in the tracker.
    /// Creates fallback initial ratings for players not yet in the system.
    fn ensure_player_ratings(&mut self, match_: &Match) {
        let player_ids: Vec<i32> = self.get_match_participants(match_);

        for player_id in player_ids {
            if self.rating_tracker.get_rating(player_id, match_.ruleset).is_none() {
                // Player not present in the rating tracker.
                // Create a fallback initial rating for them and add to tracker.

                let adjustment = RatingAdjustment {
                    player_id,
                    ruleset: match_.ruleset,
                    match_id: None,
                    rating_before: 0.0,
                    rating_after: FALLBACK_RATING,
                    volatility_before: 0.0,
                    volatility_after: DEFAULT_VOLATILITY,
                    timestamp: match_.start_time.sub(Duration::seconds(1)),
                    adjustment_type: RatingAdjustmentType::Initial
                };

                let initial_rating = PlayerRating {
                    id: 0,
                    player_id,
                    ruleset: match_.ruleset,
                    rating: FALLBACK_RATING,
                    volatility: DEFAULT_VOLATILITY,
                    percentile: 0.0,
                    global_rank: 0,
                    country_rank: 0,
                    adjustments: vec![adjustment]
                };

                self.rating_tracker.insert_or_update(from_ref(&initial_rating));
            }
        }
    }

    /// Updates the RatingTracker with the results of the rating calculation
    fn apply_results(&mut self, match_: &Match, rating_calc_result: &HashMap<i32, Rating>) {
        for (k, v) in rating_calc_result {
            // Get their current rating or create a new one with fallback values
            let mut player_rating = match self.rating_tracker.get_rating(*k, match_.ruleset) {
                Some(r) => r.clone(),
                None => {
                    // Player was processed but doesn't exist in tracker yet
                    // Create initial rating with fallback values
                    PlayerRating {
                        id: 0,
                        player_id: *k,
                        ruleset: match_.ruleset,
                        rating: FALLBACK_RATING,
                        volatility: DEFAULT_VOLATILITY,
                        percentile: 0.0,
                        global_rank: 0,
                        country_rank: 0,
                        adjustments: vec![]
                    }
                }
            };

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
}

#[cfg(test)]
mod tests {
    pub use crate::utils::test_utils::*;
    use crate::{
        database::db_structs::{Game, PlayerPlacement, PlayerRating},
        model::{
            constants::{ABSOLUTE_RATING_FLOOR, DEFAULT_VOLATILITY},
            otr_model::OtrModel,
            structures::{rating_adjustment_type::RatingAdjustmentType, ruleset::Ruleset::*}
        }
    };
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

        let rating_result = model.rate(&game, &Osu);

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

            assert_ne!(
                adjustments[1].rating_before, adjustments[1].rating_after,
                "Player {}: Match adjustment should change the rating",
                player_id
            );
            assert_ne!(
                adjustments[1].volatility_before, adjustments[1].volatility_after,
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

    /// Ensures that players not present in the rating tracker get fallback ratings.
    /// This is important because this can happen when a player is restricted
    /// or otherwise can't be fetched from the osu! API, or when a player was
    /// recently added to the system and has not yet been processed by the DataWorkerService.
    #[test]
    fn test_player_not_in_rating_tracker_uses_fallback() {
        // Create a model with only 2 players in the rating tracker
        let player_ratings = vec![
            generate_player_rating(1, Osu, 1000.0, 100.0, 1, None, None),
            generate_player_rating(2, Osu, 1000.0, 100.0, 1, None, None),
        ];

        let countries = generate_country_mapping_player_ratings(&player_ratings, "US");
        let model = OtrModel::new(&player_ratings, &countries);

        // Create a game with 3 players, where player 3 is NOT in the rating tracker
        let placements = vec![
            generate_placement(1, 1), // Player 1 - 1st place
            generate_placement(2, 2), // Player 2 - 2nd place
            generate_placement(3, 3), // Player 3 - 3rd place (NOT in rating tracker)
        ];

        let game = generate_game(1, &placements);

        // Rate the game - should now process all 3 players
        let rating_result = model.rate(&game, &Osu);

        // Verify all 3 players are in the results
        assert_eq!(
            rating_result.len(),
            3,
            "Should process all players, using fallback rating for missing ones"
        );
        assert!(rating_result.contains_key(&1), "Player 1 should be processed");
        assert!(rating_result.contains_key(&2), "Player 2 should be processed");
        assert!(
            rating_result.contains_key(&3),
            "Player 3 should be processed with fallback rating"
        );

        // Verify the ratings for processed players are reasonable
        let result_1 = rating_result.get(&1).unwrap();
        let result_2 = rating_result.get(&2).unwrap();
        let result_3 = rating_result.get(&3).unwrap();

        // Player 1 (1st place) should have higher rating than Player 2 (2nd place)
        assert!(
            result_1.mu > result_2.mu,
            "Player 1 should have higher rating than Player 2"
        );

        // Player 2 (2nd place) should have higher rating than Player 3 (3rd place)
        assert!(
            result_2.mu > result_3.mu,
            "Player 2 should have higher rating than Player 3"
        );
    }

    /// Tests that players with ratings in different rulesets than the match ruleset
    /// are handled correctly (they should use fallback ratings for the new ruleset).
    #[test]
    fn test_player_with_different_ruleset_rating_uses_fallback() {
        // Create players with ratings in different rulesets
        let player_ratings = vec![
            generate_player_rating(1, Osu, 1200.0, 100.0, 1, None, None), // Has Osu rating
            generate_player_rating(2, Osu, 1000.0, 100.0, 1, None, None), // Has Osu rating
            generate_player_rating(3, Taiko, 1200.0, 100.0, 1, None, None), // Has Taiko rating only
            generate_player_rating(4, Catch, 1300.0, 100.0, 1, None, None), // Has Catch rating only
        ];

        let countries = generate_country_mapping_player_ratings(&player_ratings, "US");
        let model = OtrModel::new(&player_ratings, &countries);

        // Create an Osu game with all 4 players
        let placements = vec![
            generate_placement(1, 1), // Player 1 - 1st place (has Osu rating)
            generate_placement(2, 2), // Player 2 - 2nd place (has Osu rating)
            generate_placement(3, 3), // Player 3 - 3rd place (has Taiko rating only)
            generate_placement(4, 4), // Player 4 - 4th place (has Catch rating only)
        ];

        let game = generate_game(1, &placements);

        // Rate the game for Osu ruleset - should now process all 4 players
        let rating_result = model.rate(&game, &Osu);

        // Verify all players are processed
        assert_eq!(
            rating_result.len(),
            4,
            "Should process all players, using fallback for those without Osu ratings"
        );
        assert!(
            rating_result.contains_key(&1),
            "Player 1 (Osu rating) should be processed"
        );
        assert!(
            rating_result.contains_key(&2),
            "Player 2 (Osu rating) should be processed"
        );
        assert!(
            rating_result.contains_key(&3),
            "Player 3 (Taiko rating only) should be processed with fallback Osu rating"
        );
        assert!(
            rating_result.contains_key(&4),
            "Player 4 (Catch rating only) should be processed with fallback Osu rating"
        );

        // Verify that players with different ruleset ratings still exist in the tracker for their respective rulesets
        assert!(
            model.rating_tracker.get_rating(3, Taiko).is_some(),
            "Player 3 should have Taiko rating"
        );
        assert!(
            model.rating_tracker.get_rating(4, Catch).is_some(),
            "Player 4 should have Catch rating"
        );
        assert!(
            model.rating_tracker.get_rating(3, Osu).is_none(),
            "Player 3 should NOT have Osu rating in tracker initially"
        );
        assert!(
            model.rating_tracker.get_rating(4, Osu).is_none(),
            "Player 4 should NOT have Osu rating in tracker initially"
        );
    }

    /// Tests that match processing works correctly when some players have ratings
    /// for different rulesets than the match ruleset.
    #[test]
    fn test_match_processing_with_mixed_ruleset_players() {
        // Create players with ratings in different rulesets
        let player_ratings = vec![
            generate_player_rating(1, Osu, 1000.0, 100.0, 1, None, None), // Has Osu rating
            generate_player_rating(2, Osu, 1100.0, 100.0, 1, None, None), // Has Osu rating
            generate_player_rating(3, Taiko, 1200.0, 100.0, 1, None, None), // Has Taiko rating only
        ];

        let countries = generate_country_mapping_player_ratings(&player_ratings, "US");
        let mut model = OtrModel::new(&player_ratings, &countries);

        // Create an Osu match with games that include the Taiko-only player
        let placements_game1 = vec![
            generate_placement(1, 1), // Player 1 - 1st place (has Osu rating)
            generate_placement(2, 2), // Player 2 - 2nd place (has Osu rating)
            generate_placement(3, 3), // Player 3 - 3rd place (has Taiko rating only)
        ];

        let placements_game2 = vec![
            generate_placement(2, 1), // Player 2 - 1st place (has Osu rating)
            generate_placement(1, 2), // Player 1 - 2nd place (has Osu rating)
            generate_placement(3, 3), // Player 3 - 3rd place (has Taiko rating only)
        ];

        let games = vec![generate_game(1, &placements_game1), generate_game(2, &placements_game2)];

        let matches = vec![generate_match(1, Osu, &games, Utc::now().fixed_offset())];

        // Store initial ratings for comparison
        let initial_rating_1 = model.rating_tracker.get_rating(1, Osu).unwrap().rating;
        let initial_rating_2 = model.rating_tracker.get_rating(2, Osu).unwrap().rating;
        let initial_rating_3_taiko = model.rating_tracker.get_rating(3, Taiko).unwrap().rating;

        // Process the match
        model.process(&matches);

        // Verify that Osu players have updated ratings
        let final_rating_1 = model.rating_tracker.get_rating(1, Osu).unwrap();
        let final_rating_2 = model.rating_tracker.get_rating(2, Osu).unwrap();
        let final_rating_3_taiko = model.rating_tracker.get_rating(3, Taiko).unwrap();

        // Osu players should have changed ratings
        assert_ne!(
            final_rating_1.rating, initial_rating_1,
            "Player 1 Osu rating should have changed"
        );
        assert_ne!(
            final_rating_2.rating, initial_rating_2,
            "Player 2 Osu rating should have changed"
        );

        // Taiko player's rating should remain unchanged since they weren't processed in the Osu match
        assert_eq!(
            final_rating_3_taiko.rating, initial_rating_3_taiko,
            "Player 3 Taiko rating should remain unchanged"
        );

        // Player 3 should now have an Osu rating (created with fallback)
        assert!(
            model.rating_tracker.get_rating(3, Osu).is_some(),
            "Player 3 should have Osu rating created with fallback"
        );

        // Verify that Osu players have match adjustments
        let adjustments_1 = model.rating_tracker.get_rating_adjustments(1, Osu).unwrap();
        let adjustments_2 = model.rating_tracker.get_rating_adjustments(2, Osu).unwrap();
        let adjustments_3_taiko = model.rating_tracker.get_rating_adjustments(3, Taiko).unwrap();
        let adjustments_3_osu = model.rating_tracker.get_rating_adjustments(3, Osu).unwrap();

        assert_eq!(
            adjustments_1.len(),
            2,
            "Player 1 should have Initial + Match adjustments for Osu"
        );
        assert_eq!(
            adjustments_2.len(),
            2,
            "Player 2 should have Initial + Match adjustments for Osu"
        );
        assert_eq!(
            adjustments_3_taiko.len(),
            1,
            "Player 3 should only have Initial adjustment for Taiko"
        );
        assert_eq!(
            adjustments_3_osu.len(),
            2,
            "Player 3 should have Initial (fallback) + Match adjustments for Osu"
        );

        // Verify the last adjustment types
        assert_eq!(
            adjustments_1.last().unwrap().adjustment_type,
            RatingAdjustmentType::Match
        );
        assert_eq!(
            adjustments_2.last().unwrap().adjustment_type,
            RatingAdjustmentType::Match
        );
        assert_eq!(
            adjustments_3_taiko.last().unwrap().adjustment_type,
            RatingAdjustmentType::Initial
        );
        assert_eq!(
            adjustments_3_osu.last().unwrap().adjustment_type,
            RatingAdjustmentType::Match
        );
    }

    /// Test that match processing handles players without ruleset data correctly
    /// by creating fallback ratings and tracking adjustments properly.
    #[test]
    fn test_match_processing_with_no_ruleset_data() {
        use crate::model::constants::FALLBACK_RATING;

        // Create a model with only one player in the rating tracker
        let player_ratings = vec![generate_player_rating(1, Osu, 1200.0, 100.0, 1, None, None)];

        let countries = generate_country_mapping_player_ratings(&player_ratings, "US");
        let mut model = OtrModel::new(&player_ratings, &countries);

        // Create matches with player 2 who has no initial rating
        let placements = vec![
            generate_placement(1, 1), // Player 1 - 1st place (has initial rating)
            generate_placement(2, 2), // Player 2 - 2nd place (no initial rating)
        ];

        let games = vec![generate_game(1, &placements), generate_game(2, &placements)];

        let matches = vec![generate_match(1, Osu, &games, Utc::now().fixed_offset())];

        // Process the matches
        model.process(&matches);

        // Player 1 should have their rating updated from initial
        let rating_1 = model.rating_tracker.get_rating(1, Osu).unwrap();
        assert!(rating_1.rating > 1200.0, "Player 1 should win and gain rating");

        // Player 2 should now exist with a rating based on fallback
        let rating_2 = model.rating_tracker.get_rating(2, Osu).unwrap();
        assert!(
            rating_2.rating < FALLBACK_RATING,
            "Player 2 should lose and drop below fallback rating"
        );

        // Check adjustments for player 2
        let adjustments_2 = model.rating_tracker.get_rating_adjustments(2, Osu).unwrap();
        assert_eq!(
            adjustments_2.len(),
            2,
            "Player 2 should have Initial + Match adjustments"
        );

        // First adjustment should be Initial with fallback rating
        assert_eq!(adjustments_2[0].adjustment_type, RatingAdjustmentType::Initial);
        assert_eq!(adjustments_2[0].rating_after, FALLBACK_RATING);

        // Second adjustment should be Match
        assert_eq!(adjustments_2[1].adjustment_type, RatingAdjustmentType::Match);
        assert_eq!(adjustments_2[1].rating_before, FALLBACK_RATING);
        assert!(
            adjustments_2[1].rating_after < FALLBACK_RATING,
            "Should lose rating from fallback"
        );
    }
}
