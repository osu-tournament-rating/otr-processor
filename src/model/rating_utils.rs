use crate::model::{
    constants,
    constants::{DEFAULT_RATING, DEFAULT_VOLATILITY, MULTIPLIER, OSU_RATING_CEILING},
    db_structs::{NewPlayer, NewPlayerRating, NewRatingAdjustment},
    structures::{rating_adjustment_type::RatingAdjustmentType, ruleset::Ruleset}
};
use chrono::{DateTime, FixedOffset};
use constants::OSU_RATING_FLOOR;
use std::collections::HashMap;
use strum::IntoEnumIterator;

pub fn initial_ratings(players: &[NewPlayer]) -> HashMap<(i32, Ruleset), NewPlayerRating> {
    let mut map = HashMap::new();

    for player in players {
        let initial_ratings = create_initial_ratings(player);
        for rating in initial_ratings {
            map.insert((player.id, rating.ruleset), rating);
        }
    }

    map
}

fn create_initial_ratings(player: &NewPlayer) -> Vec<NewPlayerRating> {
    let timestamp: DateTime<FixedOffset> = "2007-09-16T00:00:00-00:00".parse().unwrap();
    let mut ratings = Vec::new();

    for ruleset in Ruleset::iter() {
        let rating = initial_rating(player, &ruleset);
        let adjustment = NewRatingAdjustment {
            player_id: player.id,
            player_rating_id: 0, // ?
            match_id: None,
            rating_before: 0.0,
            rating_after: rating,
            volatility_before: 0.0,
            volatility_after: DEFAULT_VOLATILITY,
            timestamp,
            adjustment_type: RatingAdjustmentType::Initial
        };

        ratings.push(NewPlayerRating {
            id: 0,
            player_id: player.id,
            ruleset,
            rating,
            volatility: DEFAULT_VOLATILITY,
            percentile: 0.0,
            global_rank: 0,
            country_rank: 0,
            adjustments: vec![adjustment]
        });
    }

    ratings
}

fn initial_rating(player: &NewPlayer, ruleset: &Ruleset) -> f64 {
    let ruleset_data = player.ruleset_data.iter().find(|rd| rd.ruleset == *ruleset);
    let rank = ruleset_data.and_then(|rd| rd.earliest_global_rank.or_else(|| rd.global_rank));

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
    use super::create_initial_ratings;
    use crate::{
        model::{
            constants::{DEFAULT_VOLATILITY, OSU_RATING_CEILING, OSU_RATING_FLOOR},
            db_structs::{NewPlayer, Player},
            rating_utils::{mu_from_rank, std_dev_from_ruleset},
            structures::{
                rating_adjustment_type::RatingAdjustmentType,
                ruleset::Ruleset::{Catch, Mania4k, Mania7k, Osu, Taiko}
            }
        },
        utils::test_utils::{generate_player_rating, generate_ruleset_data}
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
        let player = NewPlayer {
            id: 1,
            username: Some("Test".to_string()),
            country: None,
            ruleset_data: vec![
                generate_ruleset_data(Osu, Some(1), None),
                generate_ruleset_data(Taiko, Some(1), None),
                generate_ruleset_data(Catch, Some(1), None),
                generate_ruleset_data(Mania4k, Some(1), None),
                generate_ruleset_data(Mania7k, Some(1), None),
            ]
        };

        let expected_osu = mu_from_rank(1, Osu);
        let expected_taiko = mu_from_rank(1, Taiko);
        let expected_catch = mu_from_rank(1, Catch);
        let expected_mania4k = mu_from_rank(1, Mania4k);
        let expected_mania7k = mu_from_rank(1, Mania7k);

        let actual_osu = super::initial_rating(&player, &Osu);
        let actual_taiko = super::initial_rating(&player, &Taiko);
        let actual_catch = super::initial_rating(&player, &Catch);
        let actual_mania_4k = super::initial_rating(&player, &Mania4k);
        let actual_mania_7k = super::initial_rating(&player, &Mania7k);

        assert_eq!(expected_osu, actual_osu);
        assert_eq!(expected_taiko, actual_taiko);
        assert_eq!(expected_catch, actual_catch);
        assert_eq!(expected_mania4k, actual_mania_4k);
        assert_eq!(expected_mania7k, actual_mania_7k);
    }

    #[test]
    fn test_create_initial_adjustment() {
        let player = NewPlayer {
            id: 0,
            username: Some("Test".to_string()),
            country: None,
            ruleset_data: vec![
                generate_ruleset_data(Osu, Some(1), None),
                generate_ruleset_data(Taiko, Some(1), None),
                generate_ruleset_data(Catch, Some(1), None),
                generate_ruleset_data(Mania4k, Some(1), None),
                generate_ruleset_data(Mania7k, Some(1), None),
            ]
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

        let initial_ratings = create_initial_ratings(&player);

        let actual_osu = initial_ratings.iter().find(|r| r.ruleset == Osu).unwrap();
        let actual_taiko = initial_ratings.iter().find(|r| r.ruleset == Taiko).unwrap();
        let actual_catch = initial_ratings.iter().find(|r| r.ruleset == Catch).unwrap();
        let actual_mania4k = initial_ratings.iter().find(|r| r.ruleset == Mania4k).unwrap();
        let actual_mania7k = initial_ratings.iter().find(|r| r.ruleset == Mania7k).unwrap();

        assert_eq!(expected_osu.rating, actual_osu.rating);
        assert_eq!(expected_osu.volatility, actual_osu.volatility);
        assert_eq!(
            expected_osu.adjustment_type,
            actual_osu.adjustments.first().unwrap().adjustment_type
        );

        assert_eq!(expected_taiko.rating, actual_taiko.rating);
        assert_eq!(expected_taiko.volatility, actual_taiko.volatility);
        assert_eq!(
            expected_taiko.adjustment_type,
            actual_taiko.adjustments.first().unwrap().adjustment_type
        );

        assert_eq!(expected_catch.rating, actual_catch.rating);
        assert_eq!(expected_catch.volatility, actual_catch.volatility);
        assert_eq!(
            expected_catch.adjustment_type,
            actual_catch.adjustments.first().unwrap().adjustment_type
        );

        assert_eq!(expected_mania4k.rating, actual_mania4k.rating);
        assert_eq!(expected_mania4k.volatility, actual_mania4k.volatility);
        assert_eq!(
            expected_mania4k.adjustment_type,
            actual_mania4k.adjustments.first().unwrap().adjustment_type
        );

        assert_eq!(expected_mania7k.rating, actual_mania7k.rating);
        assert_eq!(expected_mania7k.volatility, actual_mania7k.volatility);
        assert_eq!(
            expected_mania7k.adjustment_type,
            actual_mania7k.adjustments.first().unwrap().adjustment_type
        );
    }
}
