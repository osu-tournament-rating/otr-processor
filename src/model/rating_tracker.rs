use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet}
};

use indexmap::IndexMap;
use itertools::Itertools;

use crate::model::{
    db_structs::{NewPlayerRating, NewRatingAdjustment, PlayerRating, RatingAdjustment, RatingPost},
    structures::{rating_adjustment_type::RatingAdjustmentType, ruleset::Ruleset}
};

pub struct RatingTracker {
    // Global leaderboard, used as a reference for country leaderboards also.
    // When country ratings are updated, the global leaderboard is updated as well
    // to reflect the new country rank for the specific ruleset.
    // The `percentile`, `country_rank`, and `global_rank` values are updated through this IndexMap.
    // -- Does not store any information on the adjustments --
    leaderboard: IndexMap<(i32, Ruleset), NewPlayerRating>,
    // The PlayerRating here is used as a reference. The rankings are NOT updated here, but the
    // other values are affected by `insert_or_updated`.
    country_leaderboards: HashMap<String, IndexMap<(i32, Ruleset), NewPlayerRating>>,
    country_change_tracker: HashSet<String>, // This is so we don't have to update EVERY country with each update
    country_mapping: HashMap<i32, String>
}

impl Default for RatingTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl RatingTracker {
    pub fn new() -> RatingTracker {
        RatingTracker {
            leaderboard: IndexMap::new(),
            country_leaderboards: HashMap::new(),
            country_change_tracker: HashSet::new(),
            country_mapping: HashMap::new()
        }
    }

    fn track_country(&mut self, country: &str) {
        self.country_change_tracker.insert(country.to_owned());
    }

    /// Inserts or updates a set of player ratings into the tracker.
    /// Ratings are assumed to be inserted on a per-match basis.
    pub fn insert_or_update(&mut self, ratings: &[NewPlayerRating], country_mapping: &HashMap<i32, String>) {
        let countries: HashSet<&String> = country_mapping.values().collect();

        for country in countries {
            self.track_country(country.as_str());
        }

        for player_id in country_mapping.keys() {
            self.country_mapping
                .insert(*player_id, country_mapping.get(player_id).unwrap().to_string());
        }

        // Update the leaderboard with the current player rating information
        // (usually, this is done after a match is processed & ratings have been updated)
        for rating in ratings {
            let cloned_rating = rating.clone();

            self.leaderboard
                .insert((rating.player_id, rating.ruleset), cloned_rating.clone());

            // Get the player's country leaderboard and insert the player rating.
            // The country rankings themselves will be determined at the end of processing,
            // this just stores a record in the leaderboard for use in that step.
            let player_country = country_mapping.get(&rating.player_id).unwrap();
            self.country_leaderboards
                .entry(player_country.to_string())
                .or_default()
                .insert((rating.player_id, rating.ruleset), cloned_rating);
        }
    }

    /// Returns the current rating value for the player and the ruleset.
    pub fn get_rating(&self, player_id: i32, ruleset: Ruleset) -> Option<&NewPlayerRating> {
        self.leaderboard.get(&(player_id, ruleset))
    }

    pub fn get_country(&self, player_id: i32) -> Option<&String> {
        self.country_mapping.get(&player_id)
    }

    pub fn get_rating_adjustments(&self, player_id: i32, ruleset: Ruleset) -> Option<Vec<NewRatingAdjustment>> {
        self.get_rating(player_id, ruleset).map(|rating| rating.adjustments.clone())
    }

    /// Sorts and updates the PlayerRating global_rank, country_rank, and percentile values.
    pub fn sort(&mut self) {
        // Sort leaderboard by rating
        self.leaderboard
            .sort_by(|k1, v1, k2, v2| v2.rating.partial_cmp(&v1.rating).unwrap());

        // Iterate updating global rankings and percentiles
        let rulesets = [
            Ruleset::Osu,
            Ruleset::Taiko,
            Ruleset::Catch,
            Ruleset::Mania4k,
            Ruleset::Mania7k
        ];

        for ruleset in rulesets.iter() {
            let mut global_rank = 1;

            // Clone the iterator to get the count without consuming it
            let ruleset_leaderboard: Vec<_> = self
                .leaderboard
                .iter_mut()
                .filter(|(_, player_rating)| player_rating.ruleset == *ruleset)
                .collect();
            let count = ruleset_leaderboard.len() as i32;

            for (_, rating) in ruleset_leaderboard {
                rating.global_rank = global_rank;
                rating.percentile =
                    RatingTracker::percentile(global_rank, count).expect("Failed to calculate percentile");
                global_rank += 1;
            }
        }

        // Update country rankings
        let changed_countries: Vec<&String> = self.country_change_tracker.iter().collect();
        let country_leaderboards = self
            .country_leaderboards
            .iter_mut()
            .filter(|(country, _)| changed_countries.contains(country));
        for (_, country_leaderboard) in country_leaderboards {
            for ruleset in rulesets.iter() {
                let mut country_rank = 1;

                // Clone the iterator to get the count without consuming it
                let country_ruleset_leaderboard: Vec<_> = country_leaderboard
                    .iter_mut()
                    .filter(|(_, player_rating)| player_rating.ruleset == *ruleset)
                    .sorted_by(|(_, a), (_, b)| b.rating.partial_cmp(&a.rating).unwrap())
                    .collect();

                for (_, rating) in country_ruleset_leaderboard {
                    // This tracks the item in the appropriate "primary" leaderboard.
                    let associated_entry = self
                        .leaderboard
                        .get_mut(&(rating.player_id, rating.ruleset))
                        .expect("Failed to find associated entry in global leaderboard");

                    associated_entry.country_rank = country_rank;
                    country_rank += 1;
                }
            }
        }

        self.country_change_tracker.clear();
    }

    /// `P = (n/N) * 100`
    fn percentile(rank: i32, total: i32) -> Option<f64> {
        match rank.cmp(&1) {
            Ordering::Less => None,
            _ => {
                let n = total - rank; // The number of players below the player
                Some(n as f64 / total as f64 * 100.0)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        model::{
            constants::{DEFAULT_RATING, DEFAULT_VOLATILITY},
            rating_tracker::RatingTracker,
            structures::rating_adjustment_type::RatingAdjustmentType,
            structures::ruleset::Ruleset,
            structures::ruleset::Ruleset::Osu,
        },
        utils::test_utils::{generate_country_mapping, generate_player_rating}
    };
    use approx::assert_abs_diff_eq;

    #[test]
    fn test_track_player_initial_rating_and_match_update() {
        let mut rating_tracker = RatingTracker::new();

        // Initialize new player
        let player_ratings = vec![generate_player_rating(
            1,
            Osu,
            DEFAULT_RATING,
            DEFAULT_VOLATILITY,
            1
        )];

        let country_mapping = generate_country_mapping(player_ratings.as_slice(), "US");
        rating_tracker.insert_or_update(&player_ratings, &country_mapping);

        // Verify the player was added to the leaderboard and does not have data for another ruleset
        let player_rating = rating_tracker.get_rating(1, Osu).unwrap();

        assert_eq!(player_rating.player_id, 1);
        assert_eq!(player_rating.adjustments.len(), 1); // First adjustment contains the default adjustment

        // Update player with a new match result - overrides previous value
        let player_ratings = vec![generate_player_rating(1, Ruleset::Osu, 200.0, 85.0, 2)];
        rating_tracker.insert_or_update(&player_ratings, &country_mapping);

        // Verify the player was updated with the new rating and has an adjustment
        let verify_rating = rating_tracker.get_rating(1, Ruleset::Osu).unwrap();

        assert_eq!(verify_rating.rating, 200.0);
        assert_eq!(verify_rating.volatility, 85.0);
        assert_eq!(verify_rating.adjustments.len(), 2);
    }

    #[test]
    fn test_leaderboard_update() {
        let mut rating_tracker = RatingTracker::new();
        let player_ratings = vec![
            generate_player_rating(1, Osu, 100.0, 100.0, 1),
            generate_player_rating(2, Osu, 200.0, 100.0, 1),
        ];

        let country_mapping = generate_country_mapping(&player_ratings, "US");
        rating_tracker.insert_or_update(&player_ratings, &country_mapping);

        // Assert sorted by rating descending
        assert_eq!(rating_tracker.leaderboard.len(), 2);
        assert_abs_diff_eq!(rating_tracker.leaderboard.get_index(0).unwrap().1.rating, 200.0);
        assert_abs_diff_eq!(rating_tracker.leaderboard.get_index(1).unwrap().1.rating, 100.0);

        let p1 = rating_tracker
            .get_rating(1, Ruleset::Osu)
            .expect("Expected to find rating for Player 1 in ruleset Osu");
        let p2 = rating_tracker
            .get_rating(2, Ruleset::Osu)
            .expect("Expected to find rating for Player 2 in ruleset Osu");

        assert_eq!(p1.global_rank, 2);
        assert_eq!(p2.global_rank, 1);

        assert_eq!(p1.country_rank, 2);
        assert_eq!(p2.country_rank, 1);

        assert_abs_diff_eq!(p1.percentile, RatingTracker::percentile(2, 2).unwrap());
        assert_abs_diff_eq!(p2.percentile, RatingTracker::percentile(1, 2).unwrap());
    }

    #[test]
    fn test_initial_rating_adjustment() {
        let mut rating_tracker = RatingTracker::new();
        let player_ratings = vec![
            generate_player_rating(1, Osu, 100.0, 100.0, 1),
            generate_player_rating(2, Osu, 200.0, 100.0, 1),
        ];
        let country_mapping = generate_country_mapping(&player_ratings, "US");

        rating_tracker.insert_or_update(&player_ratings, &country_mapping);
        rating_tracker.sort();

        let adj_1 = rating_tracker.get_rating_adjustments(1, Osu).unwrap();
        let adj_2 = rating_tracker.get_rating_adjustments(2, Osu).unwrap();

        assert_eq!(adj_1.len(), 1);
        assert_eq!(adj_2.len(), 1);

        assert_eq!(adj_1[0].rating_before, 100.0);
        assert_eq!(adj_1[0].rating_after, 100.0);
        assert_eq!(adj_1[0].volatility_before, 100.0);
        assert_eq!(adj_1[0].volatility_after, 100.0);

        assert_eq!(adj_2[0].rating_before, 200.0);
        assert_eq!(adj_2[0].rating_after, 200.0);
        assert_eq!(adj_2[0].volatility_before, 100.0);
        assert_eq!(adj_2[0].volatility_after, 100.0);
    }

    #[test]
    fn test_percentile() {
        assert_eq!(RatingTracker::percentile(0, 10), None);
        assert_eq!(RatingTracker::percentile(-1, 10), None);

        assert_eq!(RatingTracker::percentile(1, 1), Some(0.0));

        assert_abs_diff_eq!(RatingTracker::percentile(1, 2).unwrap(), 50.0, epsilon = 0.0001);
        assert_abs_diff_eq!(RatingTracker::percentile(2, 2).unwrap(), 0.0, epsilon = 0.0001);

        assert_abs_diff_eq!(RatingTracker::percentile(1, 10).unwrap(), 90.0, epsilon = 0.0001);
        assert_abs_diff_eq!(RatingTracker::percentile(1, 100).unwrap(), 99.0, epsilon = 0.0001);
        assert_abs_diff_eq!(RatingTracker::percentile(1, 1000).unwrap(), 99.9, epsilon = 0.0001);
        assert_abs_diff_eq!(RatingTracker::percentile(1, 10000).unwrap(), 99.99, epsilon = 0.0001);
        assert_abs_diff_eq!(RatingTracker::percentile(1, 100000).unwrap(), 99.999, epsilon = 0.0001);
        assert_abs_diff_eq!(
            RatingTracker::percentile(1, 1000000).unwrap(),
            99.9999,
            epsilon = 0.0001
        );
    }

    #[test]
    fn test_country_change_tracker() {
        let mut rating_tracker = RatingTracker::new();
        let player_ratings = vec![
            generate_player_rating(1, Osu, 100.0, 100.0, 1),
            generate_player_rating(2, Osu, 200.0, 100.0, 1),
        ];
        let country_mapping = generate_country_mapping(&player_ratings, "US");

        rating_tracker.insert_or_update(&player_ratings, &country_mapping);
        assert_eq!(rating_tracker.country_change_tracker.len(), 1);
    }
}
