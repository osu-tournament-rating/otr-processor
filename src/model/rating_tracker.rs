use crate::{api::api_structs::PlayerRating, model::structures::ruleset::Ruleset};
use indexmap::IndexMap;
use itertools::Itertools;
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet}
};

pub struct RatingTracker {
    // Global leaderboard, used as a reference for country leaderboards also.
    // When country ratings are updated, the global leaderboard is updated as well
    // to reflect the new country rank for the specific ruleset.
    // The `percentile`, `country_rank`, and `global_rank` values are updated through this IndexMap.
    leaderboard: IndexMap<(i32, Ruleset), PlayerRating>,
    // The PlayerRating here is used as a reference. The rankings are NOT updated here, but the
    // other values are affected by `insert_or_updated`.
    country_leaderboards: HashMap<String, IndexMap<(i32, Ruleset), PlayerRating>>,
    rating_history: HashMap<(i32, Ruleset), Vec<PlayerRating>>,
    country_change_tracker: HashSet<String> // This is so we don't have to update EVERY country with each update
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
            rating_history: HashMap::new(),
            country_change_tracker: HashSet::new()
        }
    }

    fn track_country(&mut self, country: &str) {
        self.country_change_tracker.insert(country.to_owned());
    }

    /// Inserts or updates a player rating in the leaderboard and rating history.
    /// The `sort` function must be called after any insertions or updates to update rankings and percentiles.
    pub fn insert_or_update(&mut self, rating: &PlayerRating, country: &str) {
        self.leaderboard
            .insert((rating.player_id, rating.ruleset), rating.clone());

        self.country_leaderboards
            .entry(country.to_owned())
            .or_default()
            .insert((rating.player_id, rating.ruleset), rating.clone());

        self.rating_history
            .entry((rating.player_id, rating.ruleset))
            .or_default()
            .push(rating.clone());

        self.track_country(country);
    }

    /// Returns the current rating value for the player and the ruleset.
    pub fn get_rating(&self, player_id: i32, ruleset: Ruleset) -> Option<&PlayerRating> {
        self.leaderboard.get(&(player_id, ruleset))
    }

    pub fn get_rating_history(&self, player_id: i32, ruleset: Ruleset) -> Option<&Vec<PlayerRating>> {
        self.rating_history.get(&(player_id, ruleset))
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
                .filter(|(_, player)| player.ruleset == *ruleset)
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
                    .filter(|(_, player)| player.ruleset == *ruleset)
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

    /// `P = n/N * 100`
    fn percentile(rank: i32, total: i32) -> Option<f64> {
        match rank.cmp(&1) {
            Ordering::Less => None,
            _ => {
                match total.cmp(&1) {
                    Ordering::Greater => {
                        let n = total - rank; // The number of players below the player
                        Some(n as f64 / total as f64 * 100.0)
                    }
                    _ => None
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        api::api_structs::PlayerRating,
        model::{
            rating_tracker::RatingTracker,
            structures::{rating_adjustment_type::RatingSource, ruleset::Ruleset}
        }
    };
    use approx::assert_abs_diff_eq;

    #[test]
    fn test_track_player() {
        let mut rating_tracker = RatingTracker::new();
        let player = PlayerRating {
            player_id: 1,
            ruleset: Ruleset::Osu,
            rating: 1000.0,
            volatility: 100.0,
            percentile: 0.5,
            global_rank: 0,
            country_rank: 0,
            timestamp: Default::default(),
            source: RatingSource::Match,
            adjustments: vec![]
        };

        rating_tracker.insert_or_update(&player, &"US".to_string());

        let player = rating_tracker.get_rating(1, Ruleset::Osu).unwrap();
        assert_eq!(player.player_id, 1);

        let player_no_ruleset = rating_tracker.get_rating(1, Ruleset::Taiko);
        assert_eq!(player_no_ruleset, None);
    }

    #[test]
    fn test_update() {
        let mut rating_tracker = RatingTracker::new();
        let country = "US".to_string();

        rating_tracker.insert_or_update(
            &PlayerRating {
                player_id: 1,
                ruleset: Ruleset::Osu,
                rating: 100.0,
                volatility: 0.0,
                percentile: 0.0,
                global_rank: 0,
                country_rank: 0,
                timestamp: Default::default(),
                source: RatingSource::Match,
                adjustments: vec![]
            },
            &country
        );

        rating_tracker.insert_or_update(
            &PlayerRating {
                player_id: 2,
                ruleset: Ruleset::Osu,
                rating: 200.0,
                volatility: 0.0,
                percentile: 0.0,
                global_rank: 0,
                country_rank: 0,
                timestamp: Default::default(),
                source: RatingSource::Match,
                adjustments: vec![]
            },
            &country
        );

        rating_tracker.sort();

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
    fn test_percentile() {
        assert_eq!(RatingTracker::percentile(0, 10), None);

        assert_eq!(RatingTracker::percentile(1, 1), None);
        assert_eq!(RatingTracker::percentile(-1, 10), None);

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
        let country = "US".to_string();

        rating_tracker.insert_or_update(
            &PlayerRating {
                player_id: 1,
                ruleset: Ruleset::Osu,
                rating: 100.0,
                volatility: 0.0,
                percentile: 0.0,
                global_rank: 0,
                country_rank: 0,
                timestamp: Default::default(),
                source: RatingSource::Decay,
                adjustments: vec![]
            },
            &country
        );

        rating_tracker.insert_or_update(
            &PlayerRating {
                player_id: 2,
                ruleset: Ruleset::Osu,
                rating: 200.0,
                volatility: 0.0,
                percentile: 0.0,
                global_rank: 0,
                country_rank: 0,
                timestamp: Default::default(),
                source: RatingSource::Decay,
                adjustments: vec![]
            },
            &country
        );

        assert_eq!(rating_tracker.country_change_tracker.len(), 1);

        rating_tracker.sort();

        assert_eq!(rating_tracker.country_change_tracker.len(), 0);
    }
}
