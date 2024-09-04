use crate::{
    model::db_structs::{Player, PlayerRating},
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
    let timestamp: DateTime<FixedOffset> = "2007-09-16T00:00:00-00:00".parse().unwrap();

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

    if val < OSU_RATING_FLOOR {
        return OSU_RATING_FLOOR;
    }

    if val > OSU_RATING_CEILING {
        return OSU_RATING_CEILING;
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

#[cfg(test)]
mod tests {
    use crate::{
        model::db_structs::Player,
        model::{
            constants::{DEFAULT_VOLATILITY, OSU_RATING_CEILING, OSU_RATING_FLOOR},
            rating_utils::{mu_from_rank, std_dev_from_ruleset},
            structures::{
                rating_adjustment_type::RatingAdjustmentType,
                ruleset::Ruleset::{Catch, Mania4k, Mania7k, Osu, Taiko}
            }
        },
        utils::test_utils::generate_player_rating
    };

    #[test]
    fn test_ruleset_stddev_osu() {
        let expected = 1.59;
        let actual = std_dev_from_ruleset(Osu);

        assert_eq!(expected, actual)
    }

    #[test]
    fn test_ruleset_stddev_taiko() {
        let expected = 1.56;
        let actual = std_dev_from_ruleset(Taiko);

        assert_eq!(expected, actual)
    }

    #[test]
    fn test_ruleset_stddev_catch() {
        let expected = 1.54;
        let actual = std_dev_from_ruleset(Catch);

        assert_eq!(expected, actual)
    }

    #[test]
    fn test_ruleset_stddev_mania_4k_7k() {
        let expected = 1.55;
        let actual_4k = std_dev_from_ruleset(Mania4k);
        let actual_7k = std_dev_from_ruleset(Mania7k);

        assert_eq!(expected, actual_4k);
        assert_eq!(expected, actual_7k);
    }

    #[test]
    fn test_mu_from_rank_maximum() {
        let rank = 1;
        let expected_mu = OSU_RATING_CEILING;

        let actual_mu_osu = mu_from_rank(rank, Osu);
        let actual_mu_taiko = mu_from_rank(rank, Taiko);
        let actual_mu_catch = mu_from_rank(rank, Catch);
        let actual_mu_mania_4k = mu_from_rank(rank, Mania4k);
        let actual_mu_mania_7k = mu_from_rank(rank, Mania7k);

        assert_eq!(expected_mu, actual_mu_osu);
        assert_eq!(expected_mu, actual_mu_taiko);
        assert_eq!(expected_mu, actual_mu_catch);
        assert_eq!(expected_mu, actual_mu_mania_4k);
        assert_eq!(expected_mu, actual_mu_mania_7k);
    }

    #[test]
    fn test_mu_from_rank_minimum() {
        let rank = 10_000_000;
        let expected_mu = OSU_RATING_FLOOR;

        let actual_mu_osu = mu_from_rank(rank, Osu);
        let actual_mu_taiko = mu_from_rank(rank, Taiko);
        let actual_mu_catch = mu_from_rank(rank, Catch);
        let actual_mu_mania_4k = mu_from_rank(rank, Mania4k);
        let actual_mu_mania_7k = mu_from_rank(rank, Mania7k);

        assert_eq!(expected_mu, actual_mu_osu);
        assert_eq!(expected_mu, actual_mu_taiko);
        assert_eq!(expected_mu, actual_mu_catch);
        assert_eq!(expected_mu, actual_mu_mania_4k);
        assert_eq!(expected_mu, actual_mu_mania_7k);
    }

    #[test]
    fn test_create_initial_ratings() {
        let player = Player {
            id: 1,
            username: Some("Test".to_string()),
            country: None,
            rank_standard: Some(1),
            rank_taiko: Some(1),
            rank_catch: Some(1),
            rank_mania: Some(1),
            earliest_osu_global_rank: Some(1),
            earliest_osu_global_rank_date: None,
            earliest_taiko_global_rank: Some(1),
            earliest_taiko_global_rank_date: None,
            earliest_catch_global_rank: Some(1),
            earliest_catch_global_rank_date: None,
            earliest_mania_global_rank: Some(1),
            earliest_mania_global_rank_date: None
        };

        let expected_osu = mu_from_rank(1, Osu);
        let expected_taiko = mu_from_rank(1, Taiko);
        let expected_catch = mu_from_rank(1, Catch);
        let expected_mania4k = mu_from_rank(1, Mania4k);
        let expected_mania7k = mu_from_rank(1, Mania7k);

        let actual_osu = super::initial_mu(&player, &Osu);
        let actual_taiko = super::initial_mu(&player, &Taiko);
        let actual_catch = super::initial_mu(&player, &Catch);
        let actual_mania_4k = super::initial_mu(&player, &Mania4k);
        let actual_mania_7k = super::initial_mu(&player, &Mania7k);

        assert_eq!(expected_osu, actual_osu);
        assert_eq!(expected_taiko, actual_taiko);
        assert_eq!(expected_catch, actual_catch);
        assert_eq!(expected_mania4k, actual_mania_4k);
        assert_eq!(expected_mania7k, actual_mania_7k);
    }

    #[test]
    fn test_create_initial_rating() {
        let player = Player {
            id: 0,
            username: None,
            country: None,
            rank_standard: Some(1),
            rank_taiko: Some(1),
            rank_catch: Some(1),
            rank_mania: Some(1),
            earliest_osu_global_rank: None,
            earliest_osu_global_rank_date: None,
            earliest_taiko_global_rank: None,
            earliest_taiko_global_rank_date: None,
            earliest_catch_global_rank: None,
            earliest_catch_global_rank_date: None,
            earliest_mania_global_rank: None,
            earliest_mania_global_rank_date: None
        };

        let rating_osu = mu_from_rank(1, Osu);
        let rating_taiko = mu_from_rank(1, Taiko);
        let rating_catch = mu_from_rank(1, Catch);
        let rating_mania4k = mu_from_rank(1, Mania4k);
        let rating_mania7k = mu_from_rank(1, Mania7k);

        let expected_osu =
            generate_player_rating(0, rating_osu, DEFAULT_VOLATILITY, RatingAdjustmentType::Initial, None);
        let expected_taiko =
            generate_player_rating(0, rating_taiko, DEFAULT_VOLATILITY, RatingAdjustmentType::Initial, None);
        let expected_catch =
            generate_player_rating(0, rating_catch, DEFAULT_VOLATILITY, RatingAdjustmentType::Initial, None);
        let expected_mania4k = generate_player_rating(
            0,
            rating_mania4k,
            DEFAULT_VOLATILITY,
            RatingAdjustmentType::Initial,
            None
        );
        let expected_mania7k = generate_player_rating(
            0,
            rating_mania7k,
            DEFAULT_VOLATILITY,
            RatingAdjustmentType::Initial,
            None
        );

        let actual_osu = super::create_initial_rating(&player, &Osu);
        let actual_taiko = super::create_initial_rating(&player, &Taiko);
        let actual_catch = super::create_initial_rating(&player, &Catch);
        let actual_mania4k = super::create_initial_rating(&player, &Mania4k);
        let actual_mania7k = super::create_initial_rating(&player, &Mania7k);

        assert_eq!(expected_osu.rating, actual_osu.rating);
        assert_eq!(expected_osu.volatility, actual_osu.volatility);
        assert_eq!(expected_osu.adjustment_type, actual_osu.adjustment_type);

        assert_eq!(expected_taiko.rating, actual_taiko.rating);
        assert_eq!(expected_taiko.volatility, actual_taiko.volatility);
        assert_eq!(expected_taiko.adjustment_type, actual_taiko.adjustment_type);

        assert_eq!(expected_catch.rating, actual_catch.rating);
        assert_eq!(expected_catch.volatility, actual_catch.volatility);
        assert_eq!(expected_catch.adjustment_type, actual_catch.adjustment_type);

        assert_eq!(expected_mania4k.rating, actual_mania4k.rating);
        assert_eq!(expected_mania4k.volatility, actual_mania4k.volatility);
        assert_eq!(expected_mania4k.adjustment_type, actual_mania4k.adjustment_type);

        assert_eq!(expected_mania7k.rating, actual_mania7k.rating);
        assert_eq!(expected_mania7k.volatility, actual_mania7k.volatility);
        assert_eq!(expected_mania7k.adjustment_type, actual_mania7k.adjustment_type);
    }
}
