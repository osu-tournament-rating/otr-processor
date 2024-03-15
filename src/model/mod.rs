mod constants;
mod data_processing;
mod decay;
mod recalc_helpers;
pub mod structures;

use crate::{
    api::api_structs::{Game, Match, MatchRatingStats, MatchScore, Player, RatingAdjustment},
    model::{
        decay::{is_decay_possible, DecayTracker},
        structures::{
            match_cost::MatchCost, mode::Mode, player_rating::PlayerRating,
            rating_calculation_result::RatingCalculationResult, team_type::TeamType,
        },
    },
    utils::progress_utils::progress_bar,
};
use chrono::Utc;
use openskill::{
    model::{model::Model, plackett_luce::PlackettLuce},
    rating::{default_gamma, Rating},
};
use statrs::{
    distribution::{ContinuousCDF, Normal},
    statistics::Statistics,
};
use std::collections::{HashMap, HashSet};

pub fn create_model() -> PlackettLuce {
    PlackettLuce::new(constants::BETA, constants::KAPPA, default_gamma)
}

// Rating generation

pub fn create_initial_ratings(matches: &Vec<Match>, players: &Vec<Player>) -> Vec<PlayerRating> {
    // The first step in the rating algorithm. Generate ratings from known ranks.

    // A fast lookup used for understanding who has default ratings created at this time.
    let mut stored_lookup_log: HashSet<(i32, Mode)> = HashSet::new();
    let mut ratings: Vec<PlayerRating> = Vec::new();
    let bar = progress_bar(matches.len() as u64);

    // Map the osu ids for fast lookup
    let mut player_hashmap: HashMap<i32, Player> = HashMap::new();

    for player in players {
        player_hashmap.entry(player.id).or_insert(player.clone());
    }

    for m in matches {
        for game in &m.games {
            let mode = game.play_mode;

            for score in &game.match_scores {
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
    initial_ratings: &Vec<PlayerRating>,
    country_mapping: &HashMap<i32, String>,
    matches: &Vec<Match>,
    model: &PlackettLuce,
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
        ratings_hash.insert((r.player_id, r.mode), r.clone());
    }
    // Create a decay tracker to run decay adjustments
    let mut decay_tracker = DecayTracker::new();
    // Create a progress bar for match processing
    let bar = progress_bar(matches.len() as u64);

    for curr_match in matches {
        // Skip any match where expected ruleset of games doesn't match the declared one
        if curr_match.games.iter().any(|game| game.play_mode != curr_match.mode) {
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

        for match_cost in &match_costs {
            // If user has no prior activity, store the first one
            if let None = decay_tracker.get_activity(match_cost.player_id, curr_match.mode) {
                decay_tracker.record_activity(match_cost.player_id, curr_match.mode, start_time);
            }

            // Get user's current rating
            let mut rating_prior = match ratings_hash.get_mut(&(match_cost.player_id, curr_match.mode)) {
                None => panic!("No rating found?"),
                Some(rate) => rate.clone(),
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
                        rating_adjustments_hash
                            .entry(match_cost.player_id)
                            .and_modify(|a| a.append(&mut adj))
                            .or_insert(adj);
                    }
                    None => (),
                }
            }
            to_rate.push(rating_prior.clone());

            let prior_mu = rating_prior.rating.mu;
            // Updating rank for tracking
            ratings_hash
                .entry((match_cost.player_id, curr_match.mode))
                .and_modify(|f| f.rating.mu = prior_mu);
            // REQ: get user's rankings from somewhere

            // Count all games with H2H vs non-H2H team types
            let mut team_based_count = 0;
            let mut single_count = 0;

            for game in &curr_match.games {
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
                let mut curr_player_team = 1;
                // Find first Game in the Match where the player exists
                for game in &curr_match.games {
                    let game_with_player = game.match_scores.iter().find(|x| x.player_id == rating_prior.player_id);
                    match game_with_player {
                        Some(g) => {
                            curr_player_team = g.team;
                            break;
                        }
                        None => continue,
                    }
                }

                // Get IDs of all users in player's team and the opposite team
                let (mut teammate_list, mut opponent_list): (Vec<MatchScore>, Vec<MatchScore>) = curr_match
                    .games
                    .iter()
                    .map(|f| f.match_scores.clone())
                    .flatten()
                    .partition(|score| score.team == curr_player_team);

                let mut teammate_list: Vec<i32> = teammate_list.iter().map(|player| player.player_id).collect();

                let mut opponent_list: Vec<i32> = opponent_list.iter().map(|player| player.player_id).collect();

                teammate_list.sort();
                teammate_list.dedup();
                opponent_list.sort();
                opponent_list.dedup();

                // Get teammate and opponent ratings
                let mut teammates: Vec<f64> = Vec::new();
                let mut opponents: Vec<f64> = Vec::new();

                push_team_rating(&mut ratings_hash, curr_match, teammate_list, &mut teammates);
                push_team_rating(&mut ratings_hash, curr_match, opponent_list, &mut opponents);

                if teammates.len() > 0 {
                    teammate_ratings = Some(teammates);
                }

                if opponents.len() > 0 {
                    opponent_ratings = Some(opponents);
                }
            }
            // Get average ratings of both teams
            let average_t_rating = average_rating(teammate_ratings);
            let average_o_rating = average_rating(opponent_ratings);
            // Record currently processed match
            // Uses start_time as end_time can be null (issue on osu-web side)
            decay_tracker.record_activity(rating_prior.player_id, curr_match.mode, start_time);

            let global_rank_before =
                get_global_rank(&rating_prior.rating.mu, &rating_prior.player_id, &initial_ratings);
            let country_rank_before = get_country_rank(
                &rating_prior.rating.mu,
                &rating_prior.player_id,
                &country_mapping,
                &initial_ratings,
            );
            let percentile_before = get_percentile(global_rank_before, initial_ratings.len() as i32);

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
                global_rank_before,
                global_rank_after: 0,
                global_rank_change: 0,
                country_rank_before,
                country_rank_after: 0,
                country_rank_change: 0,
                percentile_before,
                percentile_after: 0.0,
                percentile_change: 0.0,
                average_teammate_rating: average_t_rating,
                average_opponent_rating: average_o_rating,
            };

            stats.insert(rating_prior.player_id, adjustment);
        }
        // Sort rated players and their matchcosts by player IDs for correct mappings
        to_rate.sort_by(|x, y| x.player_id.cmp(&y.player_id));
        match_costs.sort_by(|x, y| x.player_id.cmp(&y.player_id));

        // Variable names are used according to the function signature for easier referencing

        // Model ratings require a vector of teams to be passed, but matches are considered as FFA
        // so we need to consider every player as a one-man team
        let teams: Vec<Vec<Rating>> = to_rate.iter().map(|player| vec![player.rating.clone()]).collect();
        // Match costs are floats, but since we only need their order,
        // mapping them this way with precision loss should be fine
        let ranks: Vec<usize> = match_costs
            .iter()
            .map(|mc| (mc.match_cost * 1000.0) as usize)
            .rev()
            .collect();
        let model_rating = model.rate(teams, ranks);
        // Apply resulting ratings to the players
        let flattened_ratings: Vec<Rating> = model_rating.into_iter().flatten().collect();
        for (idx, player) in to_rate.iter_mut().enumerate() {
            player.rating = flattened_ratings[idx].clone();
        }

        for rating in to_rate {
            ratings_hash
                .entry((rating.player_id, rating.mode))
                .and_modify(|mut f| *f = rating);
        }

        for mc in match_costs {
            let curr_id = mc.player_id;
            let key = (mc.player_id, curr_match.mode);
            let new_rating = ratings_hash.get_mut(&key).unwrap();

            if new_rating.rating.mu < 100.0 {
                new_rating.rating.mu = 100.0;
            }

            let global_rank_after = get_global_rank(&new_rating.rating.mu, &new_rating.player_id, &initial_ratings);
            let country_rank_after = get_country_rank(
                &new_rating.rating.mu,
                &new_rating.player_id,
                &country_mapping,
                &initial_ratings,
            );
            let percentile_after = get_percentile(global_rank_after, initial_ratings.len() as i32);

            // get new global/country ranks and percentiles
            stats.entry(curr_id).and_modify(|f| {
                f.rating_after = new_rating.rating.mu;
                f.volatility_after = new_rating.rating.sigma;
                f.country_rank_after = country_rank_after;
                f.global_rank_after = global_rank_after;
                f.percentile_after = percentile_after;
                f.rating_change = f.rating_after - f.rating_before;
                f.volatility_change = f.volatility_after - f.volatility_before;
                f.global_rank_change = f.global_rank_after - f.global_rank_before;
                f.country_rank_change = f.country_rank_after - f.country_rank_before;
                f.percentile_change = f.percentile_after - f.percentile_before;
            });
        }
        rating_stats.extend(stats.into_values());
        bar.inc(1);
    }

    let bar = progress_bar(ratings_hash.len() as u64);

    for ((player_id, gamemode), rating) in &mut ratings_hash {
        let curr_rating = rating;

        let mu = curr_rating.rating.mu;
        let sigma = curr_rating.rating.sigma;

        if is_decay_possible(mu) {
            // As all matches prior are processed, we can use current time to apply decay
            let curr_time = Utc::now();
            let decays = match decay_tracker.decay(*player_id, *gamemode, mu, sigma, curr_time.into()) {
                Some(adj) => {
                    rating_adjustments_hash
                        .entry(*player_id)
                        .and_modify(|a| a.extend(adj.clone().into_iter()));
                    Some(adj)
                }
                None => None,
            };

            // If decays exist, apply them
            if let Some(d) = decays {
                curr_rating.rating.mu = d[d.len() - 1].rating_after;
                curr_rating.rating.sigma = d[d.len() - 1].volatility_after;
            }
        }
        bar.inc(1);
    }

    let mut base_ratings: Vec<PlayerRating> = vec![];

    for (k, v) in ratings_hash {
        base_ratings.push(v.clone());
    }

    RatingCalculationResult {
        base_ratings,
        rating_stats,
        adjustments,
    }
}
fn get_percentile(rank: i32, player_count: i32) -> f64 {
    let res = (rank / player_count) as f64;
    // println!("percentile: {:?}", res);
    res
}

fn get_country_rank(
    mu: &f64,
    player_id: &i32,
    country_mapping: &&HashMap<i32, String>,
    existing_ratings: &&Vec<PlayerRating>,
) -> i32 {
    let mut ratings: Vec<f64> = existing_ratings
        .clone()
        .iter()
        .filter(|r| r.player_id != *player_id && country_mapping.get(player_id) == country_mapping.get(&r.player_id))
        .map(|r| r.rating.mu)
        .collect();
    ratings.push(*mu);
    ratings.sort_by(|x, y| x.partial_cmp(y).unwrap());
    ratings.reverse();
    for (rank, mu_iter) in ratings.iter().enumerate() {
        if mu == mu_iter {
            return (rank + 1) as i32;
        }
    }
    return 0;
}

fn get_global_rank(mu: &f64, player_id: &i32, existing_ratings: &&Vec<PlayerRating>) -> i32 {
    let mut ratings: Vec<f64> = existing_ratings
        .clone()
        .iter()
        .filter(|r| r.player_id != *player_id)
        .map(|r| r.rating.mu)
        .collect();
    ratings.push(*mu);
    ratings.sort_by(|x, y| x.partial_cmp(y).unwrap());
    ratings.reverse();
    for (rank, mu_iter) in ratings.iter().enumerate() {
        if mu == mu_iter {
            return (rank + 1) as i32;
        }
    }
    return 0;
}

fn push_team_rating(
    ratings_hash: &mut HashMap<(i32, Mode), PlayerRating>,
    curr_match: &Match,
    teammate_list: Vec<i32>,
    teammate: &mut Vec<f64>,
) {
    for id in teammate_list {
        let teammate_id = id;
        let mode = curr_match.mode;
        let rating = match ratings_hash.get(&(teammate_id, mode)) {
            Some(r) => r.rating.mu,
            None => todo!("This player is not in the hashmap"),
        };
        teammate.push(rating)
    }
}

fn average_rating(ratings: Option<Vec<f64>>) -> Option<f64> {
    let avg_rating = if let Some(rating) = ratings {
        let len = rating.len() as f64;
        let s_ratings: f64 = rating.into_iter().sum();
        Some(s_ratings / len)
    } else {
        None
    };
    avg_rating
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
    let val =
        constants::MULTIPLIER * (constants::OSU_RATING_INTERCEPT - (constants::OSU_RATING_SLOPE * (rank as f64).ln()));

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
    use crate::{
        api::api_structs::{Beatmap, Game, Match, MatchScore},
        model::{
            calc_ratings, mu_for_rank,
            structures::{mode::Mode, player_rating::PlayerRating, scoring_type::ScoringType, team_type::TeamType},
        },
    };
    use openskill::{model::model::Model, rating::Rating};
    use std::collections::HashMap;

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

    #[test]
    fn test_calc_ratings_1v1() {
        let mut initial_ratings = Vec::new();
        let mut country_mapping = HashMap::new();

        // Set both players to be from the same country to check country rankings
        country_mapping.insert(0, "US".to_string());
        country_mapping.insert(1, "US".to_string());

        // Create 2 players with default ratings
        for i in 0..2 {
            initial_ratings.push(PlayerRating {
                player_id: i,
                mode: Mode::Osu,
                rating: Rating {
                    // We subtract 1.0 here because
                    // global / country ranks etc. need to be known ahead of time.
                    // Player 0 has a higher starting rating than player 1,
                    // but player 1 wins. Thus, we simulate an upset and
                    // associated stat changes
                    mu: 1500.0 - i as f64,
                    sigma: 200.0,
                },
            })
        }

        // Create a match with the following structure:
        // - Match
        // - Game
        // - MatchScore (Player 0, Team 1, Score 525000)
        // - MatchScore (Player 1, Team 2, Score 525001)
        let mut matches = Vec::new();

        let start_time = chrono::offset::Utc::now().fixed_offset();
        let end_time = Some(start_time); // Assuming end_time is the same as start_time for demonstration

        let beatmap = test_beatmap();

        let mut match_scores = Vec::new();
        match_scores.push(MatchScore {
            player_id: 0,
            team: 1, // Blue
            score: 525000,
            enabled_mods: None,
            misses: 0,
            accuracy_standard: 100.0,
            accuracy_taiko: 0.0,
            accuracy_catch: 0.0,
            accuracy_mania: 0.0,
        });
        match_scores.push(MatchScore {
            player_id: 1,
            team: 2,       // Red
            score: 525001, // +1 score from blue. Should be the winner.
            enabled_mods: None,
            misses: 0,
            accuracy_standard: 100.0,
            accuracy_taiko: 0.0,
            accuracy_catch: 0.0,
            accuracy_mania: 0.0,
        });

        let game = Game {
            id: 0,
            game_id: 0,
            play_mode: Mode::Osu,
            scoring_type: ScoringType::ScoreV2,
            team_type: TeamType::HeadToHead,
            start_time,
            end_time,
            beatmap: Some(beatmap),
            match_scores,
            mods: 0,
        };

        let mut games = Vec::new();
        games.push(game);

        let match_instance = Match {
            id: 0,
            match_id: 0,
            name: Some("TEST: (One) vs (One)".to_string()),
            mode: Mode::Osu,
            start_time: Some(start_time),
            end_time: None,
            games,
        };

        matches.push(match_instance);

        let loser_id = 0;
        let winner_id = 1;

        let model = super::create_model();
        let expected_outcome = model.rate(
            vec![
                vec![Rating {
                    mu: 1500.0,
                    sigma: 200.0,
                }],
                vec![Rating {
                    mu: 1499.0,
                    sigma: 200.0,
                }],
            ],
            vec![winner_id, loser_id],
        );

        let loser_expected_outcome = &expected_outcome[loser_id][0];
        let winner_expected_outcome = &expected_outcome[winner_id][0];

        let result = calc_ratings(&initial_ratings, &country_mapping, &matches, &model);
        let loser_stats = result
            .rating_stats
            .iter()
            .find(|x| x.player_id == loser_id as i32)
            .unwrap();
        let winner_stats = result
            .rating_stats
            .iter()
            .find(|x| x.player_id == winner_id as i32)
            .unwrap();

        let winner_base_stat = result
            .base_ratings
            .iter()
            .find(|x| x.player_id == winner_id as i32)
            .unwrap();
        let loser_base_stat = result
            .base_ratings
            .iter()
            .find(|x| x.player_id == loser_id as i32)
            .unwrap();

        assert!(
            (winner_base_stat.rating.mu - winner_expected_outcome.mu).abs() < f64::EPSILON,
            "Winner's base stat mu is {}, should be {}",
            winner_base_stat.rating.mu,
            winner_expected_outcome.mu
        );

        assert!(
            (winner_base_stat.rating.sigma - winner_expected_outcome.sigma).abs() < f64::EPSILON,
            "Winner's base stat sigma is {}, should be {}",
            winner_base_stat.rating.sigma,
            winner_expected_outcome.sigma
        );

        assert!(
            (loser_base_stat.rating.mu - loser_expected_outcome.mu).abs() < f64::EPSILON,
            "Loser's base stat mu is {}, should be {}",
            loser_base_stat.rating.mu,
            loser_expected_outcome.mu
        );
        assert!(
            (loser_base_stat.rating.sigma - loser_expected_outcome.sigma).abs() < f64::EPSILON,
            "Loser's base stat sigma is {}, should be {}",
            loser_base_stat.rating.sigma,
            loser_expected_outcome.sigma
        );

        assert_eq!(
            result.base_ratings.len(),
            2,
            "There are {} base ratings, should be {}",
            result.base_ratings.len(),
            2
        );

        assert_eq!(
            result.rating_stats.len(),
            2,
            "There are {} rating stats, should be {}",
            result.rating_stats.len(),
            2
        );
        assert_eq!(
            result.adjustments.len(),
            0,
            "There are {} rating adjustments, should be {}",
            result.adjustments.len(),
            0
        );

        // TODO: Test can be extended to accomodate other stats etc.

        // Ensure match cost of winner is > loser
        assert!(
            winner_stats.match_cost > loser_stats.match_cost,
            "loser's match cost is higher"
        );

        // Average teammate ratings (None because 1v1)
        assert_eq!(
            loser_stats.average_teammate_rating, None,
            "Loser's teammate rating should be None"
        );
        assert_eq!(
            winner_stats.average_teammate_rating, None,
            "Winner's teammate rating should be None"
        );

        // TODO: Figure out why the differences are this large

        // Expected mu = actual mu
        assert!(
            (loser_expected_outcome.mu - loser_stats.rating_after).abs() < 1.0,
            "Loser's rating is {}, should be {}",
            loser_stats.rating_after,
            loser_expected_outcome.mu
        );
        assert!(
            (loser_expected_outcome.sigma - loser_stats.volatility_after).abs() < 1.0,
            "Loser's volatility is {}, should be {}",
            loser_stats.volatility_after,
            loser_expected_outcome.sigma
        );

        // Expected sigma = actual sigma
        assert!(
            (winner_expected_outcome.mu - winner_stats.rating_after).abs() < 1.0,
            "Winner's rating is {}, should be {}",
            winner_stats.rating_after,
            winner_expected_outcome.mu
        );
        assert!(
            (winner_expected_outcome.sigma - winner_stats.volatility_after).abs() < 1.0,
            "Winner's volatility is {}, should be {}",
            winner_stats.volatility_after,
            winner_expected_outcome.sigma
        );

        // mu before
        assert_eq!(
            loser_stats.rating_before, 1500.0,
            "Loser's rating before is {}, should be {}",
            loser_stats.rating_before, 1500.0
        );
        assert_eq!(
            winner_stats.rating_before, 1499.0,
            "Winner's rating before is {}, should be {}",
            winner_stats.rating_before, 1499.0
        );

        // sigma before
        assert_eq!(
            loser_stats.volatility_before, 200.0,
            "Loser's volatility before is {}, should be {}",
            loser_stats.volatility_before, 200.0
        );
        assert_eq!(
            winner_stats.volatility_before, 200.0,
            "Winner's volatility before is {}, should be {}",
            winner_stats.volatility_before, 200.0
        );

        // mu change
        assert_eq!(
            loser_stats.rating_change,
            loser_stats.rating_after - loser_stats.rating_before,
            "Loser's rating change is {}, should be {}",
            loser_stats.rating_change,
            loser_stats.rating_after - loser_stats.rating_before
        );
        assert_eq!(
            winner_stats.rating_change,
            winner_stats.rating_after - winner_stats.rating_before,
            "Winner's rating change is {}, should be {}",
            winner_stats.rating_change,
            winner_stats.rating_after - winner_stats.rating_before
        );

        // sigma change
        assert_eq!(
            loser_stats.volatility_change,
            loser_stats.volatility_after - loser_stats.volatility_before,
            "Loser's volatility change is {}, should be {}",
            loser_stats.volatility_change,
            loser_stats.volatility_after - loser_stats.volatility_before
        );
        assert_eq!(
            winner_stats.volatility_change,
            winner_stats.volatility_after - winner_stats.volatility_before,
            "Winner's volatility change is {}, should be {}",
            winner_stats.volatility_change,
            winner_stats.volatility_after - winner_stats.volatility_before
        );

        // global rank before -- remember, we are simulating an upset,
        // so the loser should have a higher initial rank than the winner.
        assert_eq!(
            loser_stats.global_rank_before, 1,
            "Loser's rank before is {}, should be {}",
            loser_stats.global_rank_before, 1
        );
        assert_eq!(
            winner_stats.global_rank_before, 2,
            "Winner's rank before is {}, should be {}",
            winner_stats.global_rank_before, 2
        );

        // global rank after
        // Player 1 ended up winning, so they should be rank 1 now.
        assert_eq!(loser_stats.global_rank_after, 2);
        assert_eq!(winner_stats.global_rank_after, 1);

        // global rank change
        assert_eq!(
            loser_stats.global_rank_change,
            loser_stats.global_rank_after - loser_stats.global_rank_before
        );
        assert_eq!(
            winner_stats.global_rank_change,
            winner_stats.global_rank_after - winner_stats.global_rank_before
        );

        // country rank before
        assert_eq!(loser_stats.country_rank_before, 1);
        assert_eq!(winner_stats.country_rank_before, 2);

        // country rank after
        // Player 1 ended up winning, so they should be rank 1 now.
        assert_eq!(loser_stats.country_rank_after, 2);
        assert_eq!(winner_stats.country_rank_after, 1);

        // country rank change
        assert_eq!(
            loser_stats.country_rank_change,
            loser_stats.country_rank_after - loser_stats.country_rank_before
        );
        assert_eq!(
            winner_stats.country_rank_change,
            winner_stats.country_rank_after - winner_stats.country_rank_before
        );

        // Percentile before
        // Worst in the collection (before match takes place)
        assert_eq!(
            winner_stats.percentile_before, 1.0,
            "Winner's percentile before is {}, should be {}",
            winner_stats.percentile_before, 1.0
        );
        // Best in the collection
        assert_eq!(
            loser_stats.percentile_before, 0.0,
            "Loser's percentile before is {}, should be {}",
            loser_stats.percentile_before, 0.0
        );

        // Percentile after (upset, so reverse the percentiles)
        assert_eq!(
            winner_stats.percentile_after, 0.0,
            "Winner's percentile after is {:?}, should be {:?}",
            winner_stats.percentile_after, 0.0
        );
        assert_eq!(
            loser_stats.percentile_after, 1.0,
            "Loser's percentile after is {:?}, should be {:?}",
            loser_stats.percentile_after, 1.0
        );
    }

    fn test_beatmap() -> Beatmap {
        Beatmap {
            artist: "Test".to_string(),
            beatmap_id: 0,
            bpm: Some(220.0),
            mapper_id: 0,
            mapper_name: "efaf".to_string(),
            sr: 6.0,
            cs: 4.0,
            ar: 9.0,
            hp: 7.0,
            od: 9.0,
            drain_time: 160.0,
            length: 165.0,
            title: "Testing".to_string(),
            diff_name: Some("Testing".to_string()),
        }
    }

    #[test]
    /// Simulates a TeamVS match (4v4 TS 8), Bo3
    fn test_calc_ratings_team_vs() {
        let mut initial_ratings = Vec::new();
        let mut country_mapping = HashMap::new();
        let model = super::create_model();

        // Insert the known countries of the players

        // Ids 0-7 = Team 2 (Red)
        // Ids 8-15 = Team 1 (Blue)

        for i in 0..8 {
            country_mapping.insert(i, "US".to_string());
        }

        for i in 8..16 {
            country_mapping.insert(i, "SK".to_string());
        }

        let initial_mu = vec![
            2350.0, 2100.0, 2900.0, 1850.0, 1200.0, 2130.0, 2603.0, 2990.0, 3122.0, 3000.0, 2300.0, 2500.0, 2430.0,
            2405.0, 2740.0, 2004.0,
        ];

        for i in 0..16 {
            let cur_mu = initial_mu[i as usize];
            initial_ratings.push(PlayerRating {
                player_id: i,
                mode: Mode::Osu,
                rating: Rating {
                    mu: cur_mu,
                    sigma: 200.0,
                },
            })
        }

        let mut matches: Vec<Match> = Vec::new();

        let start_time = chrono::offset::Utc::now().fixed_offset();
        let end_time = Some(start_time); // Assuming end_time is the same as start_time for demonstration

        let beatmap = test_beatmap();

        // =====================
        // MATCH DEFINITION
        // =====================

        // Create a match with the following structure:
        // - Match: 3 Games. Team red wins twice, team blue wins once. Team red wins the first game.
        // - Game:
        //  - Team Red (2): Players 0-3 (winner)
        //  - Team Blue (1): Players 8-11
        //  - Results order (best to worst): 0, 1, 2, 3, 8, 9, 10, 11
        //
        // - Game:
        //  - Team Red: Players 4-7
        //  - Team Blue: Players 12-15 (winner)
        //  - Results order: 12, 13, 14, 15, 4, 5, 6, 7
        //
        // - Game:
        //  - Team Red: Players 3-6 (winner)
        //  - Team Blue: Players 10-13
        //  - Results order: 3, 4, 5, 6, 10, 11, 12, 13

        let fake_match = Match {
            id: 1,
            match_id: 123456,
            name: Some("OWC2024: (United States) vs (South Korea)".to_string()),
            mode: Mode::Osu,
            start_time: Some(end_time.unwrap()),
            end_time,
            games: vec![
                Game {
                    id: 1,
                    play_mode: Mode::Osu,
                    scoring_type: ScoringType::ScoreV2,
                    team_type: TeamType::TeamVs,
                    mods: 9, // HD NF
                    game_id: 1002340238,
                    start_time,
                    end_time,
                    beatmap: Some(beatmap.clone()),
                    match_scores: vec![
                        MatchScore {
                            player_id: 0,
                            team: 2,
                            score: 1_020_480,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 0.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0,
                        },
                        MatchScore {
                            player_id: 1,
                            team: 2,
                            score: 1_000_000,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 0.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0,
                        },
                        MatchScore {
                            player_id: 2,
                            team: 2,
                            score: 803_028,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 0.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0,
                        },
                        MatchScore {
                            player_id: 3,
                            team: 2,
                            score: 723_019,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 0.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0,
                        },
                        MatchScore {
                            player_id: 8,
                            team: 1,
                            score: 639_200,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 0.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0,
                        },
                        MatchScore {
                            player_id: 9,
                            team: 1,
                            score: 620_109,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 0.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0,
                        },
                        MatchScore {
                            player_id: 10,
                            team: 1,
                            score: 500_012,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 0.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0,
                        },
                        MatchScore {
                            player_id: 11,
                            team: 1,
                            score: 300_120,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 0.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0,
                        },
                    ],
                },
                Game {
                    id: 2,
                    play_mode: Mode::Osu,
                    scoring_type: ScoringType::ScoreV2,
                    team_type: TeamType::TeamVs,
                    mods: 1, // NF
                    game_id: 1002340239,
                    start_time,
                    end_time,
                    beatmap: Some(beatmap.clone()),
                    match_scores: vec![
                        MatchScore {
                            player_id: 12,
                            team: 1,
                            score: 1_020_480,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 0.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0,
                        },
                        MatchScore {
                            player_id: 13,
                            team: 1,
                            score: 1_000_000,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 0.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0,
                        },
                        MatchScore {
                            player_id: 14,
                            team: 1,
                            score: 803_028,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 0.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0,
                        },
                        MatchScore {
                            player_id: 15,
                            team: 1,
                            score: 723_019,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 0.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0,
                        },
                        MatchScore {
                            player_id: 4,
                            team: 2,
                            score: 639_200,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 0.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0,
                        },
                        MatchScore {
                            player_id: 5,
                            team: 2,
                            score: 620_109,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 0.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0,
                        },
                        MatchScore {
                            player_id: 6,
                            team: 2,
                            score: 500_012,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 0.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0,
                        },
                        MatchScore {
                            player_id: 7,
                            team: 2,
                            score: 300_120,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 0.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0,
                        },
                    ],
                },
                Game {
                    id: 3,
                    play_mode: Mode::Osu,
                    scoring_type: ScoringType::ScoreV2,
                    team_type: TeamType::TeamVs,
                    mods: 1, // NF
                    game_id: 1002340240,
                    start_time,
                    end_time,
                    beatmap: Some(beatmap.clone()),
                    match_scores: vec![
                        MatchScore {
                            player_id: 3,
                            team: 2,
                            score: 1_020_480,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 0.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0,
                        },
                        MatchScore {
                            player_id: 4,
                            team: 2,
                            score: 1_000_000,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 0.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0,
                        },
                        MatchScore {
                            player_id: 5,
                            team: 2,
                            score: 803_028,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 0.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0,
                        },
                        MatchScore {
                            player_id: 6,
                            team: 2,
                            score: 723_019,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 0.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0,
                        },
                        MatchScore {
                            player_id: 10,
                            team: 1,
                            score: 639_200,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 0.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0,
                        },
                        MatchScore {
                            player_id: 11,
                            team: 1,
                            score: 620_109,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 0.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0,
                        },
                        MatchScore {
                            player_id: 12,
                            team: 1,
                            score: 500_012,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 0.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0,
                        },
                        MatchScore {
                            player_id: 13,
                            team: 1,
                            score: 300_120,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 0.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0,
                        },
                    ],
                },
            ],
        };

        // =====================
        // END MATCH DEFINITION
        // =====================

        let mut match_costs = super::match_costs(&fake_match.games).unwrap();
        matches.push(fake_match);

        // Sort `match_costs` in descending order based on `match_cost`, so higher values (better performance) come first.
        match_costs.sort_by(|a, b| b.match_cost.partial_cmp(&a.match_cost).unwrap());

        // Generate player IDs and rankings based on the new order.
        let player_ids: Vec<_> = match_costs.iter().map(|x| x.player_id).collect();

        // Generate rankings: best performance (highest `match_cost`) gets rank 1, and so on.
        let rankings: Vec<_> = match_costs.iter().rev().map(|x| (x.match_cost * 1000.0) as usize).collect();

        // Prepare teams based on the sorted `player_ids` and their initial ratings.
        let teams: Vec<Vec<_>> = player_ids
            .iter()
            .map(|id| {
                vec![initial_ratings
                    .iter()
                    .find(|x| x.player_id == *id)
                    .unwrap()
                    .rating
                    .clone()]
            })
            .collect();

        // Calculate expected ratings using the openskill model.
        let expected_ratings = model.rate(teams, rankings);
        let actual_ratings = calc_ratings(&initial_ratings, &country_mapping, &matches, &model);

        // Check if the expected ratings match the actual ratings.
        // Assuming `expected_ratings` and `actual_ratings.base_ratings` are available
        // and `player_ids` contains the IDs in the order used for calculating `expected_ratings`.

        // Iterate through `player_ids` to ensure we're comparing the correct players.
        for (index, player_id) in player_ids.iter().enumerate() {
            // Find the actual rating for the player.
            let actual_rating = actual_ratings
                .base_ratings
                .iter()
                .find(|r| r.player_id == *player_id)
                .expect("Player ID should exist");

            // Retrieve the expected rating for this player based on the order they were processed.
            let expected_rating = &expected_ratings[index][0];

            // Compare mu values
            assert!(
                (expected_rating.mu - actual_rating.rating.mu).abs() < f64::EPSILON,
                "Player {}'s mu is {}, should be {}",
                player_id,
                actual_rating.rating.mu,
                expected_rating.mu
            );

            // Compare sigma values
            assert!(
                (expected_rating.sigma - actual_rating.rating.sigma).abs() < f64::EPSILON,
                "Player {}'s sigma is {}, should be {}",
                player_id,
                actual_rating.rating.sigma,
                expected_rating.sigma
            );
        }
    }
}
