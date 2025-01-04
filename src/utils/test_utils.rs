use crate::{
    database::db_structs::{
        Game, GameScore, Match, Player, PlayerPlacement, PlayerRating, RatingAdjustment, RulesetData
    },
    model::{
        constants::{DEFAULT_RATING, DEFAULT_VOLATILITY},
        structures::{rating_adjustment_type::RatingAdjustmentType, ruleset::Ruleset}
    }
};
use chrono::{DateTime, FixedOffset, Utc};
use std::{collections::HashMap, ops::Add};

pub fn generate_player_rating(
    player_id: i32,
    ruleset: Ruleset,
    rating: f64,
    volatility: f64,
    n_adjustments: i32
) -> PlayerRating {
    if n_adjustments < 1 {
        panic!("Number of adjustments must be at least 1");
    }

    let default_time = "2007-09-16T00:00:00-00:00".parse::<DateTime<FixedOffset>>().unwrap();

    let change_per_adjustment = rating / n_adjustments as f64;
    let mut adjustments = Vec::new();

    for i in (1..=n_adjustments).rev() {
        let adjustment_type = match i {
            // If `i` is equal to `n_adjustments`, set to Initial
            val if val == n_adjustments => RatingAdjustmentType::Initial,
            _ => RatingAdjustmentType::Match
        };

        let rating_before = rating - change_per_adjustment * ((n_adjustments - i) - 1) as f64;
        let rating_after = rating - change_per_adjustment * (n_adjustments - i) as f64;
        let volatility_before = volatility;
        let volatility_after = volatility;
        let timestamp = default_time.add(chrono::Duration::days(i as i64));

        adjustments.push(RatingAdjustment {
            player_id,
            ruleset,
            adjustment_type,
            match_id: None,
            rating_before,
            rating_after,
            volatility_before,
            volatility_after,
            timestamp
        });
    }

    PlayerRating {
        id: player_id,
        player_id,
        ruleset,
        rating,
        volatility,
        percentile: 0.0,
        global_rank: 0,
        country_rank: 0,
        adjustments
    }
}

pub fn generate_ruleset_data(ruleset: Ruleset, global_rank: i32, earliest_global_rank: Option<i32>) -> RulesetData {
    RulesetData {
        ruleset,
        global_rank,
        earliest_global_rank
    }
}

pub fn generate_placement(player_id: i32, placement: i32) -> PlayerPlacement {
    PlayerPlacement { player_id, placement }
}

pub fn generate_game(id: i32, placements: &[PlayerPlacement]) -> Game {
    let scores = placements
        .iter()
        .map(|p| GameScore {
            id: 0,
            player_id: p.player_id,
            game_id: id,
            score: 0,
            placement: p.placement
        })
        .collect();

    Game {
        id,
        ruleset: Ruleset::Osu,
        start_time: Default::default(),
        end_time: Default::default(),
        scores
    }
}

pub fn generate_country_mapping_player_ratings(player_ratings: &[PlayerRating], country: &str) -> HashMap<i32, String> {
    let mut mapping = HashMap::new();
    for p in player_ratings {
        mapping.insert(p.player_id, country.to_string());
    }

    mapping
}

pub fn generate_country_mapping_players(players: &[Player]) -> HashMap<i32, String> {
    let mut mapping: HashMap<i32, String> = HashMap::new();
    for p in players {
        mapping.insert(p.id, p.country.clone().unwrap_or_default());
    }

    mapping
}

pub fn generate_match(id: i32, ruleset: Ruleset, games: &[Game], start_time: DateTime<FixedOffset>) -> Match {
    Match {
        id,
        name: "Test Match".to_string(),
        ruleset,
        start_time,
        end_time: start_time.add(chrono::Duration::hours(1)),
        games: games.to_vec()
    }
}

pub fn generate_matches(n: i32, player_ids: &[i32]) -> Vec<Match> {
    let mut matches = Vec::new();
    for (i, _) in player_ids.iter().enumerate() {
        let game_count = 9;
        matches.push(generate_match(
            i as i32,
            Ruleset::Osu,
            &generate_games(game_count, random_placements(player_ids).as_slice()),
            Utc::now().fixed_offset()
        ));
    }

    matches
}

fn generate_games(n: i32, placements: &[PlayerPlacement]) -> Vec<Game> {
    let mut games = Vec::new();
    for i in 1..=n {
        games.push(generate_game(i, placements));
    }

    games
}

fn random_placements(player_ids: &[i32]) -> Vec<PlayerPlacement> {
    let mut placements = Vec::new();

    // Select random placements for each player (1 to size)
    for (i, id) in player_ids.iter().enumerate() {
        placements.push(generate_placement(*id, i as i32));
    }

    placements
}

/// Generates `n` player ratings with default values
pub fn generate_default_initial_ratings(n: i32) -> Vec<PlayerRating> {
    let mut players = Vec::new();
    for i in 1..=n {
        players.push(generate_player_rating(
            i,
            Ruleset::Osu,
            DEFAULT_RATING,
            DEFAULT_VOLATILITY,
            1
        ));
    }

    players
}
