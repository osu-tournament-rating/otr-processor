use crate::{
    api::api_structs::{Game, PlayerPlacement, PlayerRating},
    model::structures::{rating_adjustment_type::RatingSource, ruleset::Ruleset}
};
use std::collections::HashMap;

pub fn generate_player_rating(id: i32, rating: f64, volatility: f64) -> PlayerRating {
    PlayerRating {
        player_id: id,
        ruleset: Ruleset::Osu,
        rating,
        volatility,
        percentile: 0.0,
        global_rank: 0,
        country_rank: 0,
        timestamp: Default::default(),
        source: RatingSource::Match
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

pub fn generate_country_mapping(player_ratings: &[PlayerRating], country: &str) -> HashMap<i32, String> {
    let mut mapping = HashMap::new();
    for p in player_ratings {
        mapping.insert(p.player_id, country.to_string());
    }

    mapping
}
