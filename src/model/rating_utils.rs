use super::constants::FALLBACK_RATING;
use crate::{
    database::db_structs::{Match, Player, PlayerRating, RatingAdjustment},
    model::{
        constants,
        constants::{DEFAULT_VOLATILITY, MULTIPLIER, OSU_INITIAL_RATING_CEILING},
        structures::{rating_adjustment_type::RatingAdjustmentType, ruleset::Ruleset}
    },
    utils::progress_utils::progress_bar
};
use chrono::{DateTime, Duration, FixedOffset};
use constants::OSU_INITIAL_RATING_FLOOR;
use std::{collections::HashMap, ops::Sub};

pub fn create_initial_ratings(players: &[Player], matches: &[Match]) -> Vec<PlayerRating> {
    // Identify which players have played in each ruleset
    let mut ruleset_activity: HashMap<Ruleset, HashMap<i32, DateTime<FixedOffset>>> = HashMap::new();

    let p_bar = progress_bar(
        matches.len() as u64,
        "Identifying player ruleset participation".to_string()
    );
    for match_ in matches {
        for game in &match_.games {
            for score in &game.scores {
                // Store the player id and match start time.
                // Allows us to accurately set the timestamp of the initial rating adjustment
                // and avoid creating initial adjustments for players who are inactive in
                // any ruleset.
                ruleset_activity
                    .entry(match_.ruleset)
                    .or_default()
                    .entry(score.player_id)
                    .or_insert(match_.start_time);
            }
        }

        if let Some(bar) = &p_bar {
            bar.inc(1);
        }
    }

    if let Some(bar) = &p_bar {
        bar.finish_with_message("Initial ratings created");
    }

    let mut ratings = Vec::new();
    for player in players {
        for ruleset in ruleset_activity.keys() {
            if let Some(ruleset_entry) = ruleset_activity.get(ruleset) {
                if ruleset_entry.get(&player.id).is_none() {
                    // Player has not played in this ruleset
                    continue;
                }
            }

            let rating = initial_rating(player, ruleset);
            if let Some(timestamp) = ruleset_activity.get(ruleset).unwrap().get(&player.id) {
                let adjustment = RatingAdjustment {
                    player_id: player.id,
                    ruleset: *ruleset,
                    match_id: None,
                    rating_before: 0.0,
                    rating_after: rating,
                    volatility_before: 0.0,
                    volatility_after: DEFAULT_VOLATILITY,
                    timestamp: timestamp.sub(Duration::seconds(1)),
                    adjustment_type: RatingAdjustmentType::Initial
                };

                if rating.is_nan() || rating <= 0.0 {
                    panic!("Initial rating is NaN or <= 0.0 for player: {:?}", player);
                }

                ratings.push(PlayerRating {
                    id: 0, // database id, leave default
                    player_id: player.id,
                    ruleset: *ruleset,
                    rating,
                    volatility: DEFAULT_VOLATILITY,
                    // percentile, global_rank, and country_rank
                    // are managed by the rating_tracker
                    percentile: 0.0,
                    global_rank: 0,
                    country_rank: 0,
                    adjustments: vec![adjustment]
                });
            }
        }
    }

    ratings
}

fn initial_rating(player: &Player, ruleset: &Ruleset) -> f64 {
    match &player.ruleset_data {
        Some(data) => {
            let ruleset_data = data.iter().find(|rd| rd.ruleset == *ruleset);
            let rank = ruleset_data.and_then(|rd| rd.earliest_global_rank.or(Some(rd.global_rank)));

            match rank {
                Some(r) => mu_from_rank(r, *ruleset),
                None => FALLBACK_RATING
            }
        }
        None => FALLBACK_RATING
    }
}

fn mu_from_rank(rank: i32, ruleset: Ruleset) -> f64 {
    let left_slope = 4.0;
    let right_slope = 3.0;

    let mean = mean_from_ruleset(ruleset);
    let std_dev = std_dev_from_ruleset(ruleset);

    let z = (rank as f64 / mean.exp()).ln() / std_dev;
    let val = MULTIPLIER * (18.0 - (if z > 0.0 { left_slope } else { right_slope }) * z);

    if val < OSU_INITIAL_RATING_FLOOR {
        return OSU_INITIAL_RATING_FLOOR;
    }

    if val > OSU_INITIAL_RATING_CEILING {
        return OSU_INITIAL_RATING_CEILING;
    }

    val
}

fn mean_from_ruleset(ruleset: Ruleset) -> f64 {
    match ruleset {
        Ruleset::Osu => 9.91,
        Ruleset::Taiko => 7.59,
        Ruleset::Catch => 6.75,
        Ruleset::Mania4k | Ruleset::Mania7k | Ruleset::ManiaOther => 8.18
    }
}

fn std_dev_from_ruleset(ruleset: Ruleset) -> f64 {
    match ruleset {
        Ruleset::Osu => 1.59,
        Ruleset::Taiko => 1.56,
        Ruleset::Catch => 1.54,
        Ruleset::Mania4k | Ruleset::Mania7k | Ruleset::ManiaOther => 1.55
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        database::db_structs::Player,
        model::{
            constants::{OSU_INITIAL_RATING_CEILING, OSU_INITIAL_RATING_FLOOR},
            rating_utils::{mu_from_rank, std_dev_from_ruleset},
            structures::ruleset::Ruleset::{Catch, Mania4k, ManiaOther, Osu, Taiko}
        },
        utils::test_utils::generate_ruleset_data
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
        let actual_4k = std_dev_from_ruleset(ManiaOther);
        let actual_7k = std_dev_from_ruleset(Mania4k);

        assert_eq!(expected, actual_4k);
        assert_eq!(expected, actual_7k);
    }

    #[test]
    fn test_mu_from_rank_maximum() {
        let rank = 1;
        let expected_mu = OSU_INITIAL_RATING_CEILING;

        let actual_mu_osu = mu_from_rank(rank, Osu);
        let actual_mu_taiko = mu_from_rank(rank, Taiko);
        let actual_mu_catch = mu_from_rank(rank, Catch);
        let actual_mu_mania_4k = mu_from_rank(rank, ManiaOther);
        let actual_mu_mania_7k = mu_from_rank(rank, Mania4k);

        assert_eq!(expected_mu, actual_mu_osu);
        assert_eq!(expected_mu, actual_mu_taiko);
        assert_eq!(expected_mu, actual_mu_catch);
        assert_eq!(expected_mu, actual_mu_mania_4k);
        assert_eq!(expected_mu, actual_mu_mania_7k);
    }

    #[test]
    fn test_mu_from_rank_minimum() {
        let rank = 10_000_000;
        let expected_mu = OSU_INITIAL_RATING_FLOOR;

        let actual_mu_osu = mu_from_rank(rank, Osu);
        let actual_mu_taiko = mu_from_rank(rank, Taiko);
        let actual_mu_catch = mu_from_rank(rank, Catch);
        let actual_mu_mania_4k = mu_from_rank(rank, ManiaOther);
        let actual_mu_mania_7k = mu_from_rank(rank, Mania4k);

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
            // Player who is rank 1 in everything. wow!
            ruleset_data: Some(vec![
                generate_ruleset_data(Osu, 1, None),
                generate_ruleset_data(Taiko, 1, None),
                generate_ruleset_data(Catch, 1, None),
                generate_ruleset_data(ManiaOther, 1, None),
                generate_ruleset_data(Mania4k, 1, None),
            ])
        };

        let expected_osu = mu_from_rank(1, Osu);
        let expected_taiko = mu_from_rank(1, Taiko);
        let expected_catch = mu_from_rank(1, Catch);
        let expected_mania4k = mu_from_rank(1, ManiaOther);
        let expected_mania7k = mu_from_rank(1, Mania4k);

        let actual_osu = super::initial_rating(&player, &Osu);
        let actual_taiko = super::initial_rating(&player, &Taiko);
        let actual_catch = super::initial_rating(&player, &Catch);
        let actual_mania_4k = super::initial_rating(&player, &ManiaOther);
        let actual_mania_7k = super::initial_rating(&player, &Mania4k);

        assert_eq!(expected_osu, actual_osu);
        assert_eq!(expected_taiko, actual_taiko);
        assert_eq!(expected_catch, actual_catch);
        assert_eq!(expected_mania4k, actual_mania_4k);
        assert_eq!(expected_mania7k, actual_mania_7k);
    }
}
