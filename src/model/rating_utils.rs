use std::collections::HashMap;

use chrono::{DateTime, FixedOffset};

use crate::{
    api::api_structs::{Player, PlayerRating},
    model::{
        constants,
        constants::{DEFAULT_RATING, DEFAULT_VOLATILITY},
        structures::{rating_adjustment_type::RatingAdjustmentType, ruleset::Ruleset}
    }
};

// pub fn initial_ratings(players: &[Player]) -> HashMap<(i32, Ruleset), PlayerRating> {
//     let mut map = HashMap::new();
//
//     map
// }

fn create_rating(player: &Player, ruleset: &Ruleset) -> PlayerRating {
    let timestamp: DateTime<FixedOffset> = "2007-09-17T00:00:00".parse().unwrap();

    PlayerRating {
        player_id: player.id,
        ruleset: *ruleset,
        rating: initial_mu(player, ruleset),
        volatility: DEFAULT_VOLATILITY,
        percentile: 0.0,
        global_rank: 0,
        country_rank: 0,
        timestamp,
        adjustment_type: RatingAdjustmentType::Initial
    }
}

fn initial_mu(player: &Player, ruleset: &Ruleset) -> f64 {
    let rank = match ruleset {
        Ruleset::Osu => player.earliest_osu_global_rank.or(player.rank_standard),
        Ruleset::Taiko => player.earliest_taiko_global_rank.or(player.rank_taiko),
        Ruleset::Catch => player.earliest_catch_global_rank.or(player.rank_catch),
        Ruleset::Mania4k => player.earliest_mania_global_rank.or(player.rank_mania),
        Ruleset::Mania7k => player.earliest_mania_global_rank.or(player.rank_mania)
    };

    match rank {
        Some(r) => mu_from_rank(r),
        None => DEFAULT_RATING
    }
}

fn mu_from_rank(rank: i32) -> f64 {
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
