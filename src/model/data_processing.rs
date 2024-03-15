use crate::{api::api_structs::Match, utils::progress_utils::progress_bar_spinner};

pub struct ModMultipliers {
    pub ez: f32
}

fn get_mod_multipliers() -> ModMultipliers {
    ModMultipliers { ez: 1.75 }
}

pub fn apply_mod_multipliers(matches: &mut Vec<Match>) {
    let multipliers = get_mod_multipliers();

    let bar = progress_bar_spinner(matches.len() as u64);
    bar.println("Applying mod multipliers...");

    for m in matches.iter_mut() {
        for g in m.games.iter_mut() {
            for s in g.match_scores.iter_mut() {
                if let Some(enabled_mods) = s.enabled_mods {
                    if enabled_mods == 2 {
                        let mult_score = s.score as f64 * (multipliers.ez as f64);
                        s.score = mult_score as i64;
                    }
                }
            }
        }

        bar.inc(1);
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        api::api_structs::{Game, Match, MatchScore},
        model::{
            data_processing::{apply_mod_multipliers, get_mod_multipliers},
            structures::{mode::Mode, scoring_type::ScoringType, team_type::TeamType}
        }
    };

    #[test]
    fn multipliers_ez() {
        let multipliers = get_mod_multipliers();

        assert_eq!(multipliers.ez, 1.75)
    }

    #[test]
    fn mod_multipliers_applies_ez() {
        let earned_score = 500000;
        let expected_score = ((earned_score as f64) * 1.75) as i64;

        let score = MatchScore {
            player_id: 0,
            team: 0,
            score: earned_score,
            enabled_mods: Some(2),
            misses: 0,
            accuracy_standard: 0.0,
            accuracy_taiko: 0.0,
            accuracy_catch: 0.0,
            accuracy_mania: 0.0
        };

        let game = Game {
            id: 0,
            play_mode: Mode::Osu,
            scoring_type: ScoringType::ScoreV2,
            team_type: TeamType::TeamVs,
            mods: 0,
            game_id: 0,
            start_time: Default::default(),
            end_time: None,
            beatmap: None,
            match_scores: vec![score]
        };

        let m = Match {
            id: 123,
            match_id: 12345,
            name: Some("STT3: (Coffee) vs (The voices are back)".to_string()),
            mode: Mode::Osu,
            start_time: None,
            end_time: None,
            games: vec![game]
        };

        let mut matches = vec![m];

        apply_mod_multipliers(&mut matches);

        let new_score = matches
            .first()
            .unwrap()
            .games
            .first()
            .unwrap()
            .match_scores
            .first()
            .unwrap()
            .score;

        assert_eq!(new_score, expected_score);
    }
}
