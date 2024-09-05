use chrono::{DateTime, FixedOffset};

use crate::{
    model::{
        constants,
        constants::DECAY_DAYS,
        db_structs::PlayerRating,
        rating_tracker::RatingTracker,
        structures::{rating_adjustment_type::RatingAdjustmentType, ruleset::Ruleset}
    },
    utils::test_utils::generate_country_mapping
};
use crate::model::db_structs::{NewPlayerRating, NewRatingAdjustment};
use crate::model::structures::rating_adjustment_type::RatingAdjustmentType::Decay;

/// Tracks decay activity for players
pub struct DecayTracker;

impl DecayTracker {
    /// Returns a [`Vec<RatingAdjustment>`] for each decay application for this user.
    ///
    /// # How this works
    /// - This gets called by the rating processor during some point in time, D
    /// - The user's last play time is T
    /// - Time D may be the first time the player has played in 1 day, or 5 years.
    /// - Delta is represented as (D - T) in days
    /// - Divide delta by 7, as we apply decay once weekly.
    /// - For each week, apply decay.
    ///
    /// # Rules
    /// - User must be inactive for at least 4 months before decay begins.
    /// - Beginning after 4 months of inactivity, apply decay once weekly up to time D.
    ///
    /// If the user does not need to decay, return None.
    pub fn decay(
        &self,
        rating_tracker: &mut RatingTracker,
        player_id: i32,
        country: &str,
        ruleset: Ruleset,
        d: DateTime<FixedOffset>
    ) {
        let player_rating = rating_tracker.get_rating(player_id, ruleset);

        if player_rating.is_none() {
            return;
        }

        let mut clone_rating = player_rating.unwrap().clone();

        if !is_decay_possible(clone_rating.rating) {
            return;
        }

        // Extract the last adjustment to avoid multiple calls to unwrap and allow mutable borrow later
        let last_adjustment = match clone_rating.adjustments.last() {
            Some(adjustment) => adjustment,
            None => return, // Early return if no last adjustment
        };

        if d < last_adjustment.timestamp {
            return;
        }

        let decay_weeks = Self::n_decay(d, last_adjustment.timestamp);
        if decay_weeks < 1 {
            return;
        }

        let mut old_rating = clone_rating.rating;
        let mut old_volatility = clone_rating.volatility;

        let mut decay_ratings = Vec::new();
        for i in 0..decay_weeks {
            // Increment time by 7 days for each decay application (this is for accurate timestamps)
            let simulated_time = last_adjustment.timestamp + chrono::Duration::days(i * 7);
            let new_rating = decay_rating(old_rating);
            let new_volatility = decay_volatility(old_volatility);

            old_rating = new_rating;
            old_volatility = new_volatility;

            decay_ratings.push(NewRatingAdjustment {
                player_id,
                player_rating_id: 0,  // Presumably updated later
                match_id: None,
                rating_before: old_rating,
                rating_after: new_rating,
                volatility_before: old_volatility,
                volatility_after: new_volatility,
                timestamp: simulated_time,
                adjustment_type: Decay,
            });
        }

        // Now that all immutable borrows are done, mutably borrow adjustments and extend
        let mut new_adjustments = clone_rating.adjustments.clone();
        new_adjustments.extend(decay_ratings);
        clone_rating.adjustments = new_adjustments;

        let country_mapping = generate_country_mapping(&vec![clone_rating.clone()], country);
        rating_tracker.insert_or_update(&vec![clone_rating], &country_mapping);
    }

    /// Returns the number of decay applications that should be applied.
    ///
    /// Rules:
    /// - 1 application per 7 days, beginning from 4 months of inactivity.
    ///
    /// params:
    /// - d: The current time (in this context, it is the time the match was played)
    /// - t: The last time the user played
    fn n_decay(d: DateTime<FixedOffset>, t: DateTime<FixedOffset>) -> i64 {
        let duration = d.signed_duration_since(t);
        let duration_days = duration.num_days();

        if (duration_days as u64) < DECAY_DAYS {
            return 0;
        }

        (((duration.num_days() as u64 - DECAY_DAYS) / 7u64) + 1u64) as i64
    }
}

pub fn is_decay_possible(mu: f64) -> bool {
    mu > constants::DECAY_MINIMUM
}

fn decay_volatility(sigma: f64) -> f64 {
    let new_sigma = (sigma.powf(2.0) + constants::VOLATILITY_GROWTH_RATE).sqrt();

    new_sigma.min(constants::DEFAULT_VOLATILITY)
}

fn decay_rating(mu: f64) -> f64 {
    let new_mu = mu - constants::DECAY_RATE;

    new_mu.max(constants::DECAY_MINIMUM)
}

#[cfg(test)]
mod tests {
    use std::ops::Add;

    use approx::assert_abs_diff_eq;
    use chrono::DateTime;

    use crate::{
        model::{
            constants,
            constants::{DECAY_DAYS, MULTIPLIER},
            decay::{decay_rating, decay_volatility, is_decay_possible, DecayTracker},
            rating_tracker::RatingTracker,
            structures::{rating_adjustment_type::RatingAdjustmentType, ruleset::Ruleset}
        },
        utils::test_utils::{generate_country_mapping, generate_player_rating}
    };

    #[test]
    fn test_decay_default_days() {
        decay(DECAY_DAYS as i32)
    }

    #[test]
    fn test_decay_many_days() {
        decay(7000)
    }

    fn decay(decay_days: i32) {
        let mut rating_tracker = RatingTracker::new();
        let ruleset = Ruleset::Osu;

        let initial_rating = 2000.0;
        let initial_volatility = 100.0;

        // t = "last played time"
        let t = DateTime::parse_from_rfc3339("2021-01-01T00:00:00+00:00")
            .unwrap()
            .fixed_offset();
        let d = t.add(chrono::Duration::days(decay_days as i64));

        let player_ratings = vec![generate_player_rating(1, ruleset, 
                                                         initial_rating, initial_volatility, 1)];

        let country = "US";
        let country_mapping = generate_country_mapping(&player_ratings, country);
        rating_tracker.insert_or_update(&player_ratings, &country_mapping);

        let decay_tracker = DecayTracker;
        decay_tracker.decay(&mut rating_tracker, 1, country, ruleset, d);

        let decayed_rating = rating_tracker.get_rating(1, ruleset).unwrap();

        let n_decay = DecayTracker::n_decay(d, t);
        let mut expected_decay_rating = initial_rating;
        let mut expected_decay_volatility = initial_volatility;

        for i in 0..n_decay {
            expected_decay_rating = decay_rating(expected_decay_rating);
            expected_decay_volatility = decay_volatility(expected_decay_volatility);
        }

        assert_abs_diff_eq!(decayed_rating.rating, expected_decay_rating);
        assert_abs_diff_eq!(decayed_rating.volatility, expected_decay_volatility);

        // Assert rank updates
        assert_eq!(decayed_rating.global_rank, 1);
        assert_eq!(decayed_rating.country_rank, 1);
    }

    #[test]
    fn test_n_decay_begin() {
        let t = DateTime::parse_from_rfc3339("2021-01-01T00:00:00+00:00")
            .unwrap()
            .fixed_offset();
        let d = t.add(chrono::Duration::days(constants::DECAY_DAYS as i64));

        let n = DecayTracker::n_decay(d, t);

        assert_eq!(n, 1);
    }

    #[test]
    fn test_n_decay_one_month() {
        let t = DateTime::parse_from_rfc3339("2020-12-01T00:00:00+00:00")
            .unwrap()
            .fixed_offset();
        let d = t.add(chrono::Duration::days(DECAY_DAYS as i64 + 30));

        let n = DecayTracker::n_decay(d, t);

        assert_eq!(n, 5);
    }

    #[test]
    fn test_n_decay_less_than_decay_days() {
        let days = (constants::DECAY_DAYS - 1) as i64;

        let t = DateTime::parse_from_rfc3339("2021-01-01T00:00:00+00:00")
            .unwrap()
            .fixed_offset();
        let d = t.add(chrono::Duration::days(days));

        let n = DecayTracker::n_decay(d, t);

        assert_eq!(n, 0);
    }

    #[test]
    fn test_decay_possible() {
        let mu = 500.0;
        let decay_min = constants::DECAY_MINIMUM;

        let decay_possible = mu > (decay_min);
        let result = is_decay_possible(mu);

        assert_eq!(result, decay_possible)
    }

    #[test]
    fn test_decay_sigma_standard() {
        let sigma = 200.1;
        let new_sigma = decay_volatility(sigma);
        let expected = (sigma.powf(2.0) + constants::VOLATILITY_GROWTH_RATE).sqrt();

        assert_eq!(new_sigma, expected);
    }

    #[test]
    fn test_decay_sigma_maximum_default() {
        let sigma = 999.0;
        let new_sigma = decay_volatility(sigma);
        let expected = constants::DEFAULT_VOLATILITY;

        assert_eq!(new_sigma, expected);
    }

    #[test]
    fn test_decay_mu_standard() {
        let mu = 2000.0;
        let new_mu = decay_rating(mu);
        let expected = mu - constants::DECAY_RATE;

        assert_eq!(new_mu, expected);
    }

    #[test]
    fn test_decay_mu_min_decay() {
        let mu = MULTIPLIER * 18.0;
        let new_mu = decay_rating(mu);
        let expected = MULTIPLIER * 18.0;

        assert_eq!(new_mu, expected);
    }
}
