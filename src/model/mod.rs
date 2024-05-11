use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    ops::Index
};

use chrono::Utc;
use itertools::Itertools;
use openskill::{
    model::{model::Model, plackett_luce::PlackettLuce},
    rating::{default_gamma, Rating}
};
use statrs::{
    distribution::{ContinuousCDF, Normal},
    statistics::Statistics
};

use crate::{
    api::api_structs::{
        Game, GameWinRecord, Match, MatchRatingStats, MatchScore, MatchWinRecord, Player, PlayerCountryMapping,
        PlayerMatchStats, RatingAdjustment
    },
    model::{
        constants::BLUE_TEAM_ID,
        decay::{is_decay_possible, DecayTracker},
        structures::{
            match_cost::MatchCost, mode::Mode, player_rating::PlayerRating, processing::RatingCalculationResult,
            team_type::TeamType
        }
    },
    utils::progress_utils::progress_bar
};

use self::{
    constants::RED_TEAM_ID,
    structures::{
        match_type::MatchType,
        processing::{PlayerMatchData, ProcessedMatchData}
    }
};

/// The flow of processor
mod constants;
mod data_processing;
mod decay;
mod recalc_helpers;
pub mod structures;

pub fn create_model() -> PlackettLuce {
    PlackettLuce::new(constants::BETA, constants::KAPPA, default_gamma)
}

/// Calculates [`RatingAdjustment`] based on initial ratings
/// and new ratings (after all match changes applied)
pub fn calculate_player_adjustments(
    initial_ratings: &[PlayerRating],
    new_ratings: &[PlayerRating]
) -> Vec<RatingAdjustment> {
    let mut buff = Vec::with_capacity(new_ratings.len());

    for new_rating in new_ratings.iter() {
        let old_rating_idx = initial_ratings.iter().position(|x| x.player_id == new_rating.player_id);

        if old_rating_idx.is_none() {
            // TODO log here?
            continue;
        }

        let old_rating_idx = old_rating_idx.unwrap();

        let old_rating = &initial_ratings[old_rating_idx];

        let rating_before = old_rating.rating.mu;
        let rating_after = new_rating.rating.mu;
        let volatility_before = old_rating.rating.sigma;
        let volatility_after = new_rating.rating.sigma;

        let rating_change = rating_after - rating_before;
        let volatility_change = volatility_after - volatility_before;

        buff.push(RatingAdjustment {
            player_id: new_rating.player_id,
            mode: new_rating.mode,
            rating_adjustment_amount: rating_change,
            volatility_adjustment_amount: volatility_change,
            rating_before,
            rating_after,
            volatility_before,
            volatility_after,
            rating_adjustment_type: 0,
            timestamp: Utc::now().into()
        })
    }

    buff
}

/// Calculates [`MatchRatingStats`] based on initial ratings
/// and match adjustments
pub fn calculate_post_match_info(
    initial_ratings: &mut [PlayerRating],
    match_data: &mut [ProcessedMatchData]
) -> Vec<MatchRatingStats> {
    println!("Calculating post match info...");
    let bar = progress_bar(match_data.len() as u64);
    let mut res = Vec::with_capacity(match_data.len());

    // Calculating leadearboard for all modes
    calculate_global_ranks(initial_ratings, Mode::Osu);
    calculate_global_ranks(initial_ratings, Mode::Mania);
    calculate_global_ranks(initial_ratings, Mode::Taiko);
    calculate_global_ranks(initial_ratings, Mode::Catch);

    calculate_country_ranks(initial_ratings, Mode::Osu);
    calculate_country_ranks(initial_ratings, Mode::Mania);
    calculate_country_ranks(initial_ratings, Mode::Taiko);
    calculate_country_ranks(initial_ratings, Mode::Catch);

    for match_info in match_data.iter_mut() {
        // Preparing initial_ratings with new rating
        // and extracting old country/global ranking placements
        for player_info in &mut match_info.players_stats {
            let player_idx = initial_ratings
                .iter_mut()
                .position(|x| x.player_id == player_info.player_id && x.mode == match_info.mode);

            if player_idx.is_none() {
                continue;
            }

            let player_idx = player_idx.unwrap();

            let player = &mut initial_ratings[player_idx];

            player.rating = player_info.new_rating.clone();
            player_info.old_global_ranking = player.global_ranking;
            player_info.old_country_ranking = player.country_ranking;
        }

        // Calculating rankings after match adjustments
        calculate_global_ranks(initial_ratings, match_info.mode);
        calculate_country_ranks(initial_ratings, match_info.mode);

        for player_info in &mut match_info.players_stats {
            let player_idx = initial_ratings
                .iter_mut()
                .position(|x| x.player_id == player_info.player_id && x.mode == match_info.mode);

            if player_idx.is_none() {
                continue;
            }

            let player_idx = player_idx.unwrap();

            let player = &mut initial_ratings[player_idx];

            player_info.new_global_ranking = player.global_ranking;
            player_info.new_country_ranking = player.country_ranking;
        }

        bar.inc(1);
    }
    bar.finish();

    println!("Calculating rating stats...");
    let bar2 = progress_bar(match_data.len() as u64);
    // Casting it to MatchRatingStats since we have all neccessary data
    for match_info in match_data.iter() {
        match_info
            .players_stats
            .iter()
            .map(|x| {
                let p_before = x.old_global_ranking as f64 / initial_ratings.len() as f64;
                let p_after = x.new_global_ranking as f64 / initial_ratings.len() as f64;

                bar2.inc(1);
                MatchRatingStats {
                    player_id: x.player_id,
                    match_id: match_info.match_id,
                    match_cost: x.match_cost,
                    rating_before: x.old_rating.mu,
                    rating_after: x.new_rating.mu,
                    rating_change: x.new_rating.mu - x.old_rating.mu,
                    volatility_before: x.old_rating.sigma,
                    volatility_after: x.new_rating.sigma,
                    volatility_change: x.new_rating.sigma - x.old_rating.sigma,
                    global_rank_before: x.old_global_ranking as i32,
                    global_rank_after: x.new_global_ranking as i32,
                    global_rank_change: x.new_global_ranking as i32 - x.old_global_ranking as i32,
                    country_rank_before: x.old_country_ranking as i32,
                    country_rank_after: x.new_country_ranking as i32,
                    country_rank_change: x.new_country_ranking as i32 - x.old_country_ranking as i32,
                    percentile_before: p_before,
                    percentile_after: p_after,
                    percentile_change: p_after - p_before,
                    average_teammate_rating: x.average_teammate_rating,
                    average_opponent_rating: x.average_opponent_rating
                }
            })
            .for_each(|x| res.push(x));
    }

    bar2.finish();
    res
}

/// Calculates global ranking based on [`PlayerRating::rating`] field
///
/// # Notes
/// Modifies and invalidate any existing sorting in [`PlayerRating`] slice
pub fn calculate_global_ranks(existing_ratings: &mut [PlayerRating], mode: Mode) {
    existing_ratings.sort_by(|x, y| {
        if y.mode != mode {
            Ordering::Less
        } else {
            Ordering::Greater
        }
    });

    // According to previous sorting we can make the assumption that
    // first element is always start of current gamemode slice
    let gamemode_slice_start = 0;

    let gamemode_slice_end = existing_ratings
        .iter()
        .position(|x| x.mode != mode)
        .unwrap_or(existing_ratings.len());

    let gamemode_slice = &mut existing_ratings[gamemode_slice_start..gamemode_slice_end];

    gamemode_slice.sort_by(|x, y| y.rating.mu.partial_cmp(&x.rating.mu).unwrap());

    gamemode_slice.iter_mut().enumerate().for_each(|(i, plr)| {
        if plr.mode == mode {
            plr.global_ranking = i as u32 + 1
        }
    });
}

/// Calculates country ranking for each individual player
/// based on [`PlayerRating::rating`] field
///
/// # Notes
/// Modifies and invalidate any existing sorting in [`PlayerRating`] slice
pub fn calculate_country_ranks(existing_ratings: &mut [PlayerRating], mode: Mode) {
    let mut countries = HashSet::new();

    // Country ranking
    existing_ratings.iter().map(|x| x.country.clone()).for_each(|x| {
        countries.insert(x);
    });

    existing_ratings.sort_by(|x, y| {
        if x.mode != mode {
            Ordering::Less
        } else {
            Ordering::Greater
        }
    });

    // Finding gamemode slice
    let gamemode_start = match existing_ratings.iter().position(|x| x.mode == mode) {
        Some(v) => v,
        None => return
    };

    let gamemode_slice = &mut existing_ratings[gamemode_start..];

    let gamemode_end = gamemode_slice
        .iter()
        .position(|x| x.mode != mode)
        .unwrap_or(gamemode_slice.len());

    let gamemode_slice = &mut gamemode_slice[..gamemode_end];

    for country in countries {
        // TODO
        let country_start = gamemode_slice.iter().position(|x| x.country == country);

        if country_start.is_none() {
            continue;
        }

        let country_start = country_start.unwrap();

        let country_slice = &mut gamemode_slice[country_start..];

        let country_end = country_slice
            .iter()
            .position(|x| x.country != country)
            .unwrap_or(country_slice.len());

        let country_slice = &mut country_slice[..country_end];

        // Descending
        country_slice.sort_by(|x, y| y.rating.mu.partial_cmp(&x.rating.mu).unwrap());

        country_slice
            .iter_mut()
            .enumerate()
            .for_each(|(i, plr)| plr.country_ranking = i as u32 + 1);
    }
}

pub fn create_initial_ratings(matches: &Vec<Match>, players: &Vec<Player>) -> Vec<PlayerRating> {
    // The first step in the rating algorithm. Generate ratings from known ranks.

    // A fast lookup used for understanding who has default ratings created at this time.
    let mut stored_lookup_log: HashSet<(i32, Mode)> = HashSet::new();
    let mut ratings: Vec<PlayerRating> = Vec::new();

    println!("Processing initial ratings...");
    let bar = progress_bar(matches.len() as u64);

    // Map the osu ids for fast lookup
    let mut player_hashmap: HashMap<i32, Player> = HashMap::with_capacity(players.len());

    for player in players {
        player_hashmap.entry(player.id).or_insert(player.clone());
    }

    for m in matches {
        for game in &m.games {
            let mode = game.ruleset;

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
                    rating,
                    global_ranking: 0,
                    country_ranking: 0,
                    country: player.country.clone().unwrap_or(String::with_capacity(2))
                };

                ratings.push(player_rating);
                stored_lookup_log.insert((score.player_id, mode));
            }
        }

        bar.inc(1);
    }
    bar.finish();

    ratings
}

pub fn hash_country_mappings(country_mappings: &[PlayerCountryMapping]) -> HashMap<i32, Option<String>> {
    let mut country_mappings_hash: HashMap<i32, Option<String>> = HashMap::with_capacity(country_mappings.len());

    for c in country_mappings {
        country_mappings_hash.insert(c.player_id, c.country.clone());
    }

    country_mappings_hash
}

pub fn calculate_ratings(
    initial_ratings: Vec<PlayerRating>,
    matches: &[Match],
    model: &PlackettLuce
) -> RatingCalculationResult {
    let mut copied_ratings = initial_ratings.clone();

    let (mut match_data, adj) = calculate_processed_match_data(&copied_ratings, matches, model);
    let match_info = calculate_post_match_info(&mut copied_ratings, &mut match_data);
    let (match_wrs, game_wrs) = calculate_match_win_records(&matches);

    RatingCalculationResult {
        base_ratings: copied_ratings,
        rating_stats: match_info,
        adjustments: adj,
        processed_data: match_data,
        game_win_records: game_wrs,
        match_win_records: match_wrs
    }
}

fn calculate_game_win_records(matches: &[Match]) -> Vec<GameWinRecord> {
    let mut res = Vec::new();

    // Iterate over matches and their games, then push the game win records to the result vector
    for m in matches {
        for g in &m.games {
            res.push(game_win_record(g));
        }
    }

    res
}

fn calculate_match_win_records(matches: &[Match]) -> (Vec<MatchWinRecord>, Vec<GameWinRecord>) {
    let mut mwrs = Vec::new();
    let mut gwrs_final = Vec::new();

    for m in matches {
        let mut gwrs = Vec::new();

        for g in &m.games {
            let gwr = game_win_record(&g);
            gwrs.push(gwr.clone());
            gwrs_final.push(gwr.clone());
        }

        // Calculate match win record
        mwrs.push(match_win_record_from_game_win_records(m.id, &gwrs));
    }

    (mwrs, gwrs_final)
}

fn match_win_record_from_game_win_records(match_id: i32, game_win_records: &[GameWinRecord]) -> MatchWinRecord {
    let mut red_roster = Vec::new(); // Winner of head to head or team red
    let mut blue_roster = Vec::new(); // Loser of head to head or team blue

    let mut red_points = 0; // Winner of head to head or team red
    let mut blue_points = 0; // Loser of head to head or team blue

    let mut count_h2h = 0;
    let mut count_teamvs = 0;

    for gwr in game_win_records {
        if gwr.winner_team == 0 {
            count_h2h += 1;
        } else {
            count_teamvs += 1;
        }
    }

    let match_type = if count_h2h > count_teamvs {
        MatchType::HeadToHead
    } else {
        MatchType::Team
    };

    for gwr in game_win_records {
        match match_type {
            MatchType::Team => {
                if gwr.winner_team == 0 || gwr.loser_team == 0 {
                    panic!("Team based match type with head to head is unsupported!")
                }

                if gwr.winner_team == BLUE_TEAM_ID {
                    blue_points += 1;
                    blue_roster.extend(gwr.winners.clone());
                    red_roster.extend(gwr.losers.clone());
                }

                if gwr.winner_team == RED_TEAM_ID {
                    red_points += 1;
                    red_roster.extend(gwr.winners.clone());
                    blue_roster.extend(gwr.losers.clone());
                }
            }
            MatchType::HeadToHead => {
                if gwr.winner_team != 0 || gwr.loser_team != 0 {
                    panic!("Head to head with team based match type is unsupported!")
                }

                if gwr.winners.len() > 1 || gwr.losers.len() > 1 {
                    panic!("Head to head with more than 1 member per team is unsupported!");
                }

                // Set the winner to team red
                if red_roster.is_empty() {
                    red_roster = gwr.winners.clone();
                    blue_roster = gwr.losers.clone();
                }

                // Compare the scores based on id
                if gwr.winners == red_roster {
                    red_points += 1;
                }

                if gwr.winners == blue_roster {
                    blue_points += 1;
                }
            }
        }
    }

    red_roster = red_roster.into_iter().unique().collect();
    blue_roster = blue_roster.into_iter().unique().collect();

    let (mut winner_team, mut loser_team) = match red_points.cmp(&blue_points) {
        Ordering::Greater => (Some(RED_TEAM_ID), Some(BLUE_TEAM_ID)),
        Ordering::Less => (Some(BLUE_TEAM_ID), Some(RED_TEAM_ID)),
        Ordering::Equal => {
            return MatchWinRecord {
                match_id,
                loser_roster: blue_roster,
                winner_roster: red_roster,
                winner_points: red_points,
                loser_points: blue_points,
                winner_team: None,
                loser_team: None,
                match_type: Some(match_type)
            };
        },
    };

    // Identify winning & losing rosters. If tie, default to red.
    // In a head to head, the winning player is always red.

    let (winner_roster, loser_roster) = match (winner_team, loser_team)  {
        (Some(RED_TEAM_ID), Some(BLUE_TEAM_ID)) => (red_roster, blue_roster),
        (Some(BLUE_TEAM_ID), Some(RED_TEAM_ID)) => (blue_roster, red_roster),
        _ => panic!("Winner and loser teams should only contain RED and BLUE team ids") // Safe to panic here because that's obviously programmer mistake
    };

    let (winner_points, loser_points) = match (winner_team, loser_team)  {
        (Some(RED_TEAM_ID), Some(BLUE_TEAM_ID)) => (red_points, blue_points),
        (Some(BLUE_TEAM_ID), Some(RED_TEAM_ID)) => (blue_points, red_points),
        _ => panic!("Winner and loser teams should only contain RED and BLUE team ids") // Safe to panic here because that's obviously programmer mistake
    };

    if match_type == MatchType::HeadToHead {
        winner_team = Some(0);
        loser_team = Some(0);
    }

    MatchWinRecord {
        match_id,
        loser_roster,
        winner_roster,
        winner_points,
        loser_points,
        winner_team,
        loser_team,
        match_type: Some(match_type)
    }
}

/// For each player in the match, generate one [`PlayerMatchStats`] object.
/// This allows us to identify how each player performed in the match.
fn player_match_stats(matches: &[Match]) -> Vec<PlayerMatchStats> {
    let mut res = Vec::new();

    for m in matches {
        let mut p_ids: Vec<i32> = Vec::new();
        let mut p_scores: HashMap<i32, Vec<i64>> = HashMap::new();
        let mut p_misses: HashMap<i32, Vec<i32>> = HashMap::new();
        let mut p_accs: HashMap<i32, Vec<f64>> = HashMap::new();
        let mut p_placement: HashMap<i32, Vec<i32>> = HashMap::new();
        let mut p_gplayed: HashMap<i32, i32> = HashMap::new();
        let mut p_gwon: HashMap<i32, i32> = HashMap::new();
        let mut p_glost: HashMap<i32, i32> = HashMap::new();

        let gwrs = calculate_game_win_records(std::slice::from_ref(m));
        let mwr = match_win_record_from_game_win_records(m.id, &gwrs);

        let mut g_idx = 0;
        for g in &m.games {
            let mut s_clone = g.match_scores.clone();
            s_clone.sort_by(|a, b| b.score.cmp(&a.score));

            let gwr = &gwrs[g_idx];

            let mut p = 1;
            for s in s_clone {
                let scores = p_scores.entry(s.player_id).or_insert(Vec::new());
                let misses = p_misses.entry(s.player_id).or_insert(Vec::new());
                let accs = p_accs.entry(s.player_id).or_insert(Vec::new());
                let placement = p_placement.entry(s.player_id).or_insert(Vec::new());
                let gplayed = p_gplayed.entry(s.player_id).or_insert(0);
                let gwon = p_gwon.entry(s.player_id).or_insert(0);
                let glost = p_glost.entry(s.player_id).or_insert(0);

                p_ids.push(s.player_id);
                scores.push(s.score);
                misses.push(s.misses);
                accs.push(s.accuracy_standard);
                placement.push(p);
                *gplayed += 1;

                let won = player_won_game(&s.player_id, &gwr);
                if won {
                    *gwon += 1;
                } else {
                    // Ties are technically losses in this case, we can figure this out later.
                    *glost += 1;
                }

                p += 1;
            }

            g_idx += 1;
        }

        p_ids = p_ids.into_iter().unique().collect();

        let winning_roster = mwr.winner_roster.clone();
        let losing_roster = mwr.loser_roster.clone();

        for p_id in p_ids {
            let won = winning_roster.contains(&p_id);
            res.push(PlayerMatchStats {
                player_id: p_id,
                match_id: m.id,
                won,
                average_score: mean_i64(p_scores.entry(p_id).or_insert(Vec::new())),
                average_misses: mean_i32(p_misses.entry(p_id).or_insert(Vec::new())),
                average_accuracy: mean_f64(p_accs.entry(p_id).or_insert(Vec::new())),
                average_placement: mean_i32(p_placement.entry(p_id).or_insert(Vec::new())),
                games_won: *p_gwon.entry(p_id).or_insert(0),
                games_lost: *p_glost.entry(p_id).or_insert(0),
                games_played: *p_gplayed.entry(p_id).or_insert(0),
                teammate_ids: if won {
                    winning_roster
                        .clone()
                        .iter()
                        .filter(|x| **x != p_id)
                        .map(|x| *x)
                        .collect()
                } else {
                    losing_roster
                        .clone()
                        .iter()
                        .filter(|x| **x != p_id)
                        .map(|x| *x)
                        .collect()
                },
                opponent_ids: if won {
                    losing_roster.clone()
                } else {
                    winning_roster.clone()
                }
            });
        }
    }

    res
}

fn mean_i32(numbers: &[i32]) -> f64 {
    let sum: i32 = numbers.iter().sum();
    let count = numbers.len() as f64;
    sum as f64 / count
}

fn mean_f64(numbers: &[f64]) -> f64 {
    let sum: f64 = numbers.iter().sum();
    let count = numbers.len() as f64;
    sum / count
}

fn mean_i64(numbers: &[i64]) -> f64 {
    let sum: i64 = numbers.iter().sum();
    let count = numbers.len() as f64;
    sum as f64 / count
}

fn player_won_game(player_id: &i32, win_record: &GameWinRecord) -> bool {
    win_record.winners.contains(player_id)
}

fn mean(numbers: &[f64]) -> f64 {
    let sum: f64 = numbers.iter().sum();
    let count = numbers.len() as f64;
    sum / count
}

/// Calculates [`ProcessedMatchData`] for each match provided
pub fn calculate_processed_match_data(
    initial_ratings: &[PlayerRating],
    matches: &[Match],
    model: &PlackettLuce
) -> (Vec<ProcessedMatchData>, Vec<RatingAdjustment>) {
    println!("Calculating processed match data...");
    let bar = progress_bar(matches.len() as u64);

    let mut decay_tracker = DecayTracker::new();

    let mut ratings_hash: HashMap<(i32, Mode), PlayerRating> = HashMap::with_capacity(initial_ratings.len());

    // Pointless cloning
    // TODO think of a way to reuse same provided list
    for r in initial_ratings {
        ratings_hash.insert((r.player_id, r.mode), r.clone());
    }

    let mut to_rate = Vec::with_capacity(10);

    let mut matches_stats = Vec::new();
    let mut decays = Vec::new();

    for curr_match in matches {
        let mut current_match_stats = ProcessedMatchData {
            mode: curr_match.mode,
            match_id: curr_match.id,
            players_stats: Vec::new()
        };

        if curr_match.games.iter().any(|game| game.ruleset != curr_match.mode) {
            bar.inc(1);
            continue;
        }

        // Obtain all player match costs
        // Skip the match if there are no valid match costs
        let mut match_costs = match match_costs(&curr_match.games) {
            Some(mc) if !mc.is_empty() => mc,
            _ => {
                bar.inc(1);
                continue;
            }
        };
        // Start time of the match
        // Skip the match if not defined
        // This happens when a match cannot be retrieved by the API properly (i.e. dead link)
        let start_time = match curr_match.start_time {
            Some(t) => t,
            None => {
                bar.inc(1);
                continue;
            }
        };

        // Collection of match ratings
        // Key = player_id
        // Value = MatchRatingStats
        to_rate.clear();

        for match_cost in &match_costs {
            // If user has no prior activity, store the first one
            if decay_tracker
                .get_activity(match_cost.player_id, curr_match.mode)
                .is_none()
            {
                decay_tracker.record_activity(match_cost.player_id, curr_match.mode, start_time);
            }

            // Get user's current rating to use for decay
            let mut rating_prior = match ratings_hash.get_mut(&(match_cost.player_id, curr_match.mode)) {
                None => panic!("No rating found?"),
                Some(rate) => rate.clone()
            };

            // If decay is possible, apply it to rating_prior
            if is_decay_possible(rating_prior.rating.mu) {
                let adjustments = decay_tracker.decay(
                    match_cost.player_id,
                    curr_match.mode,
                    rating_prior.rating.mu,
                    rating_prior.rating.sigma,
                    start_time
                );
                if let Some(adj) = adjustments {
                    rating_prior.rating.mu = adj[adj.len() - 1].rating_after;
                    rating_prior.rating.sigma = adj[adj.len() - 1].volatility_after;

                    // Update the hashmap with the new decay.
                    for a in adj {
                        decays.push(a);
                    }
                };
            }
            to_rate.push(rating_prior.clone());

            // Update hashmap with decay values, if any.
            ratings_hash
                .entry((match_cost.player_id, curr_match.mode))
                .and_modify(|f| {
                    f.rating = rating_prior.rating.clone();
                });

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

            let team_based = if team_based_count == single_count {
                curr_match.games[curr_match.games.len() - 1].team_type != TeamType::HeadToHead
            } else {
                team_based_count > single_count
            };

            let mut teammate_ratings: Option<Vec<_>> = None;
            let mut opponent_ratings: Option<Vec<_>> = None;

            if team_based {
                // Get user's team ID
                let mut curr_player_team = BLUE_TEAM_ID;
                // Find first Game in the Match where the player exists
                for game in &curr_match.games {
                    let game_with_player = game
                        .match_scores
                        .iter()
                        .rfind(|x| x.player_id == rating_prior.player_id);
                    match game_with_player {
                        Some(g) => {
                            curr_player_team = g.team;
                            break;
                        }
                        None => continue
                    }
                }

                // Get IDs of all users in player's team and the opposite team
                let (teammate_list, opponent_list): (Vec<MatchScore>, Vec<MatchScore>) = curr_match
                    .games
                    .iter()
                    .flat_map(|f| f.match_scores.clone())
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

                if !teammates.is_empty() {
                    teammate_ratings = Some(teammates);
                }

                if !opponents.is_empty() {
                    opponent_ratings = Some(opponents);
                }
            }
            // Get average ratings of both teams
            let average_t_rating = average_rating(teammate_ratings);
            let average_o_rating = average_rating(opponent_ratings);
            // Record currently processed match
            // Uses start_time as end_time can be null (issue on osu-web side)
            decay_tracker.record_activity(rating_prior.player_id, curr_match.mode, start_time);

            current_match_stats.players_stats.push(PlayerMatchData {
                player_id: rating_prior.player_id,
                match_cost: match_cost.match_cost,
                old_rating: rating_prior.rating.clone(),
                new_rating: Default::default(),
                old_global_ranking: 0,
                new_global_ranking: 0,
                old_country_ranking: 0,
                new_country_ranking: 0,
                average_opponent_rating: average_o_rating,
                average_teammate_rating: average_t_rating
            })
        }

        // Sort rated players and their matchcosts by player IDs for correct mappings
        to_rate.sort_by(|x, y| x.player_id.cmp(&y.player_id));
        match_costs.sort_by(|x, y| x.player_id.cmp(&y.player_id));

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

        for rate in to_rate.iter() {
            let player_match_stats = current_match_stats
                .players_stats
                .iter_mut()
                .find(|x| x.player_id == rate.player_id)
                .unwrap();

            ratings_hash
                .entry((player_match_stats.player_id, curr_match.mode))
                .and_modify(|x| x.rating = rate.rating.clone());

            player_match_stats.new_rating = rate.rating.clone();
        }

        matches_stats.push(current_match_stats);

        bar.inc(1);
    }
    bar.finish();

    (matches_stats, decays)
}

fn calc_percentile(rank: i32, player_count: i32) -> f64 {
    rank as f64 / player_count as f64
}

pub fn get_country_rank(
    mu: &f64,
    player_id: &i32,
    country_mappings_hash: &HashMap<i32, Option<String>>,
    existing_ratings: &[PlayerRating]
) -> i32 {
    let mut ratings: Vec<f64> = existing_ratings
        .iter()
        .filter(|r| {
            r.player_id != *player_id && country_mappings_hash.get(player_id) == country_mappings_hash.get(&r.player_id)
        })
        .filter(|r| country_mappings_hash.get(&r.player_id).is_some())
        .map(|r| r.rating.mu)
        .collect();

    ratings.push(*mu);
    ratings.sort_by(|x, y| y.partial_cmp(x).unwrap());
    for (rank, mu_iter) in ratings.iter().enumerate() {
        if mu == mu_iter {
            return (rank + 1) as i32;
        }
    }

    0
}

/// Returns a vector of rankings as follows:
/// - Minimum match cost has a rank equal to the size of the collection.
/// - Maximum match cost has a rank of 1.
///
/// The lower the rank, the better. The results are returned in the
/// same order as the input vector.
pub fn ranks_from_match_costs(match_costs: &[MatchCost]) -> Vec<usize> {
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

pub fn get_global_rank(mu: &f64, player_id: &i32, existing_ratings: &[PlayerRating]) -> i32 {
    let mut ratings: Vec<f64> = existing_ratings
        .iter()
        .filter(|r| r.player_id != *player_id)
        .map(|r| r.rating.mu)
        .collect();

    ratings.push(*mu);
    ratings.sort_by(|x, y| y.partial_cmp(x).unwrap());
    for (rank, mu_iter) in ratings.iter().enumerate() {
        if mu == mu_iter {
            return (rank + 1) as i32;
        }
    }

    0
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
    if let Some(rating) = ratings {
        let len = rating.len() as f64;
        let s_ratings: f64 = rating.into_iter().sum();
        Some(s_ratings / len)
    } else {
        None
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

        if result.is_nan() {
            panic!("Match cost cannot be NaN. {}", result);
        }

        let mc = MatchCost {
            player_id,
            match_cost: result
        };

        match_costs.push(mc);
    }

    Some(match_costs)
}

fn game_win_record(game: &Game) -> GameWinRecord {
    let game_id = game.id;
    let (winners, losers, winner_team, loser_team) = identify_game_winners_losers(game);

    GameWinRecord {
        game_id,
        winners,
        losers,
        winner_team,
        loser_team
    }
}

/// Identifies the winners and losers of a game.
/// Return format is tuple of (winner ids, loser ids, winner team, loser team)
fn identify_game_winners_losers(game: &Game) -> (Vec<i32>, Vec<i32>, i32, i32) {
    match game.team_type {
        TeamType::HeadToHead => {
            if game.match_scores.len() != 2 {
                println!("Head to head game must have 2 players: {:?}", game);
            }

            // Head to head
            let [ref score_0, ref score_1] = game.match_scores[0..2] else {
                panic!("Head to head game needs at least two scores!")
            };

            let winners;
            let losers;

            if score_0.score > score_1.score {
                winners = vec![score_0.player_id];
                losers = vec![score_1.player_id];
            } else {
                winners = vec![score_1.player_id];
                losers = vec![score_0.player_id];
            }

            return (winners, losers, 0, 0);
        }
        TeamType::TeamVs => {
            let mut red_players = vec![];
            let mut blue_players = vec![];

            let mut red_scores: Vec<i64> = vec![];
            let mut blue_scores: Vec<i64> = vec![];

            for score in &game.match_scores {
                match score.team {
                    i if i == BLUE_TEAM_ID => {
                        blue_players.push(score.player_id);
                        blue_scores.push(score.score);
                    }
                    i if i == RED_TEAM_ID => {
                        red_players.push(score.player_id);
                        red_scores.push(score.score);
                    }
                    _ => panic!("Invalid team type")
                }
            }

            let red_score: i64 = red_scores.iter().sum();
            let blue_score: i64 = blue_scores.iter().sum();

            if red_score > blue_score {
                (red_players, blue_players, RED_TEAM_ID, BLUE_TEAM_ID)
            } else {
                (blue_players, red_players, BLUE_TEAM_ID, RED_TEAM_ID)
            }
        }
        _ => panic!("Invalid team type")
    }
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
    use std::collections::HashMap;

    use chrono::{FixedOffset, Utc};
    use openskill::{model::model::Model, rating::Rating};

    use crate::{
        api::api_structs::{
            Beatmap, Game, GameWinRecord, Match, MatchScore, MatchWinRecord, Player, PlayerCountryMapping,
            PlayerMatchStats
        },
        model::{
            calc_percentile, calculate_country_ranks, calculate_post_match_info, calculate_ratings,
            constants::{BLUE_TEAM_ID, RED_TEAM_ID},
            mu_for_rank,
            structures::{
                match_cost::MatchCost, match_type::MatchType, mode::Mode, player_rating::PlayerRating,
                scoring_type::ScoringType, team_type::TeamType
            }
        },
        utils::test_utils
    };

    use super::{
        calculate_global_ranks, calculate_match_win_records, calculate_player_adjustments,
        calculate_processed_match_data, create_initial_ratings, create_model, game_win_record, hash_country_mappings,
        player_match_stats
    };

    fn match_from_json(json: &str) -> Match {
        serde_json::from_str(json).unwrap()
    }

    fn matches_from_json(json: &str) -> Vec<Match> {
        serde_json::from_str(json).unwrap()
    }

    fn players_from_json(json: &str) -> Vec<Player> {
        serde_json::from_str(json).unwrap()
    }

    fn country_mapping_from_json(json: &str) -> Vec<PlayerCountryMapping> {
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
        let rank = 1;
        let expected = 1350.0; // The minimum

        let value = mu_for_rank(rank);

        assert_eq!(expected, value);
    }

    #[test]
    fn mu_for_rank_returns_correct_10k() {
        let rank = 10000;
        let expected = 698.7109864354294; // The minimum

        let value = mu_for_rank(rank);

        assert!((expected - value).abs() < 0.000001);
    }

    #[test]
    fn mu_for_rank_returns_correct_500() {
        let rank = 500;
        let expected = 1130.0964338272045; // The minimum

        let value = mu_for_rank(rank);

        assert!((expected - value).abs() < 0.000001);
    }

    #[test]
    fn test_percentile() {
        let percentiles = [0.2, 0.4, 0.6, 0.8, 1.0];
        let ranks = [1, 2, 3, 4, 5];

        for i in 0..percentiles.len() {
            let expected_percentile = percentiles[i];
            let rank = ranks[i];

            // 1.0 5
            let calculated_percentile = calc_percentile(rank, percentiles.len() as i32);
            assert_eq!(calculated_percentile, expected_percentile);
        }
    }

    #[test]
    fn test_match_2v2_data() {
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
                },
                global_ranking: 0,
                country_ranking: 0,
                country: "US".to_string()
            });
            country_mappings.push(PlayerCountryMapping {
                player_id: id,
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

        let result = super::calculate_ratings(initial_ratings.clone(), &[match_data], &model);

        println!("Expected outcome:");
        for i in 0..expected.len() {
            let team = expected.get(i).unwrap();

            let mc = match_costs.get(i).unwrap();
            let expected_rating = team.first().unwrap();
            let actual_rating = result
                .base_ratings
                .iter()
                .find(|x| x.player_id == mc.player_id)
                .unwrap();

            println!("Player id: {} Rating: {}", mc.player_id, expected_rating);

            let constants = test_utils::TestConstants::new();

            assert!(
                (expected_rating.mu - actual_rating.rating.mu).abs() < constants.open_skill_leniency,
                "Expected rating mu: {}, got: {}",
                expected_rating.mu,
                actual_rating.rating.mu
            );
            assert!(
                (expected_rating.sigma - actual_rating.rating.sigma).abs() < constants.open_skill_leniency,
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
                .first()
                .unwrap();

            let expected_starting_mu = expected_starting_rating.rating.mu;
            let expected_starting_sigma = expected_starting_rating.rating.sigma;

            let expected_after_mu = expected_evaluation.mu;
            let expected_after_sigma = expected_evaluation.sigma;

            let expected_mu_change = expected_after_mu - expected_starting_mu;
            let expected_sigma_change = expected_after_sigma - expected_starting_sigma;

            let expected_global_rank_before =
                super::get_global_rank(&expected_starting_mu, &player_id, &initial_ratings);
            let expected_country_rank_before = super::get_country_rank(
                &expected_starting_mu,
                &player_id,
                &country_mappings_hash,
                &initial_ratings
            );
            let expected_percentile_before =
                super::calc_percentile(expected_global_rank_before, initial_ratings.len() as i32);
            let expected_global_rank_after =
                super::get_global_rank(&expected_after_mu, &player_id, &result.base_ratings);
            let expected_country_rank_after = super::get_country_rank(
                &expected_after_mu,
                &player_id,
                &country_mappings_hash,
                &result.base_ratings
            );
            let expected_percentile_after =
                super::calc_percentile(expected_global_rank_after, result.base_ratings.len() as i32);

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

            assert!((expected_starting_mu - actual_starting_mu).abs() < f64::EPSILON);
            assert!((expected_starting_sigma - actual_starting_sigma).abs() < f64::EPSILON);
            assert!((expected_after_mu - actual_after_mu).abs() < f64::EPSILON);
            assert!((expected_after_sigma - actual_after_sigma).abs() < f64::EPSILON);
            assert!((expected_mu_change - actual_mu_change).abs() < f64::EPSILON);
            assert!((expected_sigma_change - actual_sigma_change).abs() < f64::EPSILON);
            assert_eq!(expected_global_rank_before, actual_global_rank_before);
            assert_eq!(expected_country_rank_before, actual_country_rank_before);
            assert!((expected_percentile_before - actual_percentile_before).abs() < f64::EPSILON);
            assert_eq!(expected_global_rank_after, actual_global_rank_after);
            assert_eq!(expected_country_rank_after, actual_country_rank_after);
            assert!((expected_percentile_after - actual_percentile_after).abs() < f64::EPSILON);
            assert_eq!(expected_global_rank_change, actual_global_rank_change);
            assert_eq!(expected_country_rank_change, actual_country_rank_change);
            assert!((expected_percentile_change - actual_percentile_change).abs() < f64::EPSILON);
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
                player_id: i,
                country: Some("US".to_string())
            });

            initial_ratings.push(PlayerRating {
                player_id: i,
                mode: Mode::Osu,
                rating: Rating {
                    // We subtract 1.0 from player 1 here because
                    // global / country ranks etc. need to be known ahead of time.
                    // Player 0 has a higher starting rating than player 1,
                    // but player 1 wins. Thus, we simulate an upset and
                    // associated stat changes
                    mu: 1500.0 - i as f64,
                    sigma: 200.0
                },
                global_ranking: 0,
                country_ranking: 0,
                country: "US".to_string()
            })
        }

        // Create a match with the following structure:
        // - Match
        // - Game
        // - MatchScore (Player 0, Team 1, Score 525000)
        // - MatchScore (Player 1, Team 2, Score 525001)

        let start_time = chrono::offset::Utc::now().fixed_offset();
        let end_time = Some(start_time); // Assuming end_time is the same as start_time for demonstration

        let beatmap = test_beatmap();

        let match_scores = vec![
            MatchScore {
                player_id: 0,
                team: 1, // Blue
                score: 525000,
                enabled_mods: None,
                misses: 0,
                accuracy_standard: 100.0,
                accuracy_taiko: 0.0,
                accuracy_catch: 0.0,
                accuracy_mania: 0.0
            },
            MatchScore {
                player_id: 1,
                team: 2,       // Red
                score: 525001, // +1 score from blue. Should be the winner.
                enabled_mods: None,
                misses: 0,
                accuracy_standard: 100.0,
                accuracy_taiko: 0.0,
                accuracy_catch: 0.0,
                accuracy_mania: 0.0
            },
        ];

        let game = Game {
            id: 0,
            game_id: 0,
            ruleset: Mode::Osu,
            scoring_type: ScoringType::ScoreV2,
            team_type: TeamType::HeadToHead,
            start_time,
            end_time,
            beatmap: Some(beatmap),
            match_scores,
            mods: 0
        };

        let games = vec![game];

        let match_instance = Match {
            id: 0,
            match_id: 0,
            name: Some("TEST: (One) vs (One)".to_string()),
            mode: Mode::Osu,
            start_time: Some(start_time),
            end_time: None,
            games
        };

        let matches = vec![match_instance];

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

        let result = calculate_ratings(initial_ratings, &matches, &model);

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

        let constants = test_utils::TestConstants::new();

        assert!(
            (winner_base_stat.rating.mu - winner_expected_outcome.mu).abs() < constants.open_skill_leniency,
            "Winner's base stat mu is {}, should be {}",
            winner_base_stat.rating.mu,
            winner_expected_outcome.mu
        );

        assert!(
            (winner_base_stat.rating.sigma - winner_expected_outcome.sigma).abs() < constants.open_skill_leniency,
            "Winner's base stat sigma is {}, should be {}",
            winner_base_stat.rating.sigma,
            winner_expected_outcome.sigma
        );

        assert!(
            (loser_base_stat.rating.mu - loser_expected_outcome.mu).abs() < constants.open_skill_leniency,
            "Loser's base stat mu is {}, should be {}",
            loser_base_stat.rating.mu,
            loser_expected_outcome.mu
        );
        assert!(
            (loser_base_stat.rating.sigma - loser_expected_outcome.sigma).abs() < constants.open_skill_leniency,
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
            (loser_expected_outcome.mu - loser_stats.rating_after).abs() < constants.open_skill_leniency,
            "Loser's rating is {}, should be {}",
            loser_stats.rating_after,
            loser_expected_outcome.mu
        );
        assert!(
            (loser_expected_outcome.sigma - loser_stats.volatility_after).abs() < constants.open_skill_leniency,
            "Loser's volatility is {}, should be {}",
            loser_stats.volatility_after,
            loser_expected_outcome.sigma
        );

        // Expected sigma = actual sigma
        assert!(
            (winner_expected_outcome.mu - winner_stats.rating_after).abs() < constants.open_skill_leniency,
            "Winner's rating is {}, should be {}",
            winner_stats.rating_after,
            winner_expected_outcome.mu
        );
        assert!(
            (winner_expected_outcome.sigma - winner_stats.volatility_after).abs() < constants.open_skill_leniency,
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

        // Since it's 1v1 match we iterating between two possible values
        // 1.0 and 0.5
        //
        // PR = n/N
        // 0.5 = 1/2
        // 1.0 = 2/2

        assert_eq!(
            loser_stats.percentile_before, 0.5,
            "Loser's percentile before is {}, should be {}",
            loser_stats.percentile_before, 0.5
        );

        assert_eq!(
            winner_stats.percentile_after, 0.5,
            "Winner's percentile after is {:?}, should be {:?}",
            winner_stats.percentile_after, 0.5
        );

        assert_eq!(
            loser_stats.percentile_after, 1.0,
            "Loser's percentile after is {:?}, should be {:?}",
            loser_stats.percentile_after, 1.0
        );
    }

    #[test]
    fn test_owc_2023_data() {
        // Arrange
        let match_data = matches_from_json(include_str!("../../test_data/owc_2023.json"));
        let player_data = players_from_json(include_str!("../../test_data/owc_2023_players.json"));
        let country_mapping = country_mapping_from_json(include_str!("../../test_data/country_mapping.json"));

        let country_hash = hash_country_mappings(&country_mapping);

        // Organized by country, sorted by rating
        let country_ordering: HashMap<String, Vec<PlayerRating>> = HashMap::new();

        // Act
        let plackett_luce = create_model();

        // Process initial ratings, establish expected data
        let initial_ratings = create_initial_ratings(&match_data, &player_data);
        let mut processed_match_data = calculate_processed_match_data(&initial_ratings, &match_data, &plackett_luce);

        let mut copied_initial_ratings = initial_ratings.clone();

        let match_rating_stats = calculate_post_match_info(&mut copied_initial_ratings, &mut processed_match_data.0);
        let adjustments = calculate_player_adjustments(&initial_ratings, &copied_initial_ratings);

        // The amount of players that participated in the matches
        let mut players_count = 0;

        for m in &match_data {
            let mut player_map: HashMap<i32, i32> = HashMap::new();

            for g in &m.games {
                for s in &g.match_scores {
                    if player_map.contains_key(&s.player_id) {
                        continue;
                    }

                    player_map.insert(s.player_id, 0);
                }
            }

            players_count += player_map.len();
        }

        // Assert

        // Ensure the length of match rating stats matches the
        // total count of unique players in each match
        assert_eq!(match_rating_stats.len(), players_count);
        assert_eq!(processed_match_data.0.len(), match_data.len());
    }

    #[test]
    fn test_multiple_mode_tracking() {
        // Load in OWC 2023 data
        // Duplicate & change all of the ruleset values to taiko (1)
        // we label it as twc for convenience, but it's the same exact
        // data as OWC with a different name (and ruleset change)

        // Arrange
        let owc_data = matches_from_json(include_str!("../../test_data/owc_2023.json"));
        // This will be modified to have all modes set to 1 -
        // structure of the data is identical between modes
        let mut twc_data = matches_from_json(include_str!("../../test_data/owc_2023.json"));
        let mut player_data = players_from_json(include_str!("../../test_data/owc_2023_players.json"));
        let country_mapping = country_mapping_from_json(include_str!("../../test_data/country_mapping.json"));
        let country_hash = hash_country_mappings(&country_mapping);

        // Organized by country, sorted by rating
        let country_ordering: HashMap<String, Vec<PlayerRating>> = HashMap::new();
        let plackett_luce = create_model();

        // Act
        // Set all modes in twc_data to taiko
        for m in &mut twc_data {
            // Set the id to something different
            m.mode = Mode::Taiko;

            for g in &mut m.games {
                g.ruleset = Mode::Taiko;
            }
        }

        let standard_match_ids: Vec<_> = owc_data.iter().map(|x| x.id).collect();
        let taiko_match_ids: Vec<_> = twc_data.iter().map(|x| x.id).collect();

        // Set the osu ranks for taiko to be the exact same as standard
        for player in &mut player_data {
            player.rank_taiko = player.rank_standard;
            player.earliest_taiko_global_rank = player.earliest_osu_global_rank;
            player.earliest_taiko_global_rank_date = player.earliest_catch_global_rank_date;
        }

        // Scenario 1:
        // Calculating ratings separatly for owc and twc data
        // and making sure they are the same
        let initial_ratings = create_initial_ratings(&owc_data, &player_data);

        let copied_initial_ratings = initial_ratings.clone();
        let standard_res = calculate_ratings(copied_initial_ratings, &owc_data, &plackett_luce);

        let copied_initial_ratings = initial_ratings.clone();
        let taiko_res = calculate_ratings(copied_initial_ratings, &owc_data, &plackett_luce);

        assert_eq!(standard_res.rating_stats.len(), taiko_res.rating_stats.len());
        assert_eq!(standard_res.adjustments.len(), taiko_res.adjustments.len());

        for std in &standard_res.rating_stats {
            let taiko = taiko_res
                .rating_stats
                .iter()
                .find(|x| x.player_id == std.player_id && x.match_id == std.match_id)
                .unwrap();

            assert_eq!(std.player_id, taiko.player_id);
            assert_eq!(std.match_id, taiko.match_id);
            assert_eq!(std.match_cost, taiko.match_cost);
            assert_eq!(std.rating_before, taiko.rating_before);
            assert_eq!(std.rating_after, taiko.rating_after);
            assert_eq!(std.global_rank_before, taiko.global_rank_before);
            assert_eq!(std.country_rank_before, taiko.country_rank_before);
        }

        // Scenario 2:
        // Combining owc and twc data together and calculating results
        // comparing that results are the same but for two modes
        let mut combined_match_data = Vec::new();

        let expected_match_results_len = standard_res.rating_stats.len() + taiko_res.rating_stats.len();

        combined_match_data.extend(owc_data);
        combined_match_data.extend(twc_data);

        let mut combined_initial_ratings = Vec::new();
        combined_initial_ratings.extend(initial_ratings.clone());

        let mut cloned_ratings = initial_ratings.clone();
        cloned_ratings.iter_mut().for_each(|x| x.mode = Mode::Taiko);

        combined_initial_ratings.extend(cloned_ratings);

        // Calculating correct ranking placements after extending
        calculate_global_ranks(&mut combined_initial_ratings, Mode::Osu);
        calculate_global_ranks(&mut combined_initial_ratings, Mode::Taiko);

        calculate_country_ranks(&mut combined_initial_ratings, Mode::Osu);
        calculate_country_ranks(&mut combined_initial_ratings, Mode::Taiko);

        let combined_res = calculate_ratings(combined_initial_ratings, &combined_match_data, &plackett_luce);

        assert_eq!(combined_res.rating_stats.len(), expected_match_results_len);

        for rating in &combined_res.rating_stats {
            let occurences: Vec<usize> = combined_res
                .rating_stats
                .iter()
                .enumerate()
                .filter_map(|(i, x)| {
                    if x.player_id == rating.player_id && x.match_id == rating.match_id {
                        Some(i)
                    } else {
                        None
                    }
                })
                .collect();

            assert_eq!(occurences.len(), 2);

            let first_idx = occurences[0];
            let second_idx = occurences[1];

            let first = &combined_res.rating_stats[first_idx];
            let second = &combined_res.rating_stats[second_idx];

            assert_eq!(first.player_id, second.player_id);
            assert_eq!(first.match_id, second.match_id);
            assert_eq!(first.match_cost, second.match_cost);
            assert_eq!(first.rating_before, second.rating_before);
            assert_eq!(first.rating_after, second.rating_after);
            assert_eq!(first.global_rank_before, second.global_rank_before);
            assert_eq!(first.country_rank_before, second.country_rank_before);
        }
    }

    #[test]
    fn test_multiple_gamemodes_calculation_minimal() {
        // Two 1v1 matches of same players but in different gamemodes
        let initial_ratings = vec![
            PlayerRating {
                player_id: 1,
                mode: Mode::Osu,
                rating: Rating {
                    mu: 1500.0,
                    sigma: 200.0
                },
                global_ranking: 1,
                country_ranking: 1,
                country: "US".to_owned()
            },
            PlayerRating {
                player_id: 1,
                mode: Mode::Taiko,
                rating: Rating {
                    mu: 800.0,
                    sigma: 200.0
                },
                global_ranking: 2,
                country_ranking: 2,
                country: "US".to_owned()
            },
            PlayerRating {
                player_id: 2,
                mode: Mode::Taiko,
                rating: Rating {
                    mu: 1500.0,
                    sigma: 200.0
                },
                global_ranking: 1,
                country_ranking: 1,
                country: "US".to_owned()
            },
            PlayerRating {
                player_id: 2,
                mode: Mode::Osu,
                rating: Rating {
                    mu: 800.0,
                    sigma: 200.0
                },
                global_ranking: 2,
                country_ranking: 2,
                country: "US".to_owned()
            },
        ];

        // Osu match: player 2 is winner
        // Taiko match: player 1 is winner
        let matches = vec![
            // Osu match
            Match {
                id: 1,
                match_id: 123,
                name: Some("Osu game".to_owned()),
                mode: Mode::Osu,
                start_time: Some(Utc::now().with_timezone(&FixedOffset::east_opt(0).unwrap())),
                end_time: None,
                games: vec![Game {
                    id: 123,
                    ruleset: Mode::Osu,
                    scoring_type: ScoringType::Score,
                    team_type: TeamType::TeamVs,
                    mods: 0,
                    game_id: 456,
                    start_time: Utc::now().with_timezone(&FixedOffset::east_opt(0).unwrap()),
                    end_time: None,
                    beatmap: None,
                    match_scores: vec![
                        MatchScore {
                            player_id: 1,
                            team: 1,
                            score: 100000,
                            enabled_mods: Some(0),
                            misses: 1,
                            accuracy_standard: 100.0,
                            accuracy_taiko: 100.0,
                            accuracy_catch: 100.0,
                            accuracy_mania: 100.0
                        },
                        MatchScore {
                            player_id: 2,
                            team: 2,
                            score: 1000000,
                            enabled_mods: Some(0),
                            misses: 0,
                            accuracy_standard: 100.0,
                            accuracy_taiko: 100.0,
                            accuracy_catch: 100.0,
                            accuracy_mania: 100.0
                        },
                    ]
                }]
            },
            Match {
                id: 2,
                match_id: 124,
                name: Some("Taiko game".to_owned()),
                mode: Mode::Taiko,
                start_time: Some(Utc::now().with_timezone(&FixedOffset::east_opt(0).unwrap())),
                end_time: None,
                games: vec![Game {
                    id: 123,
                    ruleset: Mode::Taiko,
                    scoring_type: ScoringType::Score,
                    team_type: TeamType::TeamVs,
                    mods: 0,
                    game_id: 456,
                    start_time: Utc::now().with_timezone(&FixedOffset::east_opt(0).unwrap()),
                    end_time: None,
                    beatmap: None,
                    match_scores: vec![
                        MatchScore {
                            player_id: 1,
                            team: 1,
                            score: 1_000_000,
                            enabled_mods: Some(0),
                            misses: 1,
                            accuracy_standard: 100.0,
                            accuracy_taiko: 100.0,
                            accuracy_catch: 100.0,
                            accuracy_mania: 100.0
                        },
                        MatchScore {
                            player_id: 2,
                            team: 2,
                            score: 1000,
                            enabled_mods: Some(0),
                            misses: 0,
                            accuracy_standard: 100.0,
                            accuracy_taiko: 100.0,
                            accuracy_catch: 100.0,
                            accuracy_mania: 100.0
                        },
                    ]
                }]
            },
        ];

        let mut copied = initial_ratings.clone();

        let plackett_luce = create_model();
        let (mut processed_match_data, adjustments) = calculate_processed_match_data(&copied, &matches, &plackett_luce);

        let match_info = calculate_post_match_info(&mut copied, &mut processed_match_data);

        // Sanity checks to make sure there are no weird
        // global/country ranks assinged
        // In this particular tests there should be only 1's and 2's
        for m in &match_info {
            assert!((1..=2).contains(&m.global_rank_before));
            assert!((1..=2).contains(&m.global_rank_after));
            assert!((1..=2).contains(&m.country_rank_after));
            assert!((1..=2).contains(&m.country_rank_before));
        }

        let result = calculate_ratings(initial_ratings, &matches, &plackett_luce);

        // Same sanity check as above
        for m in &result.rating_stats {
            assert!((1..=2).contains(&m.global_rank_before));
            assert!((1..=2).contains(&m.global_rank_after));
            assert!((1..=2).contains(&m.country_rank_after));
            assert!((1..=2).contains(&m.country_rank_before));
        }

        assert_eq!(result.base_ratings.len(), 4);

        let p1_osu = result
            .base_ratings
            .iter()
            .find(|x| x.player_id == 1 && x.mode == Mode::Osu)
            .unwrap();
        let p2_osu = result
            .base_ratings
            .iter()
            .find(|x| x.player_id == 2 && x.mode == Mode::Osu)
            .unwrap();

        let p1_taiko = result
            .base_ratings
            .iter()
            .find(|x| x.player_id == 1 && x.mode == Mode::Taiko)
            .unwrap();
        let p2_taiko = result
            .base_ratings
            .iter()
            .find(|x| x.player_id == 2 && x.mode == Mode::Taiko)
            .unwrap();

        assert_eq!(p2_taiko.global_ranking, 1);
        assert_eq!(p1_taiko.global_ranking, 2);

        assert_eq!(p1_osu.global_ranking, 1);
        assert_eq!(p2_osu.global_ranking, 2);
    }

    #[test]
    fn test_global_ranking_calculation_different_gamemodes2() {
        let mut players = vec![
            PlayerRating {
                player_id: 1,
                mode: Mode::Osu,
                rating: Rating {
                    mu: 1500.0,
                    sigma: 200.0
                },
                global_ranking: 1,
                country_ranking: 1,
                country: "US".to_owned()
            },
            PlayerRating {
                player_id: 1,
                mode: Mode::Taiko,
                rating: Rating {
                    mu: 800.0,
                    sigma: 200.0
                },
                global_ranking: 2,
                country_ranking: 2,
                country: "US".to_owned()
            },
            PlayerRating {
                player_id: 2,
                mode: Mode::Taiko,
                rating: Rating {
                    mu: 1500.0,
                    sigma: 200.0
                },
                global_ranking: 1,
                country_ranking: 1,
                country: "US".to_owned()
            },
            PlayerRating {
                player_id: 2,
                mode: Mode::Osu,
                rating: Rating {
                    mu: 800.0,
                    sigma: 200.0
                },
                global_ranking: 2,
                country_ranking: 2,
                country: "US".to_owned()
            },
        ];

        calculate_global_ranks(&mut players, Mode::Osu);
        calculate_global_ranks(&mut players, Mode::Taiko);
        calculate_global_ranks(&mut players, Mode::Mania);
        calculate_global_ranks(&mut players, Mode::Catch);

        assert_eq!(
            players
                .iter()
                .find(|x| x.player_id == 1 && x.mode == Mode::Osu)
                .unwrap()
                .global_ranking,
            1
        );

        assert_eq!(
            players
                .iter()
                .find(|x| x.player_id == 1 && x.mode == Mode::Taiko)
                .unwrap()
                .global_ranking,
            2
        );

        assert_eq!(
            players
                .iter()
                .find(|x| x.player_id == 2 && x.mode == Mode::Osu)
                .unwrap()
                .global_ranking,
            2
        );

        assert_eq!(
            players
                .iter()
                .find(|x| x.player_id == 2 && x.mode == Mode::Taiko)
                .unwrap()
                .global_ranking,
            1
        );
    }

    #[test]
    fn test_global_ranking_calculation_different_gamemodes() {
        let mut players = vec![
            PlayerRating {
                player_id: 200,
                mode: Mode::Osu,
                rating: Rating {
                    mu: 1500.0,
                    sigma: 200.0
                },
                global_ranking: 0,
                country_ranking: 0,
                country: "RU".to_string()
            },
            PlayerRating {
                player_id: 100,
                mode: Mode::Taiko,
                rating: Rating {
                    mu: 1500.0,
                    sigma: 200.0
                },
                global_ranking: 0,
                country_ranking: 0,
                country: "RU".to_string()
            },
            PlayerRating {
                player_id: 102,
                mode: Mode::Taiko,
                rating: Rating {
                    mu: 1000.0,
                    sigma: 200.0
                },
                global_ranking: 0,
                country_ranking: 0,
                country: "RU".to_string()
            },
            PlayerRating {
                player_id: 101,
                mode: Mode::Taiko,
                rating: Rating {
                    mu: 1499.0,
                    sigma: 200.0
                },
                global_ranking: 0,
                country_ranking: 0,
                country: "RU".to_string()
            },
            PlayerRating {
                player_id: 202,
                mode: Mode::Osu,
                rating: Rating {
                    mu: 700.0,
                    sigma: 200.0
                },
                global_ranking: 0,
                country_ranking: 0,
                country: "RU".to_string()
            },
            PlayerRating {
                player_id: 201,
                mode: Mode::Osu,
                rating: Rating {
                    mu: 899.0,
                    sigma: 200.0
                },
                global_ranking: 0,
                country_ranking: 0,
                country: "RU".to_string()
            },
        ];

        calculate_global_ranks(&mut players, Mode::Osu);
        calculate_global_ranks(&mut players, Mode::Taiko);

        assert_eq!(players.iter().find(|x| x.player_id == 200).unwrap().global_ranking, 1);

        assert_eq!(players.iter().find(|x| x.player_id == 201).unwrap().global_ranking, 2);

        assert_eq!(players.iter().find(|x| x.player_id == 202).unwrap().global_ranking, 3);

        assert_eq!(players.iter().find(|x| x.player_id == 100).unwrap().global_ranking, 1);

        assert_eq!(players.iter().find(|x| x.player_id == 101).unwrap().global_ranking, 2);

        assert_eq!(players.iter().find(|x| x.player_id == 102).unwrap().global_ranking, 3);
    }

    #[test]
    fn test_country_ranking_calculation_different_gamemodes() {
        let mut players = vec![
            PlayerRating {
                player_id: 200,
                mode: Mode::Osu,
                rating: Rating {
                    mu: 1500.0,
                    sigma: 200.0
                },
                global_ranking: 0,
                country_ranking: 0,
                country: "RU".to_string()
            },
            PlayerRating {
                player_id: 101,
                mode: Mode::Taiko,
                rating: Rating {
                    mu: 1000.0,
                    sigma: 200.0
                },
                global_ranking: 0,
                country_ranking: 0,
                country: "RU".to_string()
            },
            PlayerRating {
                player_id: 201,
                mode: Mode::Osu,
                rating: Rating {
                    mu: 1449.0,
                    sigma: 200.0
                },
                global_ranking: 0,
                country_ranking: 0,
                country: "RU".to_string()
            },
            PlayerRating {
                player_id: 102,
                mode: Mode::Taiko,
                rating: Rating {
                    mu: 900.0,
                    sigma: 200.0
                },
                global_ranking: 0,
                country_ranking: 0,
                country: "RU".to_string()
            },
            PlayerRating {
                player_id: 100,
                mode: Mode::Taiko,
                rating: Rating {
                    mu: 1500.0,
                    sigma: 200.0
                },
                global_ranking: 0,
                country_ranking: 0,
                country: "RU".to_string()
            },
        ];

        calculate_country_ranks(&mut players, Mode::Osu);
        calculate_country_ranks(&mut players, Mode::Taiko);

        assert_eq!(players.iter().find(|x| x.player_id == 100).unwrap().country_ranking, 1);

        assert_eq!(players.iter().find(|x| x.player_id == 200).unwrap().country_ranking, 1);

        assert_eq!(players.iter().find(|x| x.player_id == 101).unwrap().country_ranking, 2);

        assert_eq!(players.iter().find(|x| x.player_id == 102).unwrap().country_ranking, 3);
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

    #[test]
    fn test_game_win_record_team_vs() {
        let game = Game {
            id: 14,
            game_id: 0,
            ruleset: Mode::Osu,
            scoring_type: ScoringType::ScoreV2,
            team_type: TeamType::TeamVs,
            start_time: Utc::now().fixed_offset(),
            end_time: Some(Utc::now().fixed_offset()),
            beatmap: Some(test_beatmap()),
            match_scores: vec![
                MatchScore {
                    player_id: 0,
                    team: 1,
                    score: 525000,
                    enabled_mods: None,
                    misses: 0,
                    accuracy_standard: 100.0,
                    accuracy_taiko: 0.0,
                    accuracy_catch: 0.0,
                    accuracy_mania: 0.0
                },
                MatchScore {
                    player_id: 1,
                    team: 1,
                    score: 525000,
                    enabled_mods: None,
                    misses: 0,
                    accuracy_standard: 100.0,
                    accuracy_taiko: 0.0,
                    accuracy_catch: 0.0,
                    accuracy_mania: 0.0
                },
                MatchScore {
                    player_id: 2,
                    team: 2,
                    score: 525000,
                    enabled_mods: None,
                    misses: 0,
                    accuracy_standard: 100.0,
                    accuracy_taiko: 0.0,
                    accuracy_catch: 0.0,
                    accuracy_mania: 0.0
                },
                MatchScore {
                    player_id: 3,
                    team: 2,
                    score: 625000,
                    enabled_mods: None,
                    misses: 0,
                    accuracy_standard: 100.0,
                    accuracy_taiko: 0.0,
                    accuracy_catch: 0.0,
                    accuracy_mania: 0.0
                },
            ],
            mods: 0
        };

        let expected = GameWinRecord {
            game_id: 14,
            winners: vec![2, 3],
            losers: vec![0, 1],
            winner_team: 2,
            loser_team: 1
        };

        let result = game_win_record(&game);

        assert_eq!(result, expected);
    }

    #[test]
    fn test_game_win_record_1v1() {
        let game = Game {
            id: 14,
            game_id: 0,
            ruleset: Mode::Osu,
            scoring_type: ScoringType::ScoreV2,
            team_type: TeamType::HeadToHead,
            start_time: Utc::now().fixed_offset(),
            end_time: Some(Utc::now().fixed_offset()),
            beatmap: Some(test_beatmap()),
            match_scores: vec![
                MatchScore {
                    player_id: 0,
                    team: 0,
                    score: 525000,
                    enabled_mods: None,
                    misses: 0,
                    accuracy_standard: 100.0,
                    accuracy_taiko: 0.0,
                    accuracy_catch: 0.0,
                    accuracy_mania: 0.0
                },
                MatchScore {
                    player_id: 1,
                    team: 0,
                    score: 625000,
                    enabled_mods: None,
                    misses: 0,
                    accuracy_standard: 100.0,
                    accuracy_taiko: 0.0,
                    accuracy_catch: 0.0,
                    accuracy_mania: 0.0
                },
            ],
            mods: 0
        };

        let expected = GameWinRecord {
            game_id: 14,
            winners: vec![1],
            losers: vec![0],
            winner_team: 0,
            loser_team: 0
        };

        let result = game_win_record(&game);

        assert_eq!(result, expected);
    }

    #[test]
    fn test_match_win_records_team_vs_simple() {
        // Winners: Team red, ids 2 and 3
        // Losers: Team blue, ids 0 and 1
        let match_data = Match {
            id: 12,
            match_id: 0,
            name: Some("Foo".to_string()),
            mode: Mode::Osu,
            start_time: None,
            end_time: None,
            games: vec![
                // Game 1: Red wins against blue
                Game {
                    id: 0,
                    ruleset: Mode::Osu,
                    scoring_type: ScoringType::ScoreV2,
                    team_type: TeamType::TeamVs,
                    mods: 0,
                    game_id: 0,
                    start_time: Default::default(),
                    end_time: None,
                    beatmap: None,
                    match_scores: vec![
                        MatchScore {
                            player_id: 0,
                            team: BLUE_TEAM_ID,
                            score: 525000,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 100.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0
                        },
                        MatchScore {
                            player_id: 1,
                            team: BLUE_TEAM_ID,
                            score: 525000,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 100.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0
                        },
                        MatchScore {
                            player_id: 2,
                            team: RED_TEAM_ID,
                            score: 525000,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 100.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0
                        },
                        MatchScore {
                            player_id: 3,
                            team: RED_TEAM_ID,
                            score: 625000,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 100.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0
                        },
                    ]
                },
                // Game 2: Red wins against blue
                Game {
                    id: 1,
                    ruleset: Mode::Osu,
                    scoring_type: ScoringType::ScoreV2,
                    team_type: TeamType::TeamVs,
                    mods: 0,
                    game_id: 1,
                    start_time: Default::default(),
                    end_time: None,
                    beatmap: None,
                    match_scores: vec![
                        MatchScore {
                            player_id: 0,
                            team: BLUE_TEAM_ID,
                            score: 525000,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 100.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0
                        },
                        MatchScore {
                            player_id: 1,
                            team: BLUE_TEAM_ID,
                            score: 525000,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 100.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0
                        },
                        MatchScore {
                            player_id: 2,
                            team: RED_TEAM_ID,
                            score: 525000,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 100.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0
                        },
                        MatchScore {
                            player_id: 3,
                            team: RED_TEAM_ID,
                            score: 625000,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 100.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0
                        },
                    ]
                },
                // Game 3: Blue wins against red
                Game {
                    id: 2,
                    ruleset: Mode::Osu,
                    scoring_type: ScoringType::ScoreV2,
                    team_type: TeamType::TeamVs,
                    mods: 0,
                    game_id: 2,
                    start_time: Default::default(),
                    end_time: None,
                    beatmap: None,
                    match_scores: vec![
                        MatchScore {
                            player_id: 0,
                            team: BLUE_TEAM_ID,
                            score: 625000,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 100.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0
                        },
                        MatchScore {
                            player_id: 1,
                            team: BLUE_TEAM_ID,
                            score: 625000,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 100.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0
                        },
                        MatchScore {
                            player_id: 2,
                            team: RED_TEAM_ID,
                            score: 525000,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 100.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0
                        },
                        MatchScore {
                            player_id: 3,
                            team: RED_TEAM_ID,
                            score: 525000,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 100.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0
                        },
                    ]
                },
            ]
        };

        let expected_mwr = MatchWinRecord {
            match_id: 12,
            loser_roster: vec![0, 1],
            winner_roster: vec![2, 3],
            loser_points: 1,
            winner_points: 2,
            winner_team: Some(RED_TEAM_ID),
            loser_team: Some(BLUE_TEAM_ID),
            match_type: Some(MatchType::Team)
        };

        let expected_gwrs = vec![
            GameWinRecord {
                game_id: 0,
                winners: vec![2, 3],
                losers: vec![0, 1],
                winner_team: RED_TEAM_ID,
                loser_team: BLUE_TEAM_ID
            },
            GameWinRecord {
                game_id: 1,
                winners: vec![2, 3],
                losers: vec![0, 1],
                winner_team: RED_TEAM_ID,
                loser_team: BLUE_TEAM_ID
            },
            GameWinRecord {
                game_id: 2,
                winners: vec![0, 1],
                losers: vec![2, 3],
                winner_team: BLUE_TEAM_ID,
                loser_team: RED_TEAM_ID
            },
        ];

        let (actual_mwr, actual_gwrs) = calculate_match_win_records(&vec![match_data]);

        assert_eq!(actual_mwr.len(), 1);
        assert_eq!(actual_gwrs.len(), 3);

        assert_eq!(actual_mwr[0], expected_mwr);

        for i in 0..3 {
            assert_eq!(actual_gwrs[i], expected_gwrs[i]);
        }
    }

    #[test]
    fn test_match_win_records_head_to_head_simple() {
        // Winners: 1
        // Losers: 0
        let match_data = Match {
            id: 12,
            match_id: 0,
            name: Some("Foo".to_string()),
            mode: Mode::Osu,
            start_time: None,
            end_time: None,
            games: vec![
                // Game 1: Player 1 wins against player 0
                Game {
                    id: 0,
                    ruleset: Mode::Osu,
                    scoring_type: ScoringType::ScoreV2,
                    team_type: TeamType::HeadToHead,
                    mods: 0,
                    game_id: 0,
                    start_time: Default::default(),
                    end_time: None,
                    beatmap: None,
                    match_scores: vec![
                        MatchScore {
                            player_id: 0,
                            team: 0,
                            score: 525000,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 100.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0
                        },
                        MatchScore {
                            player_id: 1,
                            team: 0,
                            score: 625000,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 100.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0
                        },
                    ]
                },
                // Game 2: Player 1 wins against player 0
                Game {
                    id: 1,
                    ruleset: Mode::Osu,
                    scoring_type: ScoringType::ScoreV2,
                    team_type: TeamType::HeadToHead,
                    mods: 0,
                    game_id: 1,
                    start_time: Default::default(),
                    end_time: None,
                    beatmap: None,
                    match_scores: vec![
                        MatchScore {
                            player_id: 0,
                            team: 0,
                            score: 525000,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 100.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0
                        },
                        MatchScore {
                            player_id: 1,
                            team: 0,
                            score: 625000,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 100.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0
                        },
                    ]
                },
                // Game 3: Player 0 wins against player 1
                Game {
                    id: 2,
                    ruleset: Mode::Osu,
                    scoring_type: ScoringType::ScoreV2,
                    team_type: TeamType::HeadToHead,
                    mods: 0,
                    game_id: 2,
                    start_time: Default::default(),
                    end_time: None,
                    beatmap: None,
                    match_scores: vec![
                        MatchScore {
                            player_id: 0,
                            team: 0,
                            score: 625000,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 100.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0
                        },
                        MatchScore {
                            player_id: 1,
                            team: 0,
                            score: 525000,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 100.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0
                        },
                    ]
                },
            ]
        };

        let expected_mwr = MatchWinRecord {
            match_id: 12,
            loser_roster: vec![0],
            winner_roster: vec![1],
            loser_points: 1,
            winner_points: 2,
            winner_team: Some(0),
            loser_team: Some(0),
            match_type: Some(MatchType::HeadToHead)
        };

        let (mwr, _) = calculate_match_win_records(&vec![match_data]);

        assert_eq!(mwr.len(), 1);
        assert_eq!(mwr[0], expected_mwr);
    }

    #[test]
    fn test_match_win_records_head_to_head_tie() {
        // Winners: 1
        // Losers: 0

        // Winner will earn 1 point and loser will earn 1 point across two games
        // This can happen if a showmatch isn't detected, or warmups get included
        // and the game results in a tie.

        let match_data = Match {
            id: 12,
            match_id: 0,
            name: Some("Foo".to_string()),
            mode: Mode::Osu,
            start_time: None,
            end_time: None,
            games: vec![
                // Game 1: Player 0 wins
                Game {
                    id: 0,
                    ruleset: Mode::Osu,
                    scoring_type: ScoringType::ScoreV2,
                    team_type: TeamType::HeadToHead,
                    mods: 0,
                    game_id: 0,
                    start_time: Default::default(),
                    end_time: None,
                    beatmap: None,
                    match_scores: vec![
                        MatchScore {
                            player_id: 0,
                            team: 0,
                            score: 525001,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 100.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0
                        },
                        MatchScore {
                            player_id: 1,
                            team: 0,
                            score: 525000,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 100.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0
                        },
                    ]
                },
                // Game 2: Player 1 wins
                Game {
                    id: 1,
                    ruleset: Mode::Osu,
                    scoring_type: ScoringType::ScoreV2,
                    team_type: TeamType::HeadToHead,
                    mods: 0,
                    game_id: 1,
                    start_time: Default::default(),
                    end_time: None,
                    beatmap: None,
                    match_scores: vec![
                        MatchScore {
                            player_id: 0,
                            team: 0,
                            score: 525000,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 100.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0
                        },
                        MatchScore {
                            player_id: 1,
                            team: 0,
                            score: 525001,
                            enabled_mods: None,
                            misses: 0,
                            accuracy_standard: 100.0,
                            accuracy_taiko: 0.0,
                            accuracy_catch: 0.0,
                            accuracy_mania: 0.0
                        },
                    ]
                },
            ]
        };

        let expected_mwr = MatchWinRecord {
            match_id: 12,
            loser_roster: vec![0],
            winner_roster: vec![1],
            loser_points: 1,
            winner_points: 1,
            winner_team: None,
            loser_team: None,
            match_type: Some(MatchType::HeadToHead)
        };

        let (actual_mwr, actual_gwrs) = calculate_match_win_records(&vec![match_data]);

        assert_eq!(actual_mwr.len(), 1);
        assert_eq!(actual_gwrs.len(), 2);

        // Assert all is equal besides the roster (order does not matter in a tie)
        assert_eq!(actual_mwr[0].match_id, expected_mwr.match_id);
        assert_eq!(actual_mwr[0].loser_points, expected_mwr.loser_points);
        assert_eq!(actual_mwr[0].winner_points, expected_mwr.winner_points);
        assert_eq!(actual_mwr[0].winner_team, expected_mwr.winner_team);
        assert_eq!(actual_mwr[0].loser_team, expected_mwr.loser_team);
        assert_eq!(actual_mwr[0].match_type, expected_mwr.match_type);
    }

    #[test]
    fn test_player_match_stats() {
        let matches = vec![Match {
            id: 1,
            match_id: 123,
            name: Some("Osu game".to_owned()),
            mode: Mode::Osu,
            start_time: Some(Utc::now().with_timezone(&FixedOffset::east_opt(0).unwrap())),
            end_time: None,
            games: vec![
                // Game 1:
                // - Player 1: 100000
                // - Player 2: 1000000
                //
                // - P1 acc: 100.0
                // - P2 acc: 100.0
                //
                // - P1 misses: 1
                // - P2 misses: 0
                //
                // - P1 placing: 2
                // - P2 placing: 1
                Game {
                    id: 123,
                    ruleset: Mode::Osu,
                    scoring_type: ScoringType::Score,
                    team_type: TeamType::TeamVs,
                    mods: 0,
                    game_id: 456,
                    start_time: Utc::now().with_timezone(&FixedOffset::east_opt(0).unwrap()),
                    end_time: None,
                    beatmap: None,
                    match_scores: vec![
                        MatchScore {
                            player_id: 1,
                            team: 1,
                            score: 100000,
                            enabled_mods: Some(0),
                            misses: 1,
                            accuracy_standard: 100.0,
                            accuracy_taiko: 100.0,
                            accuracy_catch: 100.0,
                            accuracy_mania: 100.0
                        },
                        MatchScore {
                            player_id: 2,
                            team: 2,
                            score: 1000000,
                            enabled_mods: Some(0),
                            misses: 0,
                            accuracy_standard: 100.0,
                            accuracy_taiko: 100.0,
                            accuracy_catch: 100.0,
                            accuracy_mania: 100.0
                        },
                    ]
                },
                // Game 2:
                // - Player 1: 200000
                // - Player 2: 500000
                //
                // - P1 acc: 90.0
                // - P2 acc: 95.0
                //
                // - P1 misses: 1
                // - P2 misses: 3
                //
                // - P1 placing: 2
                // - P2 placing: 1
                Game {
                    id: 2,
                    ruleset: Mode::Osu,
                    scoring_type: ScoringType::Score,
                    team_type: TeamType::TeamVs,
                    mods: 0,
                    game_id: 457,
                    start_time: Utc::now().with_timezone(&FixedOffset::east_opt(0).unwrap()),
                    end_time: None,
                    beatmap: None,
                    match_scores: vec![
                        MatchScore {
                            player_id: 1,
                            team: 1,
                            score: 200000,
                            enabled_mods: Some(0),
                            misses: 1,
                            accuracy_standard: 90.0,
                            accuracy_taiko: 100.0,
                            accuracy_catch: 100.0,
                            accuracy_mania: 100.0
                        },
                        MatchScore {
                            player_id: 2,
                            team: 2,
                            score: 500000,
                            enabled_mods: Some(0),
                            misses: 3,
                            accuracy_standard: 95.0,
                            accuracy_taiko: 100.0,
                            accuracy_catch: 100.0,
                            accuracy_mania: 100.0
                        },
                    ]
                },
                // Game 3:
                //
                // - Player 1: 230210
                // - Player 2: 300000
                //
                // - P1 acc: 80.0
                // - P2 acc: 85.0
                //
                // - P1 misses: 12
                // - P2 misses: 7
                //
                // - P1 placing: 2
                // - P2 placing: 1
                Game {
                    id: 3,
                    ruleset: Mode::Osu,
                    scoring_type: ScoringType::Score,
                    team_type: TeamType::TeamVs,
                    mods: 0,
                    game_id: 458,
                    start_time: Utc::now().with_timezone(&FixedOffset::east_opt(0).unwrap()),
                    end_time: None,
                    beatmap: None,
                    match_scores: vec![
                        MatchScore {
                            player_id: 1,
                            team: 1,
                            score: 230210,
                            enabled_mods: Some(0),
                            misses: 12,
                            accuracy_standard: 80.0,
                            accuracy_taiko: 100.0,
                            accuracy_catch: 100.0,
                            accuracy_mania: 100.0
                        },
                        MatchScore {
                            player_id: 2,
                            team: 2,
                            score: 300000,
                            enabled_mods: Some(0),
                            misses: 7,
                            accuracy_standard: 85.0,
                            accuracy_taiko: 100.0,
                            accuracy_catch: 100.0,
                            accuracy_mania: 100.0
                        },
                    ]
                },
                // Game 4:
                //
                // - Player 1: 1000000
                // - Player 2: 100000
                //
                // - P1 acc: 100.0
                // - P2 acc: 86.0
                //
                // - P1 misses: 0
                // - P2 misses: 5
                //
                // - P1 placing: 1
                // - P2 placing: 2
                Game {
                    id: 4,
                    ruleset: Mode::Osu,
                    scoring_type: ScoringType::Score,
                    team_type: TeamType::TeamVs,
                    mods: 0,
                    game_id: 459,
                    start_time: Utc::now().with_timezone(&FixedOffset::east_opt(0).unwrap()),
                    end_time: None,
                    beatmap: None,
                    match_scores: vec![
                        MatchScore {
                            player_id: 1,
                            team: 1,
                            score: 1000000,
                            enabled_mods: Some(0),
                            misses: 0,
                            accuracy_standard: 100.0,
                            accuracy_taiko: 100.0,
                            accuracy_catch: 100.0,
                            accuracy_mania: 100.0
                        },
                        MatchScore {
                            player_id: 2,
                            team: 2,
                            score: 100000,
                            enabled_mods: Some(0),
                            misses: 5,
                            accuracy_standard: 86.0,
                            accuracy_taiko: 100.0,
                            accuracy_catch: 100.0,
                            accuracy_mania: 100.0
                        },
                    ]
                },
            ]
        }];

        let expected_p1 = PlayerMatchStats {
            player_id: 1,
            match_id: 1,
            won: false,
            average_score: 382552.5,
            average_misses: 3.5,
            average_accuracy: 92.5,
            average_placement: 1.75,
            games_won: 1,
            games_lost: 3,
            games_played: 4,
            teammate_ids: vec![],
            opponent_ids: vec![2]
        };

        let expected_p2 = PlayerMatchStats {
            player_id: 2,
            match_id: 1,
            won: true,
            average_score: 475000.0,
            average_misses: 3.75,
            average_accuracy: 91.5,
            average_placement: 1.25,
            games_won: 3,
            games_lost: 1,
            games_played: 4,
            teammate_ids: vec![],
            opponent_ids: vec![1]
        };

        let results = player_match_stats(&matches);

        let r_1 = results.iter().find(|x| x.player_id == 1).unwrap();
        let r_2 = results.iter().find(|x| x.player_id == 2).unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(*r_1, expected_p1);
        assert_eq!(*r_2, expected_p2);
    }
}
