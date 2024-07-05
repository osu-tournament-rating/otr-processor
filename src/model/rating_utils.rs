use crate::{
    api::api_structs::{Player, PlayerRating},
    model::{
        constants,
        constants::{DEFAULT_RATING, DEFAULT_VOLATILITY, MULTIPLIER, OSU_RATING_CEILING},
        structures::{rating_adjustment_type::RatingAdjustmentType, ruleset::Ruleset}
    }
};
use chrono::{DateTime, FixedOffset};
use constants::OSU_RATING_FLOOR;
use std::collections::HashMap;
use strum::IntoEnumIterator;

pub fn initial_ratings(players: &[Player]) -> HashMap<(i32, Ruleset), PlayerRating> {
    let mut map = HashMap::new();

    for player in players {
        for ruleset in Ruleset::iter() {
            map.insert((player.id, ruleset), create_initial_rating(player, &ruleset));
        }
    }

    map
}

fn create_initial_rating(player: &Player, ruleset: &Ruleset) -> PlayerRating {
    let timestamp: DateTime<FixedOffset> = "2007-09-16T00:00:00".parse().unwrap();

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
        Some(r) => mu_from_rank(r, *ruleset),
        None => DEFAULT_RATING
    }
}

fn mu_from_rank(rank: i32, ruleset: Ruleset) -> f64 {
    let left_slope = 4.0;
    let right_slope = 3.0;

    let mean = mean_from_ruleset(ruleset);
    let std_dev = std_dev_from_ruleset(ruleset);

    let z = (rank as f64 / mean.exp()).ln() / std_dev;
    let val = MULTIPLIER * (18.0 - (if z > 0.0 { left_slope } else { right_slope }) * z);

    if val < MULTIPLIER * OSU_RATING_FLOOR {
        return MULTIPLIER * OSU_RATING_FLOOR;
    }

    if val > MULTIPLIER * OSU_RATING_CEILING {
        return MULTIPLIER * OSU_RATING_CEILING;
    }

    val
}

fn mean_from_ruleset(ruleset: Ruleset) -> f64 {
    match ruleset {
        Ruleset::Osu => 9.91,
        Ruleset::Taiko => 7.59,
        Ruleset::Catch => 6.75,
        Ruleset::Mania4k | Ruleset::Mania7k => 8.18
    }
}

fn std_dev_from_ruleset(ruleset: Ruleset) -> f64 {
    match ruleset {
        Ruleset::Osu => 1.59,
        Ruleset::Taiko => 1.56,
        Ruleset::Catch => 1.54,
        Ruleset::Mania4k | Ruleset::Mania7k => 1.55
    }
}
