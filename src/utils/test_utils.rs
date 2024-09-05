use chrono::{DateTime, FixedOffset, Utc};
use std::collections::HashMap;
use std::ops::Add;

use crate::model::db_structs::{NewPlayerRating, NewRatingAdjustment};
use crate::model::{
    constants::{DEFAULT_RATING, DEFAULT_VOLATILITY},
    db_structs::{Game, Match, PlayerPlacement, PlayerRating, RulesetData},
    structures::{rating_adjustment_type::RatingAdjustmentType, ruleset::Ruleset}
};

pub fn generate_player_rating(
    player_id: i32,
    ruleset: Ruleset,
    rating: f64,
    volatility: f64,
    n_adjustments: i32
) -> NewPlayerRating {
    let default_time = "2007-09-16T00:00:00-00:00".parse::<DateTime<FixedOffset>>().unwrap();
    
    let change_per_adjustment = rating / n_adjustments as f64;
    let mut adjustments = Vec::new();
    
    for i in 1..=n_adjustments {
        let adjustment_type = match i {
            1 => RatingAdjustmentType::Initial,
            _ => RatingAdjustmentType::Match
        };
        
        let rating_before = rating - change_per_adjustment * (i - 1) as f64;
        let rating_after = rating - change_per_adjustment * i as f64;
        let volatility_before = volatility;
        let volatility_after = volatility;
        let timestamp = default_time.add(chrono::Duration::days(i as i64));

        adjustments.push(NewRatingAdjustment {
            player_id,
            adjustment_type,
            match_id: None,
            rating_before,
            rating_after,
            volatility_before,
            volatility_after,
            timestamp,
            player_rating_id: 0,
        });
    }
    
    NewPlayerRating {
        id: player_id,
        player_id,
        ruleset,
        rating,
        volatility,
        percentile: 0.0,
        global_rank: 0,
        country_rank: 0,
        adjustments,
    }
}

pub fn generate_ruleset_data(
    ruleset: Ruleset,
    global_rank: Option<i32>,
    earliest_global_rank: Option<i32>
) -> RulesetData {
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
    Game {
        id,
        game_id: 0,
        start_time: Default::default(),
        end_time: None,
        placements: placements.to_vec()
    }
}

pub fn generate_country_mapping(player_ratings: &[NewPlayerRating], country: &str) -> HashMap<i32, String> {
    let mut mapping = HashMap::new();
    for p in player_ratings {
        mapping.insert(p.player_id, country.to_string());
    }

    mapping
}

pub fn generate_match(id: i32, ruleset: Ruleset, games: &[Game], start_time: Option<DateTime<FixedOffset>>) -> Match {
    Match {
        id,
        ruleset,
        start_time,
        end_time: None,
        games: games.to_vec()
    }
}

pub fn generate_matches(n: i32, player_ratings: &[PlayerRating]) -> Vec<Match> {
    let mut matches = Vec::new();
    for i in 1..=n {
        let game_count = 9;
        matches.push(generate_match(
            i,
            Ruleset::Osu,
            &generate_games(game_count, random_placements(player_ratings).as_slice()),
            Some(Utc::now().fixed_offset())
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

fn random_placements(player_ratings: &[PlayerRating]) -> Vec<PlayerPlacement> {
    let mut placements = Vec::new();

    // Select random placements for each player (1 to size)
    for (i, rating) in player_ratings.iter().enumerate() {
        placements.push(generate_placement(rating.player_id, i as i32));
    }

    placements
}

/// Generates `n` player ratings with default values
pub fn generate_default_initial_ratings(n: i32) -> Vec<NewPlayerRating> {
    let mut players = Vec::new();
    for i in 1..=n {
        players.push(generate_player_rating(i, Ruleset::Osu, DEFAULT_RATING, DEFAULT_VOLATILITY, 1));
    }

    players
}
