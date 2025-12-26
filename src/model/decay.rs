/// Unified decay implementation for the o!TR system.
///
/// The decay system handles both rating decay (for inactive players) and volatility decay
/// (for all players) in a single pass at Wednesday 12:00 UTC timestamps.
///
/// # Key Concepts
/// - Decay Floor: A minimum rating threshold based on a player's peak rating
/// - Wednesday Decay: All decay adjustments occur at Wednesday 12:00 UTC
/// - Unified Adjustments: Rating and volatility changes combined into single adjustments
use super::{
    constants::{DECAY_DAYS, DECAY_MINIMUM, DECAY_RATE, DECAY_VOLATILITY_GROWTH_RATE, VOLATILITY_DECAY_CAP},
    structures::rating_adjustment_type::RatingAdjustmentType
};
use crate::{
    database::db_structs::{PlayerRating, RatingAdjustment},
    model::structures::rating_adjustment_type::RatingAdjustmentType::{Decay, VolatilityDecay}
};
use chrono::{DateTime, Duration, FixedOffset};
use tracing::{debug, trace};

/// Unified decay system that combines rating decay (for inactive players)
/// and volatility decay (for all players) into single Wednesday 12:00 UTC adjustments.
///
/// # Design
/// - Rating decay: Applied to players inactive for more than `DECAY_DAYS` (184 days)
/// - Volatility decay: Applied to all players not at `VOLATILITY_DECAY_CAP`
/// - Both changes are combined into a single adjustment per Wednesday per player
pub struct UnifiedDecaySystem {
    last_processed_wednesday: Option<DateTime<FixedOffset>>
}

impl UnifiedDecaySystem {
    pub fn new() -> Self {
        Self {
            last_processed_wednesday: None
        }
    }

    /// Check if there are pending Wednesdays to process up to `up_to` time.
    /// On first call, establishes baseline at `up_to` without processing.
    pub fn has_pending_wednesdays(&mut self, up_to: DateTime<FixedOffset>) -> bool {
        match self.last_processed_wednesday {
            Some(from) => !Self::get_wednesdays_between(from, up_to).is_empty(),
            None => {
                self.last_processed_wednesday = Some(up_to);
                false
            }
        }
    }

    /// Returns Wednesday 12:00 UTC timestamps between `from` (exclusive) and `to` (inclusive).
    pub fn get_wednesdays_between(
        from: DateTime<FixedOffset>,
        to: DateTime<FixedOffset>
    ) -> Vec<DateTime<FixedOffset>> {
        use chrono::{Datelike, TimeZone, Timelike, Utc, Weekday};

        let mut timestamps = Vec::new();

        // Start from the day after `from` to make it exclusive
        let start = from + Duration::days(1);
        let start_utc = start.with_timezone(&Utc);

        // Find the first Wednesday at or after start
        let days_until_wednesday =
            (Weekday::Wed.num_days_from_monday() as i64 - start_utc.weekday().num_days_from_monday() as i64 + 7) % 7;
        let first_wednesday = if days_until_wednesday == 0 && start_utc.hour() >= 12 {
            // If we're already past Wednesday 12 UTC, go to next week
            start_utc.date_naive() + chrono::Days::new(7)
        } else if days_until_wednesday == 0 {
            start_utc.date_naive()
        } else {
            start_utc.date_naive() + chrono::Days::new(days_until_wednesday as u64)
        };

        // Create Wednesday 12:00 UTC timestamp
        let mut current = Utc
            .with_ymd_and_hms(
                first_wednesday.year(),
                first_wednesday.month(),
                first_wednesday.day(),
                12,
                0,
                0
            )
            .unwrap()
            .fixed_offset();

        // If the first Wednesday 12 UTC is before or equal to from, move to next week
        if current <= from {
            current += Duration::weeks(1);
        }

        while current <= to {
            timestamps.push(current);
            current += Duration::weeks(1);
        }

        timestamps
    }

    /// Applies unified decay to all players for Wednesdays between
    /// last_processed and `up_to`.
    ///
    /// For each Wednesday:
    /// 1. Determine if each player is active or inactive AS OF that Wednesday
    /// 2. For inactive players: apply both rating AND volatility decay
    /// 3. For active players: apply volatility decay only
    ///
    /// Returns the number of adjustments created.
    pub fn apply_decay(&mut self, player_ratings: &mut [PlayerRating], up_to: DateTime<FixedOffset>) -> usize {
        // On first call, establish baseline
        let from = match self.last_processed_wednesday {
            Some(t) => t,
            None => {
                self.last_processed_wednesday = Some(up_to);
                return 0;
            }
        };

        let wednesdays = Self::get_wednesdays_between(from, up_to);
        if wednesdays.is_empty() {
            return 0;
        }

        let mut total_adjustments = 0;

        for wednesday in &wednesdays {
            for player_rating in player_ratings.iter_mut() {
                if let Some(adjustment) = self.create_decay_adjustment(player_rating, *wednesday) {
                    // Update player state
                    player_rating.rating = adjustment.rating_after;
                    player_rating.volatility = adjustment.volatility_after;
                    player_rating.adjustments.push(adjustment);
                    total_adjustments += 1;
                }
            }
        }

        if let Some(last) = wednesdays.last() {
            self.last_processed_wednesday = Some(*last);
        }

        trace!(
            wednesdays = wednesdays.len(),
            adjustments = total_adjustments,
            "Applied unified decay"
        );

        total_adjustments
    }

    /// Creates a decay adjustment for a player at the given Wednesday, if needed.
    ///
    /// Returns None if no changes are needed (player at both caps).
    fn create_decay_adjustment(
        &self,
        player_rating: &PlayerRating,
        wednesday: DateTime<FixedOffset>
    ) -> Option<RatingAdjustment> {
        // Get the player's state as of this Wednesday
        let (current_rating, current_volatility) = Self::get_state_as_of(player_rating, wednesday);

        // Check conditions
        let is_inactive = Self::is_inactive_as_of(player_rating, wednesday);
        let at_volatility_cap = current_volatility >= VOLATILITY_DECAY_CAP;
        let decay_floor = Self::calculate_decay_floor(player_rating);
        let at_rating_floor = current_rating <= decay_floor;

        // Skip if nothing to do
        if at_volatility_cap && (!is_inactive || at_rating_floor) {
            return None;
        }

        // Calculate new values
        let new_volatility = if at_volatility_cap {
            current_volatility
        } else {
            Self::calculate_new_volatility(current_volatility)
        };

        let new_rating = if is_inactive && !at_rating_floor {
            Self::calculate_decay_rating(current_rating, decay_floor)
        } else {
            current_rating
        };

        // Skip if no actual changes
        if (new_rating - current_rating).abs() < f64::EPSILON
            && (new_volatility - current_volatility).abs() < f64::EPSILON
        {
            return None;
        }

        // Use Decay for inactive players, VolatilityDecay for active players
        // This is based on player activity status, not on whether rating actually changed
        // (an inactive player at the floor still gets Decay type)
        let adjustment_type = if is_inactive {
            debug!(
                player_id = player_rating.player_id,
                rating_before = current_rating,
                rating_after = new_rating,
                volatility_before = current_volatility,
                volatility_after = new_volatility,
                "Applying decay (inactive player)"
            );
            Decay
        } else {
            VolatilityDecay
        };

        Some(RatingAdjustment {
            player_id: player_rating.player_id,
            ruleset: player_rating.ruleset,
            match_id: None,
            rating_before: current_rating,
            rating_after: new_rating,
            volatility_before: current_volatility,
            volatility_after: new_volatility,
            timestamp: wednesday,
            adjustment_type
        })
    }

    /// Gets the player's rating and volatility as of a specific timestamp.
    /// Returns the state after the last adjustment at or before the timestamp.
    fn get_state_as_of(player_rating: &PlayerRating, timestamp: DateTime<FixedOffset>) -> (f64, f64) {
        let last_adj = player_rating
            .adjustments
            .iter()
            .filter(|adj| adj.timestamp <= timestamp)
            .next_back();

        match last_adj {
            Some(adj) => (adj.rating_after, adj.volatility_after),
            None => (player_rating.rating, player_rating.volatility)
        }
    }

    /// Determines if a player is inactive as of the given Wednesday.
    /// A player is inactive if their last Match adjustment was more than DECAY_DAYS ago.
    fn is_inactive_as_of(player_rating: &PlayerRating, wednesday: DateTime<FixedOffset>) -> bool {
        let last_match = player_rating
            .adjustments
            .iter()
            .filter(|adj| adj.adjustment_type == RatingAdjustmentType::Match)
            .filter(|adj| adj.timestamp <= wednesday)
            .next_back();

        match last_match {
            Some(adj) => {
                let days_inactive = (wednesday - adj.timestamp).num_days();
                days_inactive >= DECAY_DAYS as i64
            }
            None => {
                // No matches yet - player only has Initial rating, no decay
                false
            }
        }
    }

    /// Calculates the minimum rating (floor) for a player based on their peak rating.
    pub fn calculate_decay_floor(player_rating: &PlayerRating) -> f64 {
        let peak_rating = player_rating
            .adjustments
            .iter()
            .map(|adj| adj.rating_after)
            .fold(f64::NEG_INFINITY, f64::max);

        DECAY_MINIMUM.max(0.5 * (DECAY_MINIMUM + peak_rating))
    }

    /// Calculates new volatility after a decay cycle.
    fn calculate_new_volatility(current_volatility: f64) -> f64 {
        let new_volatility = (current_volatility.powf(2.0) + DECAY_VOLATILITY_GROWTH_RATE).sqrt();
        new_volatility.min(VOLATILITY_DECAY_CAP)
    }

    /// Calculates new rating after decay, ensuring it doesn't fall below the decay floor.
    fn calculate_decay_rating(current_rating: f64, decay_floor: f64) -> f64 {
        (current_rating - DECAY_RATE).max(decay_floor)
    }
}

impl Default for UnifiedDecaySystem {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{model::structures::ruleset::Ruleset, utils::test_utils::generate_player_rating};
    use approx::assert_abs_diff_eq;
    use chrono::{Datelike, TimeZone, Timelike, Utc};

    #[test]
    fn test_get_wednesdays_between_no_wednesday() {
        // Monday to Tuesday - no Wednesday in between
        let from = Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap().fixed_offset(); // Monday
        let to = Utc.with_ymd_and_hms(2024, 1, 2, 12, 0, 0).unwrap().fixed_offset(); // Tuesday

        let timestamps = UnifiedDecaySystem::get_wednesdays_between(from, to);
        assert!(timestamps.is_empty());
    }

    #[test]
    fn test_get_wednesdays_between_one_wednesday() {
        // Monday to Thursday - one Wednesday in between
        let from = Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap().fixed_offset(); // Monday
        let to = Utc.with_ymd_and_hms(2024, 1, 4, 12, 0, 0).unwrap().fixed_offset(); // Thursday

        let timestamps = UnifiedDecaySystem::get_wednesdays_between(from, to);
        assert_eq!(timestamps.len(), 1);
        assert_eq!(timestamps[0].weekday(), chrono::Weekday::Wed);
        assert_eq!(timestamps[0].hour(), 12);
    }

    #[test]
    fn test_get_wednesdays_between_multiple_wednesdays() {
        // Span 3 weeks - should have 3 Wednesdays
        let from = Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap().fixed_offset(); // Monday
        let to = Utc.with_ymd_and_hms(2024, 1, 22, 12, 0, 0).unwrap().fixed_offset(); // 3 weeks later

        let timestamps = UnifiedDecaySystem::get_wednesdays_between(from, to);
        assert_eq!(timestamps.len(), 3);

        for ts in &timestamps {
            assert_eq!(ts.weekday(), chrono::Weekday::Wed);
            assert_eq!(ts.hour(), 12);
        }
    }

    #[test]
    fn test_first_call_establishes_baseline() {
        let mut system = UnifiedDecaySystem::new();
        let mut ratings = vec![generate_player_rating(1, Ruleset::Osu, 1000.0, 200.0, 2, None, None)];

        let up_to = Utc.with_ymd_and_hms(2024, 1, 10, 12, 0, 0).unwrap().fixed_offset();
        let count = system.apply_decay(&mut ratings, up_to);

        assert_eq!(count, 0);
        assert_eq!(system.last_processed_wednesday, Some(up_to));
        assert_eq!(ratings[0].adjustments.len(), 2); // Original adjustments unchanged
    }

    #[test]
    fn test_active_player_volatility_only() {
        let mut system = UnifiedDecaySystem::new();
        let initial_volatility = 200.0;
        let initial_rating = 1000.0;

        // Create player who played recently (within DECAY_DAYS)
        let last_played = Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap().fixed_offset();
        let mut ratings = vec![generate_player_rating(
            1,
            Ruleset::Osu,
            initial_rating,
            initial_volatility,
            2,
            Some(last_played),
            Some(last_played)
        )];

        // First call establishes baseline (Monday)
        let first_time = Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap().fixed_offset();
        system.apply_decay(&mut ratings, first_time);

        // Second call spans one Wednesday (still within DECAY_DAYS of last play)
        let second_time = Utc.with_ymd_and_hms(2024, 1, 8, 12, 0, 0).unwrap().fixed_offset();
        let count = system.apply_decay(&mut ratings, second_time);

        assert_eq!(count, 1);
        assert_eq!(ratings[0].adjustments.len(), 3); // 2 original + 1 decay

        // Check the decay adjustment
        let last_adj = ratings[0].adjustments.last().unwrap();
        assert_eq!(last_adj.adjustment_type, VolatilityDecay); // Active player gets VolatilityDecay

        // Rating should be unchanged (active player)
        assert_abs_diff_eq!(last_adj.rating_before, last_adj.rating_after);
        assert_abs_diff_eq!(last_adj.rating_after, initial_rating);

        // Volatility should have increased
        assert!(last_adj.volatility_after > last_adj.volatility_before);
    }

    #[test]
    fn test_inactive_player_both_decays() {
        let mut system = UnifiedDecaySystem::new();
        let initial_volatility = 200.0;
        let initial_rating = 1500.0;

        // Create player who played long ago (more than DECAY_DAYS)
        let last_played = Utc.with_ymd_and_hms(2023, 1, 1, 12, 0, 0).unwrap().fixed_offset();
        let mut ratings = vec![generate_player_rating(
            1,
            Ruleset::Osu,
            initial_rating,
            initial_volatility,
            2,
            Some(last_played),
            Some(last_played)
        )];

        // First call establishes baseline
        let first_time = Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap().fixed_offset();
        system.apply_decay(&mut ratings, first_time);

        // Second call spans one Wednesday (player is definitely inactive)
        let second_time = Utc.with_ymd_and_hms(2024, 1, 8, 12, 0, 0).unwrap().fixed_offset();
        let count = system.apply_decay(&mut ratings, second_time);

        assert_eq!(count, 1);

        // Check the decay adjustment
        let last_adj = ratings[0].adjustments.last().unwrap();
        assert_eq!(last_adj.adjustment_type, Decay);

        // Both rating AND volatility should have changed
        assert!(last_adj.rating_after < last_adj.rating_before, "Rating should decrease");
        assert!(
            last_adj.volatility_after > last_adj.volatility_before,
            "Volatility should increase"
        );
    }

    #[test]
    fn test_184_day_boundary_mid_week() {
        let mut system = UnifiedDecaySystem::new();
        let initial_rating = 1500.0;
        let initial_volatility = 200.0;

        // Player last played on a Thursday
        // 2024-01-04 is a Thursday
        let last_played = Utc.with_ymd_and_hms(2024, 1, 4, 12, 0, 0).unwrap().fixed_offset();
        let mut ratings = vec![generate_player_rating(
            1,
            Ruleset::Osu,
            initial_rating,
            initial_volatility,
            2,
            Some(last_played),
            Some(last_played)
        )];

        // Establish baseline at last_played
        system.apply_decay(&mut ratings, last_played);

        // 184 days later is July 6, 2024 (Saturday)
        // The Wednesday before that (July 3) is only 182 days - should NOT trigger rating decay
        // The Wednesday after that (July 10) is 188 days - should trigger rating decay
        let july_3 = Utc.with_ymd_and_hms(2024, 7, 3, 12, 0, 0).unwrap().fixed_offset();
        let july_10 = Utc.with_ymd_and_hms(2024, 7, 10, 12, 0, 0).unwrap().fixed_offset();

        // Process up to July 3 (182 days)
        system.apply_decay(&mut ratings, july_3);

        // Find all volatility decay adjustments (before 184 days, player is still active)
        let volatility_decay_adjustments: Vec<_> = ratings[0]
            .adjustments
            .iter()
            .filter(|a| a.adjustment_type == VolatilityDecay)
            .collect();

        // All should be volatility-only (rating unchanged)
        for adj in &volatility_decay_adjustments {
            assert_abs_diff_eq!(adj.rating_before, adj.rating_after, epsilon = 0.01);
        }

        // Now process to July 10 (188 days)
        system.apply_decay(&mut ratings, july_10);

        // The July 10 adjustment should have rating decay (Decay type, not VolatilityDecay)
        let last_adj = ratings[0].adjustments.last().unwrap();
        assert_eq!(last_adj.adjustment_type, Decay);
        assert!(
            last_adj.rating_after < last_adj.rating_before,
            "Rating should decrease after 184 days"
        );
    }

    #[test]
    fn test_rating_chain_consistency() {
        let mut system = UnifiedDecaySystem::new();
        let initial_rating = 1500.0;
        let initial_volatility = 200.0;

        // Create an inactive player
        let last_played = Utc.with_ymd_and_hms(2023, 1, 1, 12, 0, 0).unwrap().fixed_offset();
        let mut ratings = vec![generate_player_rating(
            1,
            Ruleset::Osu,
            initial_rating,
            initial_volatility,
            2,
            Some(last_played),
            Some(last_played)
        )];

        // Establish baseline
        let first_time = Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap().fixed_offset();
        system.apply_decay(&mut ratings, first_time);

        // Process 3 Wednesdays
        let later_time = Utc.with_ymd_and_hms(2024, 1, 22, 12, 0, 0).unwrap().fixed_offset();
        system.apply_decay(&mut ratings, later_time);

        // Check chain consistency
        let decay_adjustments: Vec<_> = ratings[0]
            .adjustments
            .iter()
            .filter(|a| a.adjustment_type == Decay)
            .collect();

        assert!(decay_adjustments.len() >= 2, "Should have multiple decay adjustments");

        for window in decay_adjustments.windows(2) {
            assert_abs_diff_eq!(window[1].rating_before, window[0].rating_after, epsilon = 0.01);
            assert_abs_diff_eq!(window[1].volatility_before, window[0].volatility_after, epsilon = 0.01);
        }
    }

    #[test]
    fn test_at_floor_still_gets_volatility() {
        let mut system = UnifiedDecaySystem::new();

        // Create an inactive player at the decay floor
        let last_played = Utc.with_ymd_and_hms(2023, 1, 1, 12, 0, 0).unwrap().fixed_offset();
        let mut ratings = vec![generate_player_rating(
            1,
            Ruleset::Osu,
            DECAY_MINIMUM, // Already at floor
            200.0,
            2,
            Some(last_played),
            Some(last_played)
        )];

        // Establish baseline
        let first_time = Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap().fixed_offset();
        system.apply_decay(&mut ratings, first_time);

        // Process one Wednesday
        let second_time = Utc.with_ymd_and_hms(2024, 1, 8, 12, 0, 0).unwrap().fixed_offset();
        let count = system.apply_decay(&mut ratings, second_time);

        assert_eq!(count, 1, "Should still create adjustment for volatility");

        let last_adj = ratings[0].adjustments.last().unwrap();
        assert_eq!(last_adj.adjustment_type, Decay); // Player IS inactive, even though at floor
        assert_abs_diff_eq!(last_adj.rating_before, last_adj.rating_after); // Rating unchanged (at floor)
        assert!(last_adj.volatility_after > last_adj.volatility_before); // Volatility increased
    }

    #[test]
    fn test_at_cap_still_gets_rating_decay() {
        let mut system = UnifiedDecaySystem::new();

        // Create an inactive player at volatility cap
        let last_played = Utc.with_ymd_and_hms(2023, 1, 1, 12, 0, 0).unwrap().fixed_offset();
        let mut ratings = vec![generate_player_rating(
            1,
            Ruleset::Osu,
            1500.0,
            VOLATILITY_DECAY_CAP, // At cap
            2,
            Some(last_played),
            Some(last_played)
        )];

        // Establish baseline
        let first_time = Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap().fixed_offset();
        system.apply_decay(&mut ratings, first_time);

        // Process one Wednesday
        let second_time = Utc.with_ymd_and_hms(2024, 1, 8, 12, 0, 0).unwrap().fixed_offset();
        let count = system.apply_decay(&mut ratings, second_time);

        assert_eq!(count, 1, "Should still create adjustment for rating decay");

        let last_adj = ratings[0].adjustments.last().unwrap();
        assert_eq!(last_adj.adjustment_type, Decay); // Rating changes, so it's Decay
        assert!(last_adj.rating_after < last_adj.rating_before); // Rating decreased
        assert_abs_diff_eq!(last_adj.volatility_before, last_adj.volatility_after);
        // Volatility unchanged
    }

    #[test]
    fn test_initial_only_no_rating_decay() {
        let mut system = UnifiedDecaySystem::new();

        // Create player with only Initial adjustment (no matches)
        let mut ratings = vec![generate_player_rating(1, Ruleset::Osu, 1000.0, 200.0, 1, None, None)];

        // Establish baseline
        let first_time = Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap().fixed_offset();
        system.apply_decay(&mut ratings, first_time);

        // Process many Wednesdays (even past 184 days)
        let later_time = Utc.with_ymd_and_hms(2024, 12, 1, 12, 0, 0).unwrap().fixed_offset();
        system.apply_decay(&mut ratings, later_time);

        // All adjustments should be VolatilityDecay (player never played, so not "inactive")
        let volatility_decay_adjustments: Vec<_> = ratings[0]
            .adjustments
            .iter()
            .filter(|a| a.adjustment_type == VolatilityDecay)
            .collect();

        assert!(
            !volatility_decay_adjustments.is_empty(),
            "Should have VolatilityDecay adjustments"
        );
        for adj in &volatility_decay_adjustments {
            assert_abs_diff_eq!(adj.rating_before, adj.rating_after, epsilon = 0.01);
        }

        // Should have no Decay adjustments
        let decay_adjustments: Vec<_> = ratings[0]
            .adjustments
            .iter()
            .filter(|a| a.adjustment_type == Decay)
            .collect();
        assert!(
            decay_adjustments.is_empty(),
            "Should have no Decay adjustments for player who never played"
        );
    }

    #[test]
    fn test_adjustment_timestamps_are_wednesdays() {
        let mut system = UnifiedDecaySystem::new();
        let mut ratings = vec![generate_player_rating(1, Ruleset::Osu, 1000.0, 200.0, 2, None, None)];

        // Establish baseline
        let first_time = Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap().fixed_offset();
        system.apply_decay(&mut ratings, first_time);

        // Process several weeks
        let later_time = Utc.with_ymd_and_hms(2024, 2, 1, 12, 0, 0).unwrap().fixed_offset();
        system.apply_decay(&mut ratings, later_time);

        // All decay adjustments (both Decay and VolatilityDecay) should be on Wednesdays at 12:00 UTC
        let decay_adjustments: Vec<_> = ratings[0]
            .adjustments
            .iter()
            .filter(|a| a.adjustment_type == Decay || a.adjustment_type == VolatilityDecay)
            .collect();

        assert!(!decay_adjustments.is_empty(), "Should have decay adjustments");
        for adj in decay_adjustments {
            assert_eq!(adj.timestamp.weekday(), chrono::Weekday::Wed);
            assert_eq!(adj.timestamp.with_timezone(&Utc).hour(), 12);
        }
    }

    #[test]
    fn test_has_pending_wednesdays_first_call() {
        let mut system = UnifiedDecaySystem::new();
        let up_to = Utc.with_ymd_and_hms(2024, 1, 10, 12, 0, 0).unwrap().fixed_offset();

        // First call should return false and set baseline
        assert!(!system.has_pending_wednesdays(up_to));
        assert_eq!(system.last_processed_wednesday, Some(up_to));
    }

    #[test]
    fn test_has_pending_wednesdays_with_wednesdays() {
        let mut system = UnifiedDecaySystem::new();

        // First call establishes baseline
        let first_time = Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap().fixed_offset();
        system.has_pending_wednesdays(first_time);

        // Second call with Wednesday in between should return true
        let second_time = Utc.with_ymd_and_hms(2024, 1, 8, 12, 0, 0).unwrap().fixed_offset();
        assert!(system.has_pending_wednesdays(second_time));
    }

    #[test]
    fn test_has_pending_wednesdays_no_wednesdays() {
        let mut system = UnifiedDecaySystem::new();

        // First call establishes baseline on Monday
        let first_time = Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap().fixed_offset();
        system.has_pending_wednesdays(first_time);

        // Second call on Tuesday - no Wednesday in between
        let second_time = Utc.with_ymd_and_hms(2024, 1, 2, 12, 0, 0).unwrap().fixed_offset();
        assert!(!system.has_pending_wednesdays(second_time));
    }
}
