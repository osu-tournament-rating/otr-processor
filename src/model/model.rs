use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use openskill::model::plackett_luce::PlackettLuce;
use openskill::rating::default_gamma;
use crate::api::api_structs::{Match, Player};
use crate::model::constants::default_constants;
use crate::model::structures::Mode::Mode;
use crate::model::structures::PlayerRating::PlayerRating;
use crate::utils::progress_utils::progress_bar;

pub fn create_model() -> PlackettLuce {
    let constants = default_constants();
    PlackettLuce::new(constants.default_beta as f64,
                      constants.default_kappa as f64,
                      default_gamma)
}

pub fn mu_for_rank(rank: i32) -> f64 {
    let constants = default_constants();
    let multiplier = constants.multiplier as f64;
    let val = multiplier * (45.0 - (3.2 * (rank as f64).ln()));

    if val < multiplier * 5.0 {
        return multiplier * 5.0;
    }

    if val > multiplier * 30.0 {
        return multiplier * 30.0;
    }

    return val;
}

pub fn create_initial_ratings(matches: Vec<Match>, model: PlackettLuce, players: Vec<Player>) -> Vec<PlayerRating> {
    // The first step in the rating algorithm. Generate ratings from known ranks.

    let mut created_ratings: HashSet<(i32, Mode)> = HashSet::new();
    let mut ratings: Vec<PlayerRating> = Vec::new();
    let bar = progress_bar(players.len() as u64);

    // Map the osu ids for fast lookup
    let mut player_hashmap: HashMap<i64, Player> = HashMap::new();

    for player in players {
        if !player_hashmap.contains_key(&player.osu_id) {
            player_hashmap.insert(player.osu_id, player);
        }
    }

    for m in matches {
        for game in m.games {
            let mode = game.play_mode;
            let enum_mode = match mode.try_into() {
                Ok(mode @ (Mode::Taiko | Mode::Catch | Mode::Mania)) => mode,
                _ => panic!("Expected one of [0, 1, 2, 3] to convert to mode enum. Found {} instead.", mode),
            };

            for score in game.match_scores {
                // Check if the player_id and enum_mode combination is already in created_ratings
                if created_ratings.contains(&(score.player_id, enum_mode)) {
                    // We've already initialized this player.
                    continue;
                }

                created_ratings.insert((score.player_id, enum_mode));

                // TODO: Complete
            }
        }
    }

    ratings
}

#[cfg(test)]
mod tests {
    use crate::model::model::mu_for_rank;

    #[test]
    fn mu_for_rank_returns_correct_min() {
        let rank = 1_000_000; // Some 7 digit player
        let expected = 225.0; // The minimum

        let value = mu_for_rank(rank);

        assert_eq!(expected, value);
    }

    #[test]
    fn mu_for_rank_returns_correct_max() {
        let rank = 1; // Some 7 digit player
        let expected = 1350.0; // The minimum

        let value = mu_for_rank(rank);

        assert_eq!(expected, value);
    }

    #[test]
    fn mu_for_rank_returns_correct_10k() {
        let rank = 10000; // Some 7 digit player
        let expected = 698.7109864354294; // The minimum

        let value = mu_for_rank(rank);

        assert!((expected - value).abs() < 0.000001);
    }

    #[test]
    fn mu_for_rank_returns_correct_500() {
        let rank = 500; // Some 7 digit player
        let expected = 1130.0964338272045; // The minimum

        let value = mu_for_rank(rank);

        assert!((expected - value).abs() < 0.000001);
    }
}