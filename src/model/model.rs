use statrs::statistics::Statistics;
use statrs::distribution::{ContinuousCDF, Normal};
use std::collections::{HashMap, HashSet};
use openskill::model::plackett_luce::PlackettLuce;
use openskill::rating::{default_gamma, Rating};
use crate::api::api_structs::{Game, Match, MatchRatingStats, Player, RatingAdjustment};
use crate::model::constants::RatingConstants;
use crate::model::structures::match_cost::MatchCost;
use crate::model::structures::mode::Mode;
use crate::model::structures::player_rating::PlayerRating;
use crate::model::structures::rating_calculation_result::RatingCalculationResult;
use crate::utils::progress_utils::progress_bar;

pub fn create_model() -> PlackettLuce {
    let constants = RatingConstants::default();
    PlackettLuce::new(constants.default_beta,
                      constants.default_kappa,
                      default_gamma)
}

// Rating generation

pub fn create_initial_ratings(matches: Vec<Match>, players: Vec<Player>) -> Vec<PlayerRating> {
    // The first step in the rating algorithm. Generate ratings from known ranks.
    let constants = RatingConstants::default();

    // A fast lookup used for understanding who has default ratings created at this time.
    let mut stored_lookup_log: HashSet<(i32, i32)> = HashSet::new();
    let mut ratings: Vec<PlayerRating> = Vec::new();
    let bar = progress_bar(matches.len() as u64);

    // Map the osu ids for fast lookup
    let mut player_hashmap: HashMap<i32, Player> = HashMap::new();

    for player in players {
        player_hashmap.entry(player.id).or_insert(player);
    }

    for m in matches {
        for game in m.games {
            let mode = game.play_mode;
            let enum_mode = match mode.try_into() {
                Ok(mode @ (Mode::Osu | Mode::Taiko | Mode::Catch | Mode::Mania)) => mode,
                _ => panic!("Expected one of [0, 1, 2, 3] to convert to mode enum. Found {} instead.", mode),
            };

            for score in game.match_scores {
                // Check if the player_id and enum_mode combination is already in created_ratings
                if stored_lookup_log.contains(&(score.player_id, enum_mode as i32)) {
                    // We've already initialized this player.
                    continue;
                }

                // Create ratings using the earliest known rank
                let player = player_hashmap.get(&score.player_id).expect("Player should be present in the hashmap.");
                let rank: Option<i32> = match enum_mode {
                    Mode::Osu => player.earliest_osu_global_rank.or(player.rank_standard),
                    Mode::Taiko => player.earliest_taiko_global_rank.or(player.rank_taiko),
                    Mode::Catch => player.earliest_catch_global_rank.or(player.rank_catch),
                    Mode::Mania => player.earliest_mania_global_rank.or(player.rank_mania)
                };

                let mu;
                let sigma;
                match rank {
                    Some(rank) => {
                        // Player has a valid identified rank (either the earliest known
                        // rank, or their current rank)
                        mu = mu_for_rank(rank);
                        sigma = constants.default_sigma;
                    },
                    None => {
                        // Player may be restricted / we cannot get hold of their rank info. Use default.
                        mu = constants.default_mu;
                        sigma = constants.default_sigma;
                    }
                }

                let rating = Rating::new(mu, sigma);
                let player_rating = PlayerRating {
                    player_id: score.player_id,
                    mode: enum_mode,
                    rating
                };
                ratings.push(player_rating);

                stored_lookup_log.insert((score.player_id, enum_mode as i32));
            }
        }

        bar.inc(1);
    }

    ratings
}

/// Calculates a vector of initial ratings based on match cost,
/// returns the new ratings
pub fn calc_ratings(initial_ratings: Vec<PlayerRating>, matches: Vec<Match>, model: PlackettLuce) -> RatingCalculationResult {
    // Key = (player_id, mode as i32)
    // Value = Associated PlayerRating (if available)
    let mut ratings_hash: HashMap<(i32, i32), PlayerRating> = HashMap::new();
    let rating_stats_hash: HashMap<i32, Vec<MatchRatingStats>> = HashMap::new();
    let rating_adjustments_hash: HashMap<i32, Vec<RatingAdjustment>> = HashMap::new();

    for r in initial_ratings {
        ratings_hash.insert((r.player_id, r.mode as i32), r);
    }

    let base_ratings: Vec<PlayerRating> = ratings_hash.into_values().collect();

    let rating_stats: Vec<Vec<MatchRatingStats>> = rating_stats_hash.into_values().collect();
    let flattened_stats: Vec<MatchRatingStats> = rating_stats.into_iter().flatten().collect();

    let adjustments: Vec<Vec<RatingAdjustment>> = rating_adjustments_hash.into_values().collect();
    let flattened_adjustments: Vec<RatingAdjustment> = adjustments.into_iter().flatten().collect();

    RatingCalculationResult {
        base_ratings,
        rating_stats: flattened_stats,
        adjustments: flattened_adjustments
    }
}

// Utility

/// Returns a vector of matchcosts for the given collection of games. If no games exist
/// in the match, returns None.
pub fn match_costs(games: &[Game]) -> Option<Vec<MatchCost>> {
    let mut match_costs: Vec<MatchCost> = Vec::new();

    // Map of { player_id, n_games_played }
    let mut games_played: HashMap<i32, i32> = HashMap::new();

    // Map of { player_id, normalized_score } - Used in matchcost formula
    let mut normalized_scores: HashMap<i32, f64> = HashMap::new();

    let n = games.len();
    if n == 0 {
        return None;
    }

    let normal = Normal::new(0.0, 1.0).unwrap();

    for game in games {
        let match_scores = &game.match_scores;
        let score_values: Vec<f64> = match_scores.iter().map(|x| x.score as f64).collect();
        let sum_scores: f64 = score_values.iter().sum();
        let average_score = sum_scores / match_scores.len() as f64;

        if average_score == 0.0 {
            continue;
        }

        let std_dev = score_values.std_dev();

        for score in match_scores {
            let player_id = score.player_id;
            
            games_played.entry(player_id).or_insert(0);
            normalized_scores.entry(player_id).or_insert(0.0);

            let cur_played = games_played.get(&player_id).unwrap();
            games_played.insert(player_id, cur_played + 1);
            let normalized_player_score = normalized_scores.get(&player_id).unwrap();

            if std_dev == 0.0 {
                normalized_scores.insert(player_id, normalized_player_score + 0.5);
            } else {
                let z_score = (score.score as f64 - average_score) / std_dev;
                normalized_scores.insert(player_id, normalized_player_score + normal.cdf(z_score));
            }
        }
    }

    for (player_id, n_played) in games_played {
        // The minimum match cost possible: e.g. 0.5 if you played 0 games in the match
        let base_score = 0.5 * n_played as f64;

        // Match cost is multiplied by something between 1.0 and (1.0 + lobby_bonus),
        // depending on whether 1 map was played vs all maps
        let lobby_bonus = 0.3;
        let norm_score = normalized_scores.get(&player_id).unwrap();

        let result = if n_played == 1 {
            (norm_score + base_score) * (1.0 / n_played as f64) * (1.0 + lobby_bonus)
        } else {
            (norm_score + base_score) * (1.0 / n_played as f64) *
                (1.0 + (lobby_bonus * ((n_played - 1) as f64) / (n as f64 / 1.0)).sqrt())
        };

        let mc = MatchCost {
            player_id,
            match_cost: result
        };

        match_costs.push(mc);
    }

    Some(match_costs)
}

pub fn mu_for_rank(rank: i32) -> f64 {
    let constants = RatingConstants::default();
    let multiplier = constants.multiplier as f64;
    let val = multiplier * (45.0 - (3.2 * (rank as f64).ln()));

    if val < multiplier * 5.0 {
        return multiplier * 5.0;
    }

    if val > multiplier * 30.0 {
        return multiplier * 30.0;
    }

    val
}

#[cfg(test)]
mod tests {
    use crate::model::model::mu_for_rank;

    #[test]
    fn mu_for_rank_returns_correct_min() {
        let rank = 1_000_000; // Some 7 digit player
        let expected = 225.0; // The minimum

        let value = mu_for_rank(rank);

        assert_eq!(expected, value);
    }

    #[test]
    fn mu_for_rank_returns_correct_max() {
        let rank = 1; // Some 7 digit player
        let expected = 1350.0; // The minimum

        let value = mu_for_rank(rank);

        assert_eq!(expected, value);
    }

    #[test]
    fn mu_for_rank_returns_correct_10k() {
        let rank = 10000; // Some 7 digit player
        let expected = 698.7109864354294; // The minimum

        let value = mu_for_rank(rank);

        assert!((expected - value).abs() < 0.000001);
    }

    #[test]
    fn mu_for_rank_returns_correct_500() {
        let rank = 500; // Some 7 digit player
        let expected = 1130.0964338272045; // The minimum

        let value = mu_for_rank(rank);

        assert!((expected - value).abs() < 0.000001);
    }
}
