use super::constants::DECAY_MINIMUM;
use crate::{
    database::db_structs::{PlayerRating, RatingAdjustment},
    model::{
        constants,
        constants::DECAY_DAYS,
        structures::rating_adjustment_type::RatingAdjustmentType::{Decay, Initial}
    }
};
use chrono::{DateTime, FixedOffset};

/// # How this works
/// - This gets called by the rating processor during some point in time, D
///     (here, D is `current_time`)
/// - The user's last play time is T
/// - Time D may be the first time the player has played in 1 day, or 5 years.
/// - Delta is represented as (D - T)
/// - For each week from (D - (T + 4 months)), beginning with (T + 4 months),
///     apply decay once weekly.
///
/// # Rules
/// - User must be inactive for at least 4 months before decay begins.
///
/// If decay is necessary, update the rating_tracker with the new RatingAdjustments
///
/// Rules:
/// - 1 application per 7 days, beginning from 4 months of inactivity.
///
/// params:
/// - d: The current time (in this context, it is the time the match was played)
/// - t: The last time the user played
pub fn decay(player_rating: &mut PlayerRating, current_time: DateTime<FixedOffset>) -> Option<&PlayerRating> {
    if decay_impossible(player_rating, current_time) {
        return None;
    }

    let decay_timestamps = decay_timestamps(player_rating, last_play_time(player_rating), current_time);
    let mut decay_adjustments = Vec::new();

    // Tracking vars for update loop
    let mut r = player_rating.rating;
    let mut v = player_rating.volatility;
    let floor = decay_floor(player_rating);
    for timestamp in decay_timestamps {
        // Increment time by 7 days for each decay application (this is for accurate timestamps)
        let new_rating = decay_rating(r, floor);
        let new_volatility = decay_volatility(v);

        decay_adjustments.push(RatingAdjustment {
            player_id: player_rating.player_id,
            ruleset: player_rating.ruleset,
            match_id: None,
            rating_before: r,
            rating_after: new_rating,
            volatility_before: v,
            volatility_after: new_volatility,
            timestamp,
            adjustment_type: Decay
        });

        r = new_rating;
        v = new_volatility;
    }

    player_rating.adjustments.extend(decay_adjustments);
    player_rating.rating = r;
    player_rating.volatility = v;

    Some(player_rating)
}

/// The number of weeks to apply decay rating adjustments for.
/// This function accounts for the possibility that a
/// player may prematurely hit their decay floor. When this happens,
/// further timestamps will not be included in the result.
fn decay_timestamps(
    player_rating: &PlayerRating,
    last_play_time: DateTime<FixedOffset>,
    current_time: DateTime<FixedOffset>
) -> Vec<DateTime<FixedOffset>> {
    // The time at which decay begins for this player
    // (4 months after their last play time)
    let offset_start_time = last_play_time + chrono::Duration::days(DECAY_DAYS as i64);

    // Return the sum of weeks from the offset start time up to the current time
    let weeks = (current_time - offset_start_time).num_weeks() + 1;

    let mut timestamps = vec![];

    let floor = decay_floor(player_rating);
    let mut sim_rating = decay_rating(player_rating.rating, floor);
    for i in 0..weeks {
        let simulated_time = offset_start_time + chrono::Duration::weeks(i);
        let decay_sim = decay_rating(sim_rating, floor);

        if sim_rating == decay_sim {
            break;
        }

        sim_rating = decay_sim;
        timestamps.push(simulated_time);
    }

    timestamps
}

fn last_play_time(player_rating: &PlayerRating) -> DateTime<FixedOffset> {
    player_rating.adjustments.last().unwrap().timestamp
}

/// Returns true if the player has played in the last {DECAY_DAYS} days.
fn is_active(player_rating: &PlayerRating, current_time: DateTime<FixedOffset>) -> bool {
    let last_play_time = last_play_time(player_rating);
    let delta = current_time - last_play_time;
    let days = chrono::Duration::days(DECAY_DAYS as i64);

    delta < days
}

fn previous_rating_is_initial_rating(player_rating: &PlayerRating) -> bool {
    player_rating.adjustments.last().unwrap().adjustment_type == Initial
}

fn decay_below_minimum(player_rating: &PlayerRating) -> bool {
    player_rating.rating <= decay_floor(player_rating)
}

fn decay_impossible(player_rating: &PlayerRating, current_time: DateTime<FixedOffset>) -> bool {
    player_rating.adjustments.is_empty()
        || is_active(player_rating, current_time)
        || previous_rating_is_initial_rating(player_rating)
        || decay_below_minimum(player_rating)
}

fn decay_volatility(sigma: f64) -> f64 {
    let new_sigma = (sigma.powf(2.0) + constants::VOLATILITY_GROWTH_RATE).sqrt();

    new_sigma.min(constants::DEFAULT_VOLATILITY)
}

fn decay_rating(mu: f64, decay_floor: f64) -> f64 {
    let new_mu = mu - constants::DECAY_RATE;

    new_mu.max(decay_floor)
}

/// The minimum possible decay value based on a player's peak rating
fn decay_floor(player_rating: &PlayerRating) -> f64 {
    DECAY_MINIMUM.max(0.5 * (DECAY_MINIMUM + peak_rating(player_rating).rating_after))
}

fn peak_rating(player_rating: &PlayerRating) -> &RatingAdjustment {
    player_rating
        .adjustments
        .iter()
        .max_by(|a, b| a.rating_after.partial_cmp(&b.rating_after).unwrap())
        .unwrap()
}

#[cfg(test)]
mod tests {
    use crate::{
        model::decay::decay,
        model::decay::decay_floor,
        model::decay::decay_rating,
        model::decay::decay_volatility,
        model::decay::peak_rating,
        model::constants,
        model::constants::DECAY_DAYS,
        model::constants::DECAY_MINIMUM,
        model::constants::MULTIPLIER,
        model::structures::ruleset::Ruleset::Osu,
        utils::test_utils::{generate_player_rating}
    };
    use approx::assert_abs_diff_eq;
    use crate::database::db_structs::{PlayerRating, RatingAdjustment};
    use crate::model::structures::rating_adjustment_type::RatingAdjustmentType::{Decay, Initial, Match};

    fn test_rating() -> PlayerRating {
        PlayerRating {
            id: 1,
            player_id: 1,
            ruleset: Osu,
            rating: 2350f64,
            volatility: 225f64,
            percentile: 0.0,
            global_rank: 0,
            country_rank: 0,
            adjustments: vec![
                RatingAdjustment {
                    player_id: 1,
                    ruleset: Osu,
                    match_id: None,
                    rating_before: 0.0,
                    rating_after: 2000f64,
                    volatility_before: 0.0,
                    volatility_after: 300f64,
                    timestamp: "2007-09-16T00:00:00-00:00".parse().unwrap(),
                    adjustment_type: Initial,
                },
                RatingAdjustment {
                    player_id: 1,
                    ruleset: Osu,
                    match_id: Some(1),
                    rating_before: 2000f64,
                    rating_after: 2241.1781f64,
                    volatility_before: 300f64,
                    volatility_after: 280.221f64,
                    timestamp: "2007-09-17T00:00:00-00:00".parse().unwrap(),
                    adjustment_type: Match,
                },
                RatingAdjustment {
                    player_id: 1,
                    ruleset: Osu,
                    match_id: Some(2),
                    rating_before: 2241.1781f64,
                    rating_after: 2350f64,
                    volatility_before: 280.221f64,
                    volatility_after: 225f64,
                    timestamp: "2007-09-18T00:00:00-00:00".parse().unwrap(),
                    adjustment_type: Match,
                }
            ],
        }
    }

    #[test]
    fn decay_once_field_validation() {
        // Arrange
        let player_rating = &mut test_rating();
        let last_adjustment = player_rating.adjustments.last().unwrap();
        let current_time =
            last_adjustment.timestamp + chrono::Duration::days(DECAY_DAYS as i64);

        // Decay once
        let expected_rating = decay_rating(player_rating.rating, decay_floor(player_rating));
        let expected_volatility = decay_volatility(player_rating.volatility);

        // Act
        let clone = &mut player_rating.clone();
        let actual_decay = decay(clone, current_time).unwrap();
        
        // Assert
        assert_abs_diff_eq!(expected_rating, actual_decay.rating);
        assert_abs_diff_eq!(expected_volatility, actual_decay.volatility);
        assert_eq!(actual_decay.adjustments.len(), player_rating.adjustments.len() + 1);
        assert_eq!(actual_decay.adjustments.last().unwrap().adjustment_type, Decay)
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
        let sigma = f64::MAX;
        let new_sigma = decay_volatility(sigma);
        let expected = constants::DEFAULT_VOLATILITY;

        assert_eq!(new_sigma, expected);
    }

    #[test]
    fn test_decay_mu_standard() {
        let mu = 2000.0;
        let new_mu = decay_rating(mu, DECAY_MINIMUM);
        let expected = mu - constants::DECAY_RATE;

        assert_eq!(new_mu, expected);
    }

    #[test]
    fn test_decay_mu_min_decay() {
        let mu = MULTIPLIER * 15.0;
        let new_mu = decay_rating(mu, DECAY_MINIMUM);
        let expected = MULTIPLIER * 15.0;

        assert_eq!(new_mu, expected);
    }

    #[test]
    fn test_decay_floor() {
        let rating = generate_player_rating(1, Osu, 2300f64, 225f64, 10);
        let floor = decay_floor(&rating);

        let peak = peak_rating(&rating).rating_after;

        assert_abs_diff_eq!(floor, DECAY_MINIMUM.max(0.5 * (DECAY_MINIMUM + peak)))
    }

    #[test]
    fn test_decay_floor_cannot_decay_below_const_min() {
        let rating = generate_player_rating(1, Osu, 0.0, 225f64, 1);
        let floor = decay_floor(&rating);

        assert_eq!(floor, DECAY_MINIMUM);
    }
}
