use crate::api::api_structs::{Game, Match};
/// Returns a collection of valid games for the match.
/// Checks the game mode to ensure erroneous games
/// are not being counted.
///
/// Assumes the match has at least one game.
pub fn valid_games(m: &Match) -> Vec<&Game> {
    let mut valid = Vec::new();
    let games = &m.games;

    for g in games {
        if g.play_mode == m.mode {
            valid.push(g);
        }
    }

    valid
}
