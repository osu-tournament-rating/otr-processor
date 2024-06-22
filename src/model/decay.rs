use crate::{
    api::api_structs::PlayerRating,
    model::{
        constants,
        rating_tracker::RatingTracker,
        structures::{rating_adjustment_type::RatingSource, ruleset::Ruleset}
    }
};
use chrono::{DateTime, FixedOffset};

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
        let rating = rating_tracker.get_rating(player_id, ruleset);
        match rating {
            None => return,
            Some(r) => {
                if !is_decay_possible(r.rating) {
                    return;
                }
            }
        }

        let last_play_time = rating.unwrap().timestamp;
        if d < last_play_time {
            return;
        }

        let decay_weeks = Self::n_decay(d, last_play_time);
        if decay_weeks < 1 {
            return;
        }

        let mut old_rating = rating.unwrap().rating;
        let mut old_volatility = rating.unwrap().volatility;

        let source = RatingSource::Decay;

        for i in 0..decay_weeks {
            // Increment time by 7 days for each decay application (this is for accurate timestamps)
            let now = last_play_time + chrono::Duration::days(i * 7);
            let new_rating = decay_rating(old_rating);
            let new_volatility = decay_volatility(old_volatility);

            old_rating = new_rating;
            old_volatility = new_volatility;

            let new_rating = PlayerRating {
                player_id,
                ruleset,
                rating: new_rating,
                volatility: new_volatility,
                // Values with 0 are handled by the RatingTracker
                percentile: 0.0,
                global_rank: 0,
                country_rank: 0,
                timestamp: now,
                source,
                adjustments: Vec::new()
            };

            rating_tracker.insert_or_update(&new_rating, country)
        }

        rating_tracker.sort();
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

        if (duration_days as u64) < constants::DECAY_DAYS {
            return 0;
        }

        duration.num_days() / 7
    }
}

pub fn is_decay_possible(mu: f64) -> bool {
    mu > constants::DECAY_MINIMUM
}

pub fn decay_volatility(sigma: f64) -> f64 {
    let new_sigma = (sigma.powf(2.0) + constants::VOLATILITY_GROWTH_RATE).sqrt();

    new_sigma.min(constants::SIGMA)
}

pub fn decay_rating(mu: f64) -> f64 {
    let new_mu = mu - constants::DECAY_RATE;

    new_mu.max(constants::DECAY_MINIMUM)
}

#[cfg(test)]
mod tests {
    use crate::{
        api::api_structs::PlayerRating,
        model::{
            constants,
            constants::MULTIPLIER,
            decay::{decay_rating, decay_volatility, is_decay_possible, DecayTracker},
            rating_tracker::RatingTracker,
            structures::{rating_adjustment_type::RatingSource::Decay, ruleset::Ruleset}
        }
    };
    use approx::{assert_abs_diff_eq, assert_abs_diff_ne};
    use chrono::DateTime;
    use std::ops::{Add};

    #[test]
    fn test_decay() {
        let mut rating_tracker = RatingTracker::new();
        let country = "US".to_string();
        let ruleset = Ruleset::Osu;
        let volatility = 100.0;
        let initial_rating = 1000.0;

        let t = DateTime::parse_from_rfc3339("2021-01-01T00:00:00+00:00")
            .unwrap()
            .fixed_offset();
        let d = t.add(chrono::Duration::days(constants::DECAY_DAYS as i64));

        for i in 0..10 {
            // ID 0 has the best starting rating, ID 9 has the worst. Useful for asserting ranking updates
            let rating = initial_rating - (i as f64 * 20.0);
            let player_id = i;
            // ID 0 will have decay applied
            // Other IDs will not have decay applied
            rating_tracker.insert_or_update(
                &PlayerRating {
                    player_id,
                    ruleset,
                    rating,
                    volatility,
                    percentile: 0.0,
                    global_rank: 0,
                    country_rank: 0,
                    timestamp: t,
                    source: Decay,
                    adjustments: Vec::new()
                },
                &country
            );
        }

        let decay_tracker = DecayTracker;
        decay_tracker.decay(&mut rating_tracker, 0, &country, ruleset, d);
        decay_tracker.decay(
            &mut rating_tracker,
            1,
            &country,
            ruleset,
            d.add(chrono::Duration::days(10))
        );

        let decayed_rating = rating_tracker.get_rating(0, ruleset).unwrap();
        let non_decay_rating = rating_tracker.get_rating(1, ruleset).unwrap();

        let n_decay = DecayTracker::n_decay(d, t);
        let mut dec_rating = initial_rating;
        let mut dec_volatility = volatility;

        for i in 0..n_decay {
            dec_rating = decay_rating(dec_rating);
            dec_volatility = decay_volatility(dec_volatility);
        }

        assert_abs_diff_eq!(decayed_rating.rating, dec_rating);
        assert_abs_diff_eq!(decayed_rating.volatility, dec_volatility);

        assert_abs_diff_ne!(non_decay_rating.rating, 1000.0);
        assert_abs_diff_ne!(non_decay_rating.volatility, 100.0);

        // Assert rank updates
        assert_eq!(decayed_rating.global_rank, 2);
        assert_eq!(decayed_rating.country_rank, 2);
        assert_eq!(non_decay_rating.global_rank, 4);
        assert_eq!(non_decay_rating.country_rank, 4);
    }

    #[test]
    fn test_n_decay() {
        let t = DateTime::parse_from_rfc3339("2021-01-01T00:00:00+00:00")
            .unwrap()
            .fixed_offset();
        let d = t.add(chrono::Duration::days(constants::DECAY_DAYS as i64));

        let n = DecayTracker::n_decay(d, t);

        assert_eq!(n, constants::DECAY_DAYS as i64 / 7);
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
        let expected = constants::SIGMA;

        assert_eq!(new_sigma, expected);
    }

    #[test]
    fn test_decay_mu_standard() {
        let mu = 1100.0;
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
