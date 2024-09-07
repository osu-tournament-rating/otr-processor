use indexmap::IndexMap;
use itertools::Itertools;
use std::{cmp::Ordering, collections::HashMap};

use crate::model::{
    db_structs::{PlayerRating, RatingAdjustment},
    structures::ruleset::Ruleset
};

pub struct RatingTracker {
    // Global leaderboard, used as a reference for country leaderboards also.
    // When country ratings are updated, the global leaderboard is updated as well
    // to reflect the new country rank for the specific ruleset.
    // The `percentile`, `country_rank`, and `global_rank` values are updated through this IndexMap.
    // -- Does not store any information on the adjustments --
    leaderboard: IndexMap<(i32, Ruleset), PlayerRating>,
    // The PlayerRating here is used as a reference. The rankings are NOT updated here, but the
    // other values are affected by `insert_or_updated`.
    country_leaderboards: HashMap<String, IndexMap<(i32, Ruleset), PlayerRating>>,
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
            country_mapping: HashMap::new()
        }
    }

    pub fn get_all_ratings(&self) -> Vec<PlayerRating> {
        self.leaderboard.values().cloned().collect()
    }

    pub fn set_country_mapping(&mut self, country_mapping: HashMap<i32, String>) {
        self.country_mapping = country_mapping;
    }

    /// Inserts or updates a set of player ratings into the tracker.
    /// Ratings are assumed to be inserted on a per-match basis.
    pub fn insert_or_update(&mut self, ratings: &[PlayerRating]) {
        // Update the leaderboard with the current player rating information
        // (usually, this is done after a match is processed & ratings have been updated)
        for rating in ratings {
            let cloned_rating = rating.clone();

            self.leaderboard
                .insert((rating.player_id, rating.ruleset), cloned_rating.clone());
        }
    }

    /// Returns the current rating value for the player and the ruleset.
    pub fn get_rating(&self, player_id: i32, ruleset: Ruleset) -> Option<&PlayerRating> {
        self.leaderboard.get(&(player_id, ruleset))
    }

    pub fn get_country(&self, player_id: i32) -> Option<&String> {
        self.country_mapping.get(&player_id)
    }

    pub fn get_rating_adjustments(&self, player_id: i32, ruleset: Ruleset) -> Option<Vec<RatingAdjustment>> {
        self.get_rating(player_id, ruleset)
            .map(|rating| rating.adjustments.clone())
    }

    /// Sorts and updates the PlayerRating global_rank, country_rank, and percentile values.
    pub fn sort(&mut self) {
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

            // Sort the ruleset-specific leaderboard
            let ruleset_leaderboard: Vec<_> = self
                .leaderboard
                .iter_mut()
                .filter(|(_, player_rating)| player_rating.ruleset == *ruleset)
                .sorted_by(|(_, a), (_, b)| b.rating.partial_cmp(&a.rating).unwrap())
                .collect();
            let count = ruleset_leaderboard.len() as i32;

            for (_, rating) in ruleset_leaderboard {
                rating.global_rank = global_rank;
                rating.percentile =
                    RatingTracker::percentile(global_rank, count).expect("Failed to calculate percentile");
                global_rank += 1;
            }
        }

        // Slot all players from main leaderboard into country leaderboard
        for (player_id, country) in &self.country_mapping {
            for ruleset in rulesets.iter() {
                if let Some(player_rating) = self.leaderboard.get(&(*player_id, *ruleset)) {
                    let country_leaderboard = self
                        .country_leaderboards
                        .entry(country.clone())
                        .or_default();
                    country_leaderboard.insert((*player_id, *ruleset), player_rating.clone());
                }
            }
        }

        // Update country rankings
        for country_leaderboard in self.country_leaderboards.values() {
            for ruleset in rulesets.iter() {
                let mut country_rank = 1;

                // Clone the iterator to get the count without consuming it
                let country_ruleset_leaderboard: Vec<_> = country_leaderboard
                    .iter()
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
            structures::{
                ruleset::{Ruleset, Ruleset::Osu}
            }
        },
        utils::test_utils::{generate_country_mapping_player_ratings, generate_player_rating}
    };
    use approx::assert_abs_diff_eq;

    #[test]
    fn test_track_player_initial_rating_and_match_update() {
        let mut rating_tracker = RatingTracker::new();

        // Initialize new player
        let player_ratings = vec![generate_player_rating(1, Osu, DEFAULT_RATING, DEFAULT_VOLATILITY, 1)];

        let country_mapping = generate_country_mapping_player_ratings(player_ratings.as_slice(), "US");
        rating_tracker.set_country_mapping(country_mapping);
        rating_tracker.insert_or_update(&player_ratings);

        // Verify the player was added to the leaderboard and does not have data for another ruleset
        let player_rating = rating_tracker.get_rating(1, Osu).unwrap();

        assert_eq!(player_rating.player_id, 1);
        assert_eq!(player_rating.adjustments.len(), 1); // First adjustment contains the default adjustment

        // Update player with a new match result - overrides previous value
        let player_ratings = vec![generate_player_rating(1, Ruleset::Osu, 200.0, 85.0, 2)];
        rating_tracker.insert_or_update(&player_ratings);

        // Verify the player was updated with the new rating and has an adjustment
        let verify_rating = rating_tracker.get_rating(1, Ruleset::Osu).unwrap();

        assert_eq!(verify_rating.rating, 200.0);
        assert_eq!(verify_rating.volatility, 85.0);
        assert_eq!(verify_rating.adjustments.len(), 2);
    }

    #[test]
    fn test_sort() {
        let mut rating_tracker = RatingTracker::new();
        let player_ratings = vec![
            generate_player_rating(1, Osu, 100.0, 100.0, 1),
            generate_player_rating(2, Osu, 200.0, 100.0, 1),
        ];

        let country_mapping = generate_country_mapping_player_ratings(&player_ratings, "US");
        rating_tracker.set_country_mapping(country_mapping);
        rating_tracker.insert_or_update(&player_ratings);

        let p1 = rating_tracker
            .get_rating(1, Osu)
            .expect("Expected to find rating for Player 1 in ruleset Osu");

        let p2 = rating_tracker
            .get_rating(2, Osu)
            .expect("Expected to find rating for Player 2 in ruleset Osu");

        // Assert global ranks are different from what they should be
        assert_abs_diff_eq!(p1.global_rank, 0);
        assert_abs_diff_eq!(p2.global_rank, 0);

        assert_abs_diff_eq!(p1.country_rank, 0);
        assert_abs_diff_eq!(p2.country_rank, 0);

        assert_abs_diff_eq!(p1.percentile, 0.0);
        assert_abs_diff_eq!(p2.percentile, 0.0);

        // Sort updates the global & country rankings for all users
        rating_tracker.sort();

        let p1 = rating_tracker
            .get_rating(1, Osu)
            .expect("Expected to find rating for Player 1 in ruleset Osu");
        let p2 = rating_tracker
            .get_rating(2, Osu)
            .expect("Expected to find rating for Player 2 in ruleset Osu");

        assert_eq!(p1.global_rank, 2);
        assert_eq!(p2.global_rank, 1);

        assert_eq!(p1.country_rank, 2);
        assert_eq!(p2.country_rank, 1);

        assert_abs_diff_eq!(p1.percentile, RatingTracker::percentile(2, 2).unwrap());
        assert_abs_diff_eq!(p2.percentile, RatingTracker::percentile(1, 2).unwrap());
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
}
