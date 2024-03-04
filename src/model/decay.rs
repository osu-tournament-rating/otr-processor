use crate::api::api_structs::RatingAdjustment;
use crate::model::constants;
use chrono::{DateTime, FixedOffset};
use std::collections::HashMap;
use crate::model::structures::mode::Mode;

/// Tracks decay activity for players
pub struct DecayTracker {
    last_play_time: HashMap<(i32, Mode), DateTime<FixedOffset>>,
}

impl DecayTracker {
    pub fn new() -> DecayTracker {
        DecayTracker {
            last_play_time: HashMap::new(),
        }
    }
    pub fn record_activity(&mut self, player_id: i32, mode: Mode, time: DateTime<FixedOffset>) {
        self.last_play_time.insert((player_id, mode), time);
    }

    pub fn get_activity(&mut self, player_id: i32, mode: Mode) -> Option<&DateTime<FixedOffset>> {
        self.last_play_time.get(&(player_id, mode))
    }

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
    /// - Beginning after 4 months of inactivity, apply decay once weekly.
    ///
    /// If the user does not need to decay, return None.
    pub fn decay(
        &self,
        player_id: i32,
        mode: Mode,
        mu: f64,
        sigma: f64,
        d: DateTime<FixedOffset>,
    ) -> Option<Vec<RatingAdjustment>> {
        let last_play_time = self.last_play_time.get(&(player_id, mode)).unwrap();
        let decay_weeks = Self::n_decay(d, *last_play_time);

        if decay_weeks < 1 {
            return None;
        }

        let mut adjustments = Vec::new();
        let mut new_mu = mu;
        let mut new_sigma = sigma;

        for i in 0..decay_weeks {
            // Increment time by 7 days for each decay application (this is for accurate timestamps)
            let now = last_play_time.fixed_offset() + chrono::Duration::days(i * 7);
            new_mu = decay_mu(new_mu);
            new_sigma = decay_sigma(new_sigma);

            let adjustment = RatingAdjustment {
                player_id,
                mode,
                rating_adjustment_amount: new_mu - mu,
                volatility_adjustment_amount: new_sigma - sigma,
                rating_before: mu,
                rating_after: new_mu,
                volatility_before: sigma,
                volatility_after: new_sigma,
                rating_adjustment_type: 0,
                timestamp: now,
            };

            adjustments.push(adjustment);
        }

        Some(adjustments)
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

pub fn decay_sigma(sigma: f64) -> f64 {
    let new_sigma = (sigma.powf(2.0) + *constants::VOLATILITY_GROWTH_RATE).sqrt();

    new_sigma.min(constants::SIGMA)
}

pub fn decay_mu(mu: f64) -> f64 {
    let new_mu = mu - constants::DECAY_RATE;

    new_mu.max(constants::DECAY_MINIMUM)
}

#[cfg(test)]
mod tests {
    use crate::model::{
        constants,
        decay::{decay_mu, decay_sigma, is_decay_possible, DecayTracker},
    };
    use std::ops::Add;
    use chrono::DateTime;
    use crate::model::constants::MULTIPLIER;
    use crate::model::structures::mode::Mode;

    #[test]
    fn test_decay() {
        let id = 1;
        let mode = Mode::Osu;
        let mu = 1000.0;
        let sigma = 200.0;

        let mut expected_mu = mu;
        let mut expected_sigma = sigma;

        let mut tracker = DecayTracker::new();

        // Set time for one match and record activity
        let t = DateTime::parse_from_rfc3339("2021-01-01T00:00:00+00:00")
            .unwrap()
            .fixed_offset();
        // Jump ahead 4 months. Should be (4 months in weeks) decay applications
        let d = DateTime::parse_from_rfc3339("2021-05-01T00:00:00+00:00")
            .unwrap()
            .fixed_offset();

        let n_decay = DecayTracker::n_decay(d, t);

        for _ in 0..n_decay {
            expected_mu = decay_mu(expected_mu);
            expected_sigma = decay_sigma(expected_sigma);
        }

        tracker.record_activity(id, mode, t);

        let adjustments = tracker.decay(id, mode, mu, sigma, d).unwrap();

        assert_eq!(adjustments.len() as i64, n_decay);

        // Ensure all adjustments are of type 0
        for a in adjustments.iter() {
            assert_eq!(a.rating_adjustment_type, 0);
        }

        // Ensure adjustment timestamps are correct
        for i in 0..n_decay {
            let expected_time = t.add(chrono::Duration::days(i * 7));
            assert_eq!(adjustments[i as usize].timestamp, expected_time);
        }
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
        let new_sigma = decay_sigma(sigma);
        let expected = (sigma.powf(2.0) + *constants::VOLATILITY_GROWTH_RATE).sqrt();

        assert_eq!(new_sigma, expected);
    }

    #[test]
    fn test_decay_sigma_maximum_default() {
        let sigma = 999.0;
        let new_sigma = decay_sigma(sigma);
        let expected = constants::SIGMA;

        assert_eq!(new_sigma, expected);
    }

    #[test]
    fn test_decay_mu_standard() {
        let mu = 1100.0;
        let new_mu = decay_mu(mu);
        let expected = mu - constants::DECAY_RATE;

        assert_eq!(new_mu, expected);
    }

    #[test]
    fn test_decay_mu_min_decay() {
        let mu = MULTIPLIER * 18.0;
        let new_mu = decay_mu(mu);
        let expected = MULTIPLIER * 18.0;

        assert_eq!(new_mu, expected);
    }
}
