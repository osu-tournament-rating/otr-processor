use std::collections::HashMap;
use itertools::Itertools;
use crate::{
    model::{
        constants::PERFORMANCE_SCALING_FACTOR,
        db_structs::{Game, Match, PlayerRating, RatingAdjustment},
        decay::DecayTracker,
        rating_tracker::RatingTracker,
        structures::{rating_adjustment_type::RatingAdjustmentType, ruleset::Ruleset}
    },
    utils::progress_utils::progress_bar
};
use openskill::{
    constant::*,
    model::{model::Model, plackett_luce::PlackettLuce},
    rating::{default_gamma, Rating}
};
use serde::__private::de::Content::F64;
use statrs::statistics::Statistics;

pub struct OtrModel {
    pub model: PlackettLuce,
    pub rating_tracker: RatingTracker,
    pub decay_tracker: DecayTracker
}

impl OtrModel {
    pub fn new(initial_player_ratings: &[PlayerRating], country_mapping: &HashMap<i32, String>) -> OtrModel {
        let mut tracker = RatingTracker::new();

        tracker.set_country_mapping(country_mapping.clone());
        tracker.insert_or_update(initial_player_ratings);

        OtrModel {
            rating_tracker: tracker,
            decay_tracker: DecayTracker,
            model: PlackettLuce::new(DEFAULT_BETA, KAPPA, default_gamma)
        }
    }

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
    fn process_match(&mut self, match_: &Match) {
        // Apply decay to all players
        self.apply_decay(match_);

        let new_ratings = Vec::new();

        self.rating_tracker.insert_or_update(new_ratings.as_slice())
    }

    /// Rating method a, per the docs https://docs.otr.stagec.xyz/rating-calculation.html#ranking-rating-calculation
    ///
    /// R_A = (R_1 + .. + R_k + (n-k) * R) / n
    /// V_A = sqrt((V_1^2 + .. + V_k^2 + (n-k) * V^2) / n)
    ///
    /// Returns a new Rating object (containing the mu and sigma)
    fn rating_result_a(&self, player_id: i32, match_: Match) -> Rating {
        let current_rating = self.rating_tracker.get_rating(player_id, match_.ruleset);

        let r = current_rating.unwrap().rating;
        
        // Identify the number of games played of the total
        let games_played: Vec<&Game> = match_.games.iter().filter(|f| f.scores.iter().any(|s| s.player_id == player_id)).collect();
        let k = games_played.len();
        let n = match_.games.len();

        // For each played game, identify the placement
        let mut ratings: Vec<Rating> = Vec::new();
        for game in games_played {
            let placement = game.scores.iter().find(|f| f.player_id == player_id).unwrap().placement;
            let (teams, placements) = self.form_rating_teams(game, placement);
            let result = self.model.rate(teams, placements);
            
            ratings.extend(result.into_iter().flatten());
        }

        let mu_collection = ratings.iter().map(|f| f.mu);
        let sigma_collection = ratings.iter().map(|f| f.sigma.powf(2.0));

        let r_a = mu_collection.sum::<f64>() + (n as f64 - k as f64) * r / n as f64;
        let v_a = (sigma_collection.sum::<f64>() + (n as f64 - k as f64) * current_rating.unwrap().volatility.powf(2.0) / n as f64).sqrt();
        
        Rating {
            mu: r_a,
            sigma: v_a,
        }
    }
    
    /// Form the vector of ratings to feed into the model
    /// Returns the input to the OpenSkill model
    fn form_rating_teams(&self, game: &Game, placement: i32) -> (Vec<Vec<Rating>>, Vec<usize>) {
        // Each score is it's own "team" according to the model (vector of ratings)
        let mut teams = Vec::new();
        let mut placements = Vec::new();
        for score in &game.scores {
            let player_rating = self.rating_tracker.get_rating(score.player_id, game.ruleset).expect("Expected player to have a rating");
            let rating = Rating {
                mu: player_rating.rating,
                sigma: player_rating.volatility,
            };
            
            teams.insert((placement - 1) as usize, vec![rating]);
            placements.push(placement as usize);
        }

        (teams, placements)
    }
    
    fn process_rating_result(
        &mut self,
        match_: &Match,
        unprocessed_performances: &mut HashMap<i32, Vec<Rating>>,
        player_id: &i32
    ) -> PlayerRating {
        let mut current_rating = self
            .rating_tracker
            .get_rating(*player_id, match_.ruleset)
            .unwrap()
            .clone();
        let performances = unprocessed_performances.get(player_id).unwrap_or_else(|| {
            panic!(
                "Expected player {} to have performances for this match {}!",
                player_id, match_.id
            )
        });

        if n_games == 0 {
            panic!("Expected at least one game in the match!");
        }

        let n_performances = performances.len() as i32;

        // A scaling factor based on game performance
        let performance_frequency = n_performances as f64 / n_games as f64;

        // Calculate differences from the baseline rating
        // This works because we only update the ratings in the leaderboard once per match
        // (these performances are from the game level)
        
        // This is known as rating_change_a in the docs: https://docs.otr.stagec.xyz/rating-calculation.html#ranking-rating-calculation
        let baseline_mu_change: f64 = performances.iter().map(|f| f.mu - current_rating.rating).mean();
        let rating_change_a = baseline_mu_change * 0.9;
        
        let baseline_volatility_change: f64 = performances.iter().map(|f| f.sigma - current_rating.volatility).mean();

        // Average the sigma changes
        
        let scaled_rating = (current_rating.rating + baseline_mu_change) * performance_frequency;
        let scaled_volatility: f64 = (current_rating.volatility + baseline_volatility_change) * performance_frequency;

        let rating_delta = scaled_rating - current_rating.rating;

        let performance_scaled_rating = Self::performance_scaled_rating(
            current_rating.rating,
            rating_delta,
            performance_frequency,
            PERFORMANCE_SCALING_FACTOR
        );

        if scaled_volatility == 0.0 {
            panic!(
                "Scaled volatility is 0.0 for player {} in match {}",
                player_id, match_.id
            );
        }

        let adjustment = RatingAdjustment {
            player_id: *player_id,
            match_id: Some(match_.id),
            rating_before: current_rating.rating,
            rating_after: performance_scaled_rating,
            volatility_before: current_rating.volatility,
            volatility_after: scaled_volatility,
            timestamp: match_.start_time,
            adjustment_type: RatingAdjustmentType::Match
        };

        current_rating.rating = scaled_rating;
        current_rating.volatility = scaled_volatility;
        current_rating.adjustments.push(adjustment);

        current_rating
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
    pub use crate::{
        model::{
            db_structs::PlayerRating,
            otr_model::OtrModel,
            structures::ruleset::{Ruleset, Ruleset::Osu}
        },
        utils::test_utils::*
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
        let rating_2 = model.rating_tracker.get_rating(2, Osu).unwrap();
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
