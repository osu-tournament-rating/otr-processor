mod constants;
mod data_processing;
mod decay;
mod recalc_helpers;
pub mod structures;

use crate::{
    api::api_structs::{Game, Match, MatchRatingStats, MatchScore, Player, PlayerCountryMapping, RatingAdjustment},
    model::{
        decay::{is_decay_possible, DecayTracker},
        structures::{
            match_cost::MatchCost, mode::Mode, player_rating::PlayerRating,
            rating_calculation_result::RatingCalculationResult, team_type::TeamType
        }
    },
    utils::progress_utils::progress_bar
};
use chrono::Utc;
use openskill::{
    model::{model::Model, plackett_luce::PlackettLuce},
    rating::{default_gamma, Rating}
};
use statrs::{
    distribution::{ContinuousCDF, Normal},
    statistics::Statistics
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

    println!("Processing initial ratings...");
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
                    Mode::Mania => player.earliest_mania_global_rank.or(player.rank_mania)
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
                    rating
                };
                ratings.push(player_rating);

                stored_lookup_log.insert((score.player_id, mode));
            }
        }

        bar.inc(1);
    }

    ratings
}

pub fn hash_country_mappings(country_mappings: &Vec<PlayerCountryMapping>) -> HashMap<i32, Option<String>> {
    let mut country_mappings_hash: HashMap<i32, Option<String>> = HashMap::new();

    for c in country_mappings {
        country_mappings_hash.insert(c.playerId, c.country.clone());
    }

    country_mappings_hash
}

/// Calculates a vector of initial ratings based on match cost,
/// returns the new ratings
pub fn calc_ratings(
    initial_ratings: &Vec<PlayerRating>,
    country_mappings: &Vec<PlayerCountryMapping>,
    matches: &Vec<Match>,
    model: &PlackettLuce
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
    // Key = player_id
    // Value = Country code in string form
    let country_mappings_hash = hash_country_mappings(country_mappings);

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
    println!("Calculating all ratings...");
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
            None => continue
        };
        // Start time of the match
        // Skip the match if not defined
        let start_time = match curr_match.start_time {
            Some(t) => t,
            None => continue
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
                Some(rate) => rate.clone()
            };
            // If decay is possible, apply it to rating_prior
            if is_decay_possible(rating_prior.rating.mu) {
                let adjustment = decay_tracker.decay(
                    match_cost.player_id,
                    curr_match.mode,
                    rating_prior.rating.mu,
                    rating_prior.rating.sigma,
                    start_time
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
                    None => ()
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
                        None => continue
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

            // Use this set of ratings to determine global ranks
            let prior_ratings: Vec<PlayerRating> = ratings_hash
                .iter()
                .map(|(_, v)| v)
                .filter(|x| x.mode == curr_match.mode)
                .cloned() // This will clone each `&PlayerRating` to `PlayerRating`
                .collect();

            let global_rank_before = get_global_rank(&rating_prior.rating.mu, &rating_prior.player_id, &&prior_ratings);
            let country_rank_before = get_country_rank(
                &rating_prior.rating.mu,
                &rating_prior.player_id,
                &&country_mappings_hash,
                &&prior_ratings
            );
            let percentile_before = get_percentile(global_rank_before, prior_ratings.len() as i32);

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
                average_opponent_rating: average_o_rating
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
        let ranks: Vec<usize> = ranks_from_match_costs(&match_costs);
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

        let current_ratings: Vec<PlayerRating> = ratings_hash
            .iter()
            .map(|x| x.1)
            .filter(|x| x.mode == curr_match.mode)
            .cloned()
            .collect();

        for mc in match_costs {
            let curr_id = mc.player_id;
            let key = (mc.player_id, curr_match.mode);
            let new_rating = ratings_hash.get_mut(&key).unwrap();

            if new_rating.rating.mu < 100.0 {
                new_rating.rating.mu = 100.0;
            }

            let global_rank_after = get_global_rank(&new_rating.rating.mu, &new_rating.player_id, &&current_ratings);
            let country_rank_after = get_country_rank(
                &new_rating.rating.mu,
                &new_rating.player_id,
                &&country_mappings_hash,
                &&current_ratings
            );
            let percentile_after = get_percentile(global_rank_after, current_ratings.len() as i32);

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
                None => None
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
        adjustments
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
    country_mappings_hash: &&HashMap<i32, Option<String>>,
    existing_ratings: &&Vec<PlayerRating>
) -> i32 {
    let mut ratings: Vec<f64> = existing_ratings
        .clone()
        .iter()
        .filter(|r| {
            r.player_id != *player_id && country_mappings_hash.get(player_id) == country_mappings_hash.get(&r.player_id)
        })
        .filter(|r| country_mappings_hash.get(&r.player_id).is_some())
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

/// Returns a vector of rankings as follows:
/// - Minimum match cost has a rank equal to the size of the collection.
/// - Maximum match cost has a rank of 1.
///
/// The lower the rank, the better. The results are returned in the
/// same order as the input vector.
fn ranks_from_match_costs(match_costs: &Vec<MatchCost>) -> Vec<usize> {
    let mut ranks = vec![0; match_costs.len()];
    let mut sorted_indices = (0..match_costs.len()).collect::<Vec<_>>();

    // Sort indices based on match_cost, preserving original order in case of ties
    sorted_indices.sort_by(|&a, &b| {
        match_costs[a]
            .match_cost
            .partial_cmp(&match_costs[b].match_cost)
            .unwrap()
    });

    // Assign ranks based on sorted positions, with the minimum match cost getting the highest rank
    for (rank, &idx) in sorted_indices.iter().enumerate() {
        ranks[idx] = match_costs.len() - rank;
    }

    ranks
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
    teammate: &mut Vec<f64>
) {
    for id in teammate_list {
        let teammate_id = id;
        let mode = curr_match.mode;
        let rating = match ratings_hash.get(&(teammate_id, mode)) {
            Some(r) => r.rating.mu,
            None => todo!("This player is not in the hashmap")
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
            match_cost: result
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
        api::api_structs::{Beatmap, Game, Match, MatchScore, PlayerCountryMapping},
        model::{
            calc_ratings, mu_for_rank,
            structures::{
                match_cost::MatchCost, mode::Mode, player_rating::PlayerRating, scoring_type::ScoringType,
                team_type::TeamType
            }
        }
    };
    use openskill::{model::model::Model, rating::Rating};
    use std::{collections::HashMap, ops::Add};

    fn match_from_json(json: &str) -> Match {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn ranks_from_match_costs_returns_correct_scalar() {
        let match_costs = vec![
            MatchCost {
                player_id: 1,
                match_cost: 0.5
            },
            MatchCost {
                player_id: 2,
                match_cost: 0.2
            },
            MatchCost {
                player_id: 3,
                match_cost: 0.7
            },
            MatchCost {
                player_id: 4,
                match_cost: 0.3
            },
            MatchCost {
                player_id: 5,
                match_cost: 2.1
            },
        ];

        let expected = vec![3, 5, 2, 4, 1];
        let value = super::ranks_from_match_costs(&match_costs);

        assert_eq!(expected, value);
    }

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
    fn match_2v2_returns_correct_rating_result() {
        // Read data from /test_data/match_2v2.json
        let mut match_data = match_from_json(include_str!("../../test_data/match_2v2.json"));

        // Override match date to current time to avoid accidental decay
        match_data.start_time = Some(chrono::offset::Utc::now().fixed_offset());
        match_data.end_time = Some(chrono::offset::Utc::now().fixed_offset());

        let match_costs = super::match_costs(&match_data.games).unwrap();
        let ranks = super::ranks_from_match_costs(&match_costs);

        let player_ids = match_costs.iter().map(|mc| mc.player_id).collect::<Vec<i32>>();
        let mut initial_ratings = vec![];
        let mut country_mappings: Vec<PlayerCountryMapping> = vec![];

        let mut offset = 0.0;
        for id in player_ids {
            initial_ratings.push(PlayerRating {
                player_id: id,
                mode: Mode::Osu,
                rating: Rating {
                    mu: 1500.0 + offset,
                    sigma: 200.0
                }
            });
            country_mappings.push(PlayerCountryMapping {
                playerId: id,
                country: Some("US".to_string())
            });

            offset += 1.0;
        }

        let country_mappings_hash = super::hash_country_mappings(&country_mappings);
        let model_ratings = initial_ratings.iter().map(|r| vec![r.rating.clone()]).collect();

        let model = super::create_model();

        println!("Model input:");
        println!("Input ratings: {:?}", &model_ratings);
        println!("Input rankings: {:?}", &ranks);
        let expected = model.rate(model_ratings, ranks);

        let result = super::calc_ratings(&initial_ratings, &country_mappings, &vec![match_data], &model);

        println!("Expected outcome:");
        for i in 0..expected.len() {
            let team = expected.get(i).unwrap();

            let mc = match_costs.get(i).unwrap();
            let expected_rating = team.get(0).unwrap();
            let actual_rating = result
                .base_ratings
                .iter()
                .find(|x| x.player_id == mc.player_id)
                .unwrap();

            println!("Player id: {} Rating: {}", mc.player_id, expected_rating);

            assert!(
                (expected_rating.mu - actual_rating.rating.mu).abs() < f64::EPSILON,
                "Expected rating mu: {}, got: {}",
                expected_rating.mu,
                actual_rating.rating.mu
            );
            assert!(
                (expected_rating.sigma - actual_rating.rating.sigma).abs() < f64::EPSILON,
                "Expected rating sigma: {}, got: {}",
                expected_rating.sigma,
                actual_rating.rating.sigma
            );
        }

        println!("Actual outcome:");
        for stat in &result.base_ratings {
            println!("Player id: {} Rating: {}", stat.player_id, &stat.rating);
        }

        // Test stats
        for stat in &result.rating_stats {
            let player_id = stat.player_id;
            let expected_starting_rating = initial_ratings.iter().find(|x| x.player_id == player_id).unwrap();
            let expected_evaluation = expected
                .iter()
                .find(|x| {
                    x[0].mu
                        == result
                            .base_ratings
                            .iter()
                            .find(|y| y.player_id == player_id)
                            .unwrap()
                            .rating
                            .mu
                })
                .unwrap()
                .get(0)
                .unwrap();

            let expected_starting_mu = expected_starting_rating.rating.mu;
            let expected_starting_sigma = expected_starting_rating.rating.sigma;

            let expected_after_mu = expected_evaluation.mu;
            let expected_after_sigma = expected_evaluation.sigma;

            let expected_mu_change = expected_after_mu - expected_starting_mu;
            let expected_sigma_change = expected_after_sigma - expected_starting_sigma;

            let expected_global_rank_before =
                super::get_global_rank(&expected_starting_mu, &player_id, &&initial_ratings);
            let expected_country_rank_before = super::get_country_rank(
                &expected_starting_mu,
                &player_id,
                &&country_mappings_hash,
                &&initial_ratings
            );
            let expected_percentile_before =
                super::get_percentile(expected_global_rank_before, initial_ratings.len() as i32);
            let expected_global_rank_after =
                super::get_global_rank(&expected_after_mu, &player_id, &&result.base_ratings);
            let expected_country_rank_after = super::get_country_rank(
                &expected_after_mu,
                &player_id,
                &&country_mappings_hash,
                &&result.base_ratings
            );
            let expected_percentile_after =
                super::get_percentile(expected_global_rank_after, result.base_ratings.len() as i32);

            let expected_global_rank_change = expected_global_rank_after - expected_global_rank_before;
            let expected_country_rank_change = expected_country_rank_after - expected_country_rank_before;
            let expected_percentile_change = expected_percentile_after - expected_percentile_before;

            let actual_starting_mu = stat.rating_before;
            let actual_starting_sigma = stat.volatility_before;

            let actual_after_mu = stat.rating_after;
            let actual_after_sigma = stat.volatility_after;

            let actual_mu_change = stat.rating_change;
            let actual_sigma_change = stat.volatility_change;

            let actual_global_rank_before = stat.global_rank_before;
            let actual_country_rank_before = stat.country_rank_before;
            let actual_percentile_before = stat.percentile_before;

            let actual_global_rank_after = stat.global_rank_after;
            let actual_country_rank_after = stat.country_rank_after;
            let actual_percentile_after = stat.percentile_after;

            let actual_global_rank_change = stat.global_rank_change;
            let actual_country_rank_change = stat.country_rank_change;
            let actual_percentile_change = stat.percentile_change;

            assert_eq!(expected_starting_mu, actual_starting_mu);
            assert_eq!(expected_starting_sigma, actual_starting_sigma);
            assert_eq!(expected_after_mu, actual_after_mu);
            assert_eq!(expected_after_sigma, actual_after_sigma);
            assert_eq!(expected_mu_change, actual_mu_change);
            assert_eq!(expected_sigma_change, actual_sigma_change);
            assert_eq!(expected_global_rank_before, actual_global_rank_before);
            assert_eq!(expected_country_rank_before, actual_country_rank_before);
            assert_eq!(expected_percentile_before, actual_percentile_before);
            assert_eq!(expected_global_rank_after, actual_global_rank_after);
            assert_eq!(expected_country_rank_after, actual_country_rank_after);
            assert_eq!(expected_percentile_after, actual_percentile_after);
            assert_eq!(expected_global_rank_change, actual_global_rank_change);
            assert_eq!(expected_country_rank_change, actual_country_rank_change);
            assert_eq!(expected_percentile_change, actual_percentile_change);
        }
    }

    #[test]
    fn test_calc_ratings_1v1() {
        let mut initial_ratings = Vec::new();
        let mut country_mappings: Vec<PlayerCountryMapping> = Vec::new();

        // Set both players to be from the same country to check country rankings
        // Create 2 players with default ratings
        for i in 0..2 {
            country_mappings.push(PlayerCountryMapping {
                playerId: i,
                country: Some("US".to_string())
            });

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
                    sigma: 200.0
                }
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
            accuracy_mania: 0.0
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
            accuracy_mania: 0.0
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
            mods: 0
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
            games
        };

        matches.push(match_instance);

        let loser_id = 0;
        let winner_id = 1;

        let model = super::create_model();
        let expected_outcome = model.rate(
            vec![
                vec![Rating {
                    mu: 1500.0,
                    sigma: 200.0
                }],
                vec![Rating {
                    mu: 1499.0,
                    sigma: 200.0
                }],
            ],
            vec![winner_id, loser_id]
        );

        let loser_expected_outcome = &expected_outcome[loser_id][0];
        let winner_expected_outcome = &expected_outcome[winner_id][0];

        let result = calc_ratings(&initial_ratings, &country_mappings, &matches, &model);
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
            (loser_expected_outcome.mu - loser_stats.rating_after).abs() < f64::EPSILON,
            "Loser's rating is {}, should be {}",
            loser_stats.rating_after,
            loser_expected_outcome.mu
        );
        assert!(
            (loser_expected_outcome.sigma - loser_stats.volatility_after).abs() < f64::EPSILON,
            "Loser's volatility is {}, should be {}",
            loser_stats.volatility_after,
            loser_expected_outcome.sigma
        );

        // Expected sigma = actual sigma
        assert!(
            (winner_expected_outcome.mu - winner_stats.rating_after).abs() < f64::EPSILON,
            "Winner's rating is {}, should be {}",
            winner_stats.rating_after,
            winner_expected_outcome.mu
        );
        assert!(
            (winner_expected_outcome.sigma - winner_stats.volatility_after).abs() < f64::EPSILON,
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
            diff_name: Some("Testing".to_string())
        }
    }
}
