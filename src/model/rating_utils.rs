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
                    panic!("Initial rating is NaN or <= 0.0 for: {player:?}");
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
            // Special handling for Mania4k and Mania7k.
            // This is here because osu!track cannot track Mania4k and Mania7k separately.
            // Thus, a player_osu_ruleset_data entry exists for ManiaOther which must be
            // used specifically for earliest global rank info for these rulesets.
            // Using the overall mania rank for the initial ratingis close enough in accuracy
            // for our purposes.
            if matches!(ruleset, Ruleset::Mania4k | Ruleset::Mania7k) {
                // First, try to get earliest_global_rank from ManiaOther (ruleset 3)
                if let Some(mania_other_data) = data.iter().find(|rd| rd.ruleset == Ruleset::ManiaOther) {
                    if let Some(earliest_rank) = mania_other_data.earliest_global_rank {
                        return mu_from_rank(earliest_rank, *ruleset);
                    }
                }

                // Fallback: use global_rank from the respective ruleset
                if let Some(ruleset_data) = data.iter().find(|rd| rd.ruleset == *ruleset) {
                    return mu_from_rank(ruleset_data.global_rank, *ruleset);
                }

                // If no data found, use fallback rating and log a warning
                log::warn!("No data found for player, falling back to default initial rating: [player_id: {:?}, ruleset: {:?}]", player.id, ruleset);
                return FALLBACK_RATING;
            }

            // Handle other rulesets as normal
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
            constants::{FALLBACK_RATING, OSU_INITIAL_RATING_CEILING, OSU_INITIAL_RATING_FLOOR},
            rating_utils::{mu_from_rank, std_dev_from_ruleset},
            structures::ruleset::Ruleset::{Catch, Mania4k, Mania7k, ManiaOther, Osu, Taiko}
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
                generate_ruleset_data(Mania4k, 1, None),
                generate_ruleset_data(Mania7k, 1, None),
            ])
        };

        let expected_osu = mu_from_rank(1, Osu);
        let expected_taiko = mu_from_rank(1, Taiko);
        let expected_catch = mu_from_rank(1, Catch);
        let expected_mania4k = mu_from_rank(1, Mania4k);
        let expected_mania7k = mu_from_rank(1, Mania7k);

        let actual_osu = super::initial_rating(&player, &Osu);
        let actual_taiko = super::initial_rating(&player, &Taiko);
        let actual_catch = super::initial_rating(&player, &Catch);
        let actual_mania4k = super::initial_rating(&player, &Mania4k);
        let actual_mania7k = super::initial_rating(&player, &Mania7k);

        assert_eq!(expected_osu, actual_osu);
        assert_eq!(expected_taiko, actual_taiko);
        assert_eq!(expected_catch, actual_catch);
        assert_eq!(expected_mania4k, actual_mania4k);
        assert_eq!(expected_mania7k, actual_mania7k);
    }

    #[test]
    fn test_mania4k_mania7k_initial_rating_logic() {
        use crate::model::structures::ruleset::Ruleset::*;

        // Test case 1: Player with ManiaOther earliest_global_rank - should be preferred
        let player_with_mania_other_earliest = Player {
            id: 1,
            username: Some("TestPlayer1".to_string()),
            country: None,
            ruleset_data: Some(vec![
                generate_ruleset_data(ManiaOther, 5000, Some(1000)), // earliest_global_rank = 1000
                generate_ruleset_data(Mania4k, 2000, None),          // global_rank = 2000
                generate_ruleset_data(Mania7k, 3000, None),          // global_rank = 3000
            ])
        };

        // Both Mania4k and Mania7k should use ManiaOther's earliest_global_rank (1000)
        let mania4k_rating = super::initial_rating(&player_with_mania_other_earliest, &Mania4k);
        let mania7k_rating = super::initial_rating(&player_with_mania_other_earliest, &Mania7k);
        let expected_rating_from_mania_other = mu_from_rank(1000, Mania4k); // Using rank 1000 with Mania4k ruleset
        let expected_rating_from_mania_other_7k = mu_from_rank(1000, Mania7k); // Using rank 1000 with Mania7k ruleset

        assert_eq!(mania4k_rating, expected_rating_from_mania_other);
        assert_eq!(mania7k_rating, expected_rating_from_mania_other_7k);

        // Test case 2: Player without ManiaOther earliest_global_rank - should use respective ruleset global_rank
        let player_without_mania_other_earliest = Player {
            id: 2,
            username: Some("TestPlayer2".to_string()),
            country: None,
            ruleset_data: Some(vec![
                generate_ruleset_data(ManiaOther, 5000, None),    // No earliest_global_rank
                generate_ruleset_data(Mania4k, 2000, Some(1500)), // global_rank = 2000, earliest = 1500
                generate_ruleset_data(Mania7k, 3000, Some(2500)), // global_rank = 3000, earliest = 2500
            ])
        };

        let mania4k_rating_fallback = super::initial_rating(&player_without_mania_other_earliest, &Mania4k);
        let mania7k_rating_fallback = super::initial_rating(&player_without_mania_other_earliest, &Mania7k);
        let expected_mania4k_fallback = mu_from_rank(2000, Mania4k); // Using Mania4k global_rank
        let expected_mania7k_fallback = mu_from_rank(3000, Mania7k); // Using Mania7k global_rank

        assert_eq!(mania4k_rating_fallback, expected_mania4k_fallback);
        assert_eq!(mania7k_rating_fallback, expected_mania7k_fallback);

        // Test case 3: Player with no relevant data - should use FALLBACK_RATING
        let player_no_mania_data = Player {
            id: 3,
            username: Some("TestPlayer3".to_string()),
            country: None,
            ruleset_data: Some(vec![
                generate_ruleset_data(Osu, 1000, None),
                generate_ruleset_data(Taiko, 2000, None),
            ])
        };

        let mania4k_rating_no_data = super::initial_rating(&player_no_mania_data, &Mania4k);
        let mania7k_rating_no_data = super::initial_rating(&player_no_mania_data, &Mania7k);

        assert_eq!(mania4k_rating_no_data, FALLBACK_RATING);
        assert_eq!(mania7k_rating_no_data, FALLBACK_RATING);
    }
}
