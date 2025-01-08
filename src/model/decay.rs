use super::constants::{DECAY_DAYS, DECAY_MINIMUM, DECAY_RATE, DEFAULT_VOLATILITY, VOLATILITY_GROWTH_RATE};
use crate::{
    database::db_structs::{PlayerRating, RatingAdjustment},
    model::structures::rating_adjustment_type::RatingAdjustmentType::{Decay, Initial},
};
use chrono::{DateTime, Duration, FixedOffset};
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum DecayError {
    #[error("Player rating has no adjustments")]
    NoAdjustments,
    #[error("Player is still active")]
    PlayerActive,
    #[error("Previous rating is initial")]
    InitialRating,
    #[error("Rating already at or below decay floor")]
    BelowDecayFloor,
}

/// Represents the result of a decay calculation
#[derive(Debug)]
pub struct DecayResult {
    pub new_rating: f64,
    pub new_volatility: f64,
    pub timestamp: DateTime<FixedOffset>,
}

/// Core decay system implementation
pub struct DecaySystem {
    current_time: DateTime<FixedOffset>,
}

impl DecaySystem {
    pub fn new(current_time: DateTime<FixedOffset>) -> Self {
        Self { current_time }
    }

    /// Apply decay to a player's rating if necessary
    pub fn decay<'a>(&self, player_rating: &'a mut PlayerRating) -> Result<Option<&'a PlayerRating>, DecayError> {
        self.validate_decay(player_rating)?;

        let last_play_time = self.get_last_play_time(player_rating)?;
        let decay_timestamps = self.calculate_decay_timestamps(player_rating, last_play_time);

        if decay_timestamps.is_empty() {
            return Ok(None);
        }

        self.apply_decay_adjustments(player_rating, decay_timestamps);
        Ok(Some(player_rating))
    }

    /// Calculate decay floor for a player based on their peak rating
    pub fn calculate_decay_floor(&self, player_rating: &PlayerRating) -> f64 {
        let peak_rating = player_rating
            .adjustments
            .iter()
            .map(|adj| adj.rating_after)
            .fold(f64::NEG_INFINITY, f64::max);

        DECAY_MINIMUM.max(0.5 * (DECAY_MINIMUM + peak_rating))
    }

    /// Calculate new volatility after decay
    pub fn calculate_decay_volatility(&self, current_volatility: f64) -> f64 {
        let new_volatility = (current_volatility.powf(2.0) + VOLATILITY_GROWTH_RATE).sqrt();
        new_volatility.min(DEFAULT_VOLATILITY)
    }

    /// Calculate new rating after decay
    pub fn calculate_decay_rating(&self, current_rating: f64, decay_floor: f64) -> f64 {
        (current_rating - DECAY_RATE).max(decay_floor)
    }

    fn validate_decay(&self, player_rating: &PlayerRating) -> Result<(), DecayError> {
        if player_rating.adjustments.is_empty() {
            return Err(DecayError::NoAdjustments);
        }

        let last_play_time = self.get_last_play_time(player_rating)?;

        if self.is_player_active(last_play_time) {
            return Err(DecayError::PlayerActive);
        }

        if let Some(last_adjustment) = player_rating.adjustments.last() {
            if last_adjustment.adjustment_type == Initial {
                return Err(DecayError::InitialRating);
            }
        }

        let decay_floor = self.calculate_decay_floor(player_rating);
        if player_rating.rating <= decay_floor {
            return Err(DecayError::BelowDecayFloor);
        }

        Ok(())
    }

    fn get_last_play_time(&self, player_rating: &PlayerRating) -> Result<DateTime<FixedOffset>, DecayError> {
        player_rating
            .adjustments
            .last()
            .map(|adj| adj.timestamp)
            .ok_or(DecayError::NoAdjustments)
    }

    fn is_player_active(&self, last_play_time: DateTime<FixedOffset>) -> bool {
        self.current_time - last_play_time < Duration::days(DECAY_DAYS as i64)
    }

    fn calculate_decay_timestamps(
        &self,
        player_rating: &PlayerRating,
        last_play_time: DateTime<FixedOffset>,
    ) -> Vec<DateTime<FixedOffset>> {
        let decay_start = last_play_time + Duration::days(DECAY_DAYS as i64);
        let mut timestamps = Vec::new();
        let floor = self.calculate_decay_floor(player_rating);

        let mut current_rating = player_rating.rating;
        let mut current_time = decay_start;

        while current_time <= self.current_time {
            let new_rating = self.calculate_decay_rating(current_rating, floor);

            if current_rating == new_rating {
                break;
            }

            timestamps.push(current_time);
            current_rating = new_rating;
            current_time += Duration::weeks(1);
        }

        timestamps
    }

    fn apply_decay_adjustments(&self, player_rating: &mut PlayerRating, timestamps: Vec<DateTime<FixedOffset>>) {
        let mut current_rating = player_rating.rating;
        let mut current_volatility = player_rating.volatility;
        let floor = self.calculate_decay_floor(player_rating);

        let mut adjustments = Vec::with_capacity(timestamps.len());

        for timestamp in timestamps {
            let new_rating = self.calculate_decay_rating(current_rating, floor);
            let new_volatility = self.calculate_decay_volatility(current_volatility);

            adjustments.push(RatingAdjustment {
                player_id: player_rating.player_id,
                ruleset: player_rating.ruleset,
                match_id: None,
                rating_before: current_rating,
                rating_after: new_rating,
                volatility_before: current_volatility,
                volatility_after: new_volatility,
                timestamp,
                adjustment_type: Decay,
            });

            current_rating = new_rating;
            current_volatility = new_volatility;
        }

        player_rating.adjustments.extend(adjustments);
        player_rating.rating = current_rating;
        player_rating.volatility = current_volatility;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::structures::{rating_adjustment_type::RatingAdjustmentType, ruleset::Ruleset},
        utils::test_utils::generate_player_rating,
    };
    use approx::assert_abs_diff_eq;
    use chrono::{TimeZone, Utc};

    #[test]
    fn test_decay_error_no_adjustments() {
        let current_time = Utc::now().fixed_offset();
        let system = DecaySystem::new(current_time);
        let mut rating = PlayerRating {
            id: 1,
            player_id: 1,
            ruleset: Ruleset::Osu,
            rating: 2000.0,
            volatility: 200.0,
            percentile: 0.0,
            global_rank: 0,
            country_rank: 0,
            adjustments: vec![],
        };

        assert_eq!(system.decay(&mut rating), Err(DecayError::NoAdjustments));
    }

    #[test]
    fn test_decay_error_player_active() {
        let last_played = Utc::now().fixed_offset();
        let current_time = last_played + Duration::days(DECAY_DAYS as i64 - 1);
        let system = DecaySystem::new(current_time);
        let mut rating =
            generate_player_rating(1, Ruleset::Osu, 2000.0, 200.0, 2, Some(last_played), Some(last_played));

        assert_eq!(system.decay(&mut rating), Err(DecayError::PlayerActive));
    }

    #[test]
    fn test_decay_error_initial_rating() {
        let last_played = Utc::now().fixed_offset();
        let current_time = last_played + Duration::days(DECAY_DAYS as i64 + 1);
        let system = DecaySystem::new(current_time);
        let mut rating =
            generate_player_rating(1, Ruleset::Osu, 2000.0, 200.0, 1, Some(last_played), Some(last_played));

        assert_eq!(system.decay(&mut rating), Err(DecayError::InitialRating));
    }

    #[test]
    fn test_decay_error_below_floor() {
        let last_played = Utc::now().fixed_offset();
        let current_time = last_played + Duration::days(DECAY_DAYS as i64 + 1);
        let system = DecaySystem::new(current_time);
        let mut rating = generate_player_rating(
            1,
            Ruleset::Osu,
            DECAY_MINIMUM,
            200.0,
            2,
            Some(last_played),
            Some(last_played),
        );

        assert_eq!(system.decay(&mut rating), Err(DecayError::BelowDecayFloor));
    }

    #[test]
    fn test_single_decay_cycle() {
        let last_played = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap().fixed_offset();
        let current_time = last_played + Duration::days(DECAY_DAYS as i64);
        let system = DecaySystem::new(current_time);

        let initial_rating = 2000.0;
        let initial_volatility = 200.0;
        let mut rating = generate_player_rating(
            1,
            Ruleset::Osu,
            initial_rating,
            initial_volatility,
            2,
            Some(last_played),
            Some(last_played),
        );

        let result = system.decay(&mut rating).unwrap().unwrap();

        assert_eq!(result.adjustments.len(), 3); // Initial + Match + 1 decay
        let decay_adjustment = result.adjustments.last().unwrap();
        assert_eq!(decay_adjustment.adjustment_type, Decay);
        assert!(decay_adjustment.rating_after < initial_rating);
        assert!(decay_adjustment.volatility_after > initial_volatility);
    }

    #[test]
    fn test_multiple_decay_cycles() {
        let last_played = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap().fixed_offset();
        let current_time = last_played + Duration::days(DECAY_DAYS as i64 + 21);
        let system = DecaySystem::new(current_time);

        let mut rating =
            generate_player_rating(1, Ruleset::Osu, 2000.0, 200.0, 2, Some(last_played), Some(last_played));

        let result = system.decay(&mut rating).unwrap().unwrap();

        assert_eq!(result.adjustments.len(), 6); // Initial + Match + 4 decay cycles (one at DECAY_DAYS + 1 per 7 extra days)

        let decay_adjustments: Vec<_> = result
            .adjustments
            .iter()
            .filter(|adj| adj.adjustment_type == Decay)
            .collect();

        for window in decay_adjustments.windows(2) {
            let time_diff = window[1].timestamp - window[0].timestamp;
            assert_eq!(time_diff, Duration::weeks(1));
        }
    }

    #[test]
    fn test_decay_volatility_growth() {
        let system = DecaySystem::new(Utc::now().fixed_offset());

        let initial_volatility = 200.0;
        let new_volatility = system.calculate_decay_volatility(initial_volatility);

        assert!(new_volatility > initial_volatility);
        assert!(new_volatility <= DEFAULT_VOLATILITY);
    }

    #[test]
    fn test_decay_floor_calculation() {
        let system = DecaySystem::new(Utc::now().fixed_offset());
        let peak_rating = 2500.0;
        let mut rating = generate_player_rating(1, Ruleset::Osu, 2000.0, 200.0, 3, None, None);

        // Add a peak rating adjustment
        rating.adjustments.push(RatingAdjustment {
            player_id: 1,
            ruleset: Ruleset::Osu,
            match_id: None,
            rating_before: 2400.0,
            rating_after: peak_rating,
            volatility_before: 200.0,
            volatility_after: 200.0,
            timestamp: Utc::now().fixed_offset(),
            adjustment_type: RatingAdjustmentType::Match,
        });

        let floor = system.calculate_decay_floor(&rating);
        let expected_floor = DECAY_MINIMUM.max(0.5 * (DECAY_MINIMUM + peak_rating));

        assert_abs_diff_eq!(floor, expected_floor);
        assert!(floor >= DECAY_MINIMUM);
    }
}
