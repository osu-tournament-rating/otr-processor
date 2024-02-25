mod constants;
mod data_processing;
mod decay;
mod recalc_helpers;
pub mod structures;

use crate::api::api_structs::{BaseStatsPost, Game, Match, MatchRatingStats, Player, RatingAdjustment};
use crate::model::decay::{is_decay_possible, DecayTracker};
use crate::model::structures::match_cost::MatchCost;
use crate::model::structures::mode::Mode;
use crate::model::structures::player_rating::PlayerRating;
use crate::model::structures::rating_calculation_result::RatingCalculationResult;
use crate::model::structures::team_type::TeamType;
use crate::utils::progress_utils::progress_bar;
use chrono::Utc;
use openskill::model::model::Model;
use openskill::model::plackett_luce::PlackettLuce;
use openskill::rating::{default_gamma, Rating};
use statrs::distribution::{ContinuousCDF, Normal};
use statrs::statistics::Statistics;
use std::collections::{HashMap, HashSet};

pub fn create_model() -> PlackettLuce {
    PlackettLuce::new(constants::BETA, constants::KAPPA, default_gamma)
}

// Rating generation

pub fn create_initial_ratings(matches: Vec<Match>, players: Vec<Player>) -> Vec<PlayerRating> {
    // The first step in the rating algorithm. Generate ratings from known ranks.

    // A fast lookup used for understanding who has default ratings created at this time.
    let mut stored_lookup_log: HashSet<(i32, Mode)> = HashSet::new();
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

            for score in game.match_scores {
                // Check if the player_id and enum_mode combination is already in created_ratings
                if stored_lookup_log.contains(&(score.player_id, mode)) {
                    // We've already initialized this player.
                    continue;
                }

                // Create ratings using the earliest known rank
                let player = player_hashmap
                    .get(&score.player_id)
                    .expect("Player should be present in the hashmap.");
                let rank: Option<i32> = match mode {
                    Mode::Osu => player.earliest_osu_global_rank.or(player.rank_standard),
                    Mode::Taiko => player.earliest_taiko_global_rank.or(player.rank_taiko),
                    Mode::Catch => player.earliest_catch_global_rank.or(player.rank_catch),
                    Mode::Mania => player.earliest_mania_global_rank.or(player.rank_mania),
                };

                let mu;
                let sigma;
                match rank {
                    Some(rank) => {
                        // Player has a valid identified rank (either the earliest known
                        // rank, or their current rank)
                        mu = mu_for_rank(rank);
                        sigma = constants::SIGMA;
                    }
                    None => {
                        // Player may be restricted / we cannot get hold of their rank info. Use default.
                        mu = constants::MU;
                        sigma = constants::SIGMA;
                    }
                }

                let rating = Rating::new(mu, sigma);
                let player_rating = PlayerRating {
                    player_id: score.player_id,
                    mode,
                    rating,
                };
                ratings.push(player_rating);

                stored_lookup_log.insert((score.player_id, mode));
            }
        }

        bar.inc(1);
    }

    ratings
}
/// Calculates a vector of initial ratings based on match cost,
/// returns the new ratings
pub fn calc_ratings(
    initial_ratings: Vec<PlayerRating>,
    matches: Vec<Match>,
    model: PlackettLuce,
) -> RatingCalculationResult {
    // Key = (player_id, mode as i32)
    // Value = Associated PlayerRating (if available)
    let mut ratings_hash: HashMap<(i32, Mode), PlayerRating> = HashMap::new();
    // Key = match_id
    // Value = Vec of MatchRatingStats per match
    let mut rating_stats_hash: HashMap<i32, Vec<MatchRatingStats>> = HashMap::new();
    // Key = player_id
    // Value = Vec of RatingAdjustments per player
    let mut rating_adjustments_hash: HashMap<i32, Vec<RatingAdjustment>> = HashMap::new();

    // Vector of match ratings to return
    let mut rating_stats: Vec<MatchRatingStats> = Vec::new();
    // Vector of adjustments to return
    let mut adjustments: Vec<RatingAdjustment> = Vec::new();

    // Insert every given player into initial ratings
    for r in initial_ratings {
        ratings_hash.insert((r.player_id, r.mode), r);
    }
    // Create a decay tracker to run decay adjustments
    let mut decay_tracker = DecayTracker::new();
    // Create a progress bar for match processing
    let bar = progress_bar(matches.len() as u64);

    for curr_match in matches {
        // Skip any match where expected ruleset of games doesn't match the declared one
        // TODO: Implement proper median
        if curr_match
            .games
            .iter()
            .any(|game| game.play_mode != curr_match.mode)
        {
            continue;
        }
        // Obtain all player match costs
        // Skip the match if there are no valid match costs
        let mut match_costs = match match_costs(&curr_match.games) {
            Some(mc) => mc,
            None => continue,
        };
        // Start time of the match
        // Skip the match if not defined
        let start_time = match curr_match.start_time {
            Some(t) => t,
            None => continue,
        };
        // Collection of match ratings
        // Key = player_id
        // Value = MatchRatingStats
        let mut stats: HashMap<i32, MatchRatingStats> = HashMap::new();

        let mut to_rate = vec![];

        for match_cost in match_costs {
            // If user has no prior activity, store the first one
            if let None = decay_tracker.get_activity(match_cost.player_id, curr_match.mode) {
                decay_tracker.record_activity(match_cost.player_id, curr_match.mode, start_time);
            }

            // Get user's current rating
            let mut rating_prior =
                match ratings_hash.get_mut(&(match_cost.player_id, curr_match.mode)) {
                    None => panic!("No rating found?"),
                    Some(rate) => rate,
                };
            // If decay is possible, apply it to rating_prior
            if is_decay_possible(rating_prior.rating.mu) {
                let adjustment = decay_tracker.decay(
                    match_cost.player_id,
                    curr_match.mode,
                    rating_prior.rating.mu,
                    rating_prior.rating.sigma,
                    start_time,
                );
                match adjustment {
                    Some(mut adj) => {
                        rating_prior.rating.mu = adj[adj.len() - 1].rating_after;
                        rating_prior.rating.sigma = adj[adj.len() - 1].volatility_after;
                        // Save all rating adjustments for graph displays in the front end
                        rating_adjustments_hash.entry(match_cost.player_id)
                            .and_modify(|a| a.append(&mut adj))
                            .or_insert(adj);
                    }
                    None => (),
                }
            }
            to_rate.push(rating_prior);

            let prior_mu = rating_prior.rating.mu;
            // TODO: Get country
            let country = ();
            // Updating rank for tracking
            ratings_hash
                .entry((match_cost.player_id, curr_match.mode))
                .and_modify(|f| f.rating.mu = prior_mu);
            // REQ: get user's rankings from somewhere

            let rating_stats_before = rating_stats_hash.get(&rating_prior.player_id).unwrap();
            let current_player_index = rating_stats_before
                .iter()
                .position(|x| x.player_id == rating_prior.player_id)
                .unwrap();
            // let global_rank_before = rating_stats_before[current_player_index].global_rank_before;
            // let country_rank_before = rating_stats_before[current_player_index].country_rank_before;
            // let percentile_before = rating_stats_before[current_player_index].percentile_before;

            // Count all games with H2H vs non-H2H team types
            let mut team_based_count = 0;
            let mut single_count = 0;

            for game in curr_match.games {
                if game.team_type == TeamType::HeadToHead {
                    single_count += 1;
                } else {
                    team_based_count += 1;
                }
            }
            let team_based = team_based_count > single_count;

            let mut teammate_ratings: Option<Vec<_>> = None;
            let mut opponent_ratings: Option<Vec<_>> = None;

            if team_based {
                // Get user's team ID
                // TODO: needs to be a median across all games ideally
                let curr_player_team = curr_match.games[0]
                    .match_scores
                    .iter()
                    .find(|x| x.player_id == rating_prior.player_id)
                    .unwrap()
                    .team;

                // Get IDs of all users in player's team and the opposite team
                let t_ids: Vec<_> = curr_match.games[0]
                    .match_scores
                    .iter()
                    .filter(|x| x.team == curr_player_team)
                    .map(|x| x.player_id)
                    .collect();
                let o_ids: Vec<_> = curr_match.games[0]
                    .match_scores
                    .iter()
                    .filter(|x| x.team != curr_player_team)
                    .map(|x| x.player_id)
                    .collect();
                // Get teammate and opponent ratings

                let mut teammate: Vec<f64> = Vec::new();
                let mut opponent: Vec<f64> = Vec::new();

                for id in t_ids {
                    let teammate_id = id;
                    let mode = curr_match.mode;
                    let rating = match ratings_hash.get(&(teammate_id, mode)) {
                        Some(r) => r.rating.mu,
                        None => todo!("This player is not in the hashmap"),
                    };
                    teammate.push(rating)
                }
                for id in o_ids {
                    let opponent_id = id;
                    let mode = curr_match.mode;
                    let rating = match ratings_hash.get(&(opponent_id, mode)) {
                        Some(r) => r.rating.mu,
                        None => todo!("This player is not in the hashmap"),
                    };
                    opponent.push(rating)
                }

                teammate_ratings = Some(teammate);
                opponent_ratings = Some(opponent);
            }
            // Get average ratings of both teams
            let average_t_rating = if teammate_ratings.is_some() {
                let len = teammate_ratings.unwrap().len();
                let ratings = teammate_ratings.unwrap().iter().sum();
                ratings / len
            } else {
                None
            };
            let average_o_rating = if opponent_ratings.is_some() {
                let len = opponent_ratings.unwrap().len();
                let ratings = opponent_ratings.unwrap().iter().sum();
                ratings / len
            } else {
                None
            };
            // Record currently processed match
            // Uses start_time as end_time can be null (issue on osu-web side)
            decay_tracker.record_activity(rating_prior.player_id, curr_match.mode, start_time);

            let adjustment = MatchRatingStats {
                player_id: rating_prior.player_id,
                match_id: curr_match.match_id as i32,
                match_cost: match_cost.match_cost,
                rating_before: rating_prior.rating.mu,
                rating_after: 0.0,
                rating_change: 0.0,
                volatility_before: rating_prior.rating.sigma,
                volatility_after: 0.0,
                volatility_change: 0.0,
                global_rank_before: 0,
                global_rank_after: 0,
                global_rank_change: 0,
                country_rank_before: 0,
                country_rank_after: 0,
                country_rank_change: 0,
                percentile_before: 0.0,
                percentile_after: 0.0,
                percentile_change: 0.0,
                average_teammate_rating: average_t_rating,
                average_opponent_rating: average_o_rating,
            };

            stats.insert(rating_prior.player_id, adjustment);
        }
        // Sort rated players and their matchcosts by player IDs for correct mappings
        to_rate.sort_by(|x, y| x.player_id.cmp(&y.player_id));
        match_costs.sort_by(|x,y| x.player_id.cmp(&y.player_id));

        // Variable names are used according to the function signature for easier referencing

        // Model ratings require a vector of teams to be passed, but matches are considered as FFA
        // so we need to consider every player as a one-man team
        let teams: Vec<Vec<Rating>> = to_rate
            .iter()
            .map(|player| vec![player.rating])
            .collect();
        // Match costs are floats, but since we only need their order,
        // mapping them this way with precision loss should be fine
        let ranks: Vec<usize> = match_costs
            .iter()
            .map(|mc| (mc.match_cost * 1000.0) as usize)
            .collect();
        let model_rating = model.rate(teams, ranks);
        // Apply resulting ratings to the players
        let flattened_ratings: Vec<Rating> = model_rating
            .into_iter()
            .flatten()
            .collect();
        for (idx, player) in to_rate.iter().enumerate() {
            player.rating = flattened_ratings[idx].clone();
        }
        

        for mc in match_costs {
            let curr_id = mc.player_id;
            let key = (mc.player_id, curr_match.mode);
            let mut new_rating = ratings_hash.get_mut(&key).unwrap();

            if new_rating.rating.mu < 100.0 {
                new_rating.rating.mu = 100.0;
            }

            // get new global/country ranks and percentiles
            stats.entry(curr_id).and_modify(|f| {
                f.rating_after = new_rating.rating.mu;
                f.volatility_after = new_rating.rating.sigma;
                // f.country_rank_after = new_country_rank
                // f.global_rank_after = new_global_rank
                // f.percentile_after = new_percentile
                f.rating_change = f.rating_after - f.rating_before;
                f.volatility_change = f.volatility_after - f.volatility_before;
                // f.global_rank_change = f.global_rank_after - f.global_rank_before;
                // f.country_rank_change = f.country_rank_after - f.country_rank_before;
                // f.percentile_change = f.percentile_after - f.percentile_before;
            });
        }
        rating_stats.extend(stats.into_values());
        bar.inc(1);
    }

    let bar = progress_bar(ratings_hash.len() as u64);

    for ((player_id, gamemode), rating) in ratings_hash {
        let curr_rating = rating;

        let mu = curr_rating.rating.mu;
        let sigma = curr_rating.rating.sigma;

        if is_decay_possible(mu) {
            // As all matches prior are processed, we can use current time to apply decay
            let curr_time = Utc::now();
            let decays = match decay_tracker
                .decay(player_id, gamemode, mu, sigma, curr_time.into()) {
                Some(adj) => {
                    rating_adjustments_hash.entry(player_id).and_modify(|a| a.extend(adj.iter()));
                    Some(adj)
                },
                None => None,
            };

            // If decays exist, apply them
            if let Some(d) = decays {
                ratings_hash.entry((player_id, gamemode)).and_modify(|f| {
                    f.rating.mu = d[d.len() - 1].rating_after;
                    f.rating.sigma = d[d.len() - 1].volatility_after;
                });
            }
        }
        bar.inc(1);
    }

    RatingCalculationResult {
        base_ratings: ratings_hash.values().collect(),
        rating_stats,
        adjustments,
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
            (norm_score + base_score)
                * (1.0 / n_played as f64)
                * (1.0 + (lobby_bonus * ((n_played - 1) as f64) / (n as f64 / 1.0)).sqrt())
        };

        let mc = MatchCost {
            player_id,
            match_cost: result,
        };

        match_costs.push(mc);
    }

    Some(match_costs)
}

pub fn mu_for_rank(rank: i32) -> f64 {
    let val = constants::MULTIPLIER * (constants::OSU_RATING_INTERCEPT -
        (constants::OSU_RATING_SLOPE * (rank as f64).ln()));

    if val < constants::MULTIPLIER * constants::OSU_RATING_FLOOR {
        return constants::MULTIPLIER * constants::OSU_RATING_FLOOR;
    }

    if val > constants::MULTIPLIER * constants::OSU_RATING_CEILING {
        return constants::MULTIPLIER * constants::OSU_RATING_CEILING;
    }

    val
}

#[cfg(test)]
mod tests {
    use crate::model::mu_for_rank;

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
