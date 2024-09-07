use crate::model::{
    db_structs::{Game, GameScore, Match, Player, RulesetData},
    structures::ruleset::Ruleset
};
use serde_json::to_string;
use std::sync::Arc;
use tokio_postgres::{Client, Error, NoTls};

#[derive(Clone)]
pub struct DbClient {
    client: Arc<Client>
}

impl DbClient {
    // Connect to the database and return a DbClient instance
    pub async fn connect(connection_str: &str) -> Result<Self, Error> {
        let (client, connection) = tokio_postgres::connect(connection_str, NoTls).await?;

        // Spawn the connection object to run in the background
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("connection error: {}", e);
            }
        });

        Ok(DbClient {
            client: Arc::new(client)
        })
    }

    pub async fn get_matches(&self) -> Vec<Match> {
        let mut matches: Vec<Match> = Vec::new();
        let rows = self.client.query("
        SELECT
            t.id AS tournament_id, t.name AS tournament_name, t.ruleset AS tournament_ruleset,
            m.id AS match_id, m.name AS match_name, m.start_time AS match_start_time, m.end_time AS match_end_time, m.tournament_id AS match_tournament_id,
            g.id AS game_id, g.ruleset AS game_ruleset, g.start_time AS game_start_time, g.end_time AS game_end_time, g.match_id AS game_match_id,
            gs.id AS game_score_id, gs.player_id AS game_score_player_id, gs.game_id AS game_score_game_id, gs.score AS game_score_score, gs.placement AS game_score_placement
        FROM tournaments t
        LEFT JOIN matches m ON t.id = m.tournament_id
        LEFT JOIN games g ON m.id = g.match_id
        LEFT JOIN game_scores gs ON g.id = gs.game_id
        WHERE t.verification_status = 1 AND m.verification_status = 2 AND g.verification_status = 2
        AND gs.verification_status = 0
        ORDER BY m.start_time", &[]).await.unwrap();

        // TODO: Add 'WHERE t.processing_status = 4' to the query
        // TODO: Change 'WHERE t.verification_status = 1' to 'WHERE t.verification_status = 4'
        // TODO: Change 'WHERE m.verification_status = 2' to 'WHERE m.verification_status = 4'
        // TODO: Change 'WHERE gs.verification_status = 0' to 'WHERE gs.verification_status = 4'

        let mut current_match_id = -1;
        let mut current_game_id = -1;
        let mut current_game_score_id = -1;

        for row in rows {
            if row.get::<_, i32>("match_id") != current_match_id {
                let match_ = Match {
                    id: row.get("match_id"),
                    name: row.get("match_name"),
                    start_time: row.get("match_start_time"),
                    end_time: row.get("match_end_time"),
                    ruleset: Ruleset::try_from(row.get::<_, i32>("tournament_ruleset")).unwrap(),
                    games: Vec::new()
                };
                matches.push(match_);
                current_match_id = row.get("match_id");
            }

            if row.get::<_, i32>("game_id") != current_game_id {
                let game = Game {
                    id: row.get("game_id"),
                    ruleset: Ruleset::try_from(row.get::<_, i32>("game_ruleset")).unwrap(),
                    start_time: row.get("game_start_time"),
                    end_time: row.get("game_end_time"),
                    scores: Vec::new()
                };
                matches.last_mut().unwrap().games.push(game);
                current_game_id = row.get("game_id");
            }

            if row.get::<_, i32>("game_score_id") != current_game_score_id {
                let game_score = GameScore {
                    id: row.get("game_score_id"),
                    player_id: row.get("game_score_player_id"),
                    game_id: row.get("game_score_game_id"),
                    score: row.get("game_score_score"),
                    placement: row.get("game_score_placement")
                };
                matches
                    .last_mut()
                    .unwrap()
                    .games
                    .last_mut()
                    .unwrap()
                    .scores
                    .push(game_score);
                current_game_score_id = row.get("game_score_id");
            }
        }

        matches
    }

    pub async fn get_players(&self) -> Vec<Player> {
        let mut players: Vec<Player> = Vec::new();
        let rows = self
            .client
            .query(
                "SELECT p.id AS player_id, p.username AS username, \
        p.country AS country, prd.ruleset AS ruleset, prd.earliest_global_rank AS earliest_global_rank,\
          prd.global_rank AS global_rank FROM players p \
        LEFT JOIN player_osu_ruleset_data prd ON prd.player_id = p.id",
                &[]
            )
            .await
            .unwrap();

        let mut current_player_id = -1;
        for row in rows {
            if row.get::<_, i32>("player_id") != current_player_id {
                let player = Player {
                    id: row.get("player_id"),
                    username: row.get("username"),
                    country: row.get("country"),
                    ruleset_data: vec![RulesetData {
                        ruleset: Ruleset::try_from(row.get::<_, i32>("ruleset")).unwrap(),
                        global_rank: row.get("global_rank"),
                        earliest_global_rank: row.get("earliest_global_rank")
                    }]
                };
                players.push(player);
                current_player_id = row.get("player_id");
            } else {
                players.last_mut().unwrap().ruleset_data.push(RulesetData {
                    ruleset: Ruleset::try_from(row.get::<_, i32>("ruleset")).unwrap(),
                    global_rank: row.get("global_rank"),
                    earliest_global_rank: row.get("earliest_global_rank")
                });
            }
        }

        players
    }

    // Access the underlying Client
    pub fn client(&self) -> Arc<Client> {
        Arc::clone(&self.client)
    }
}
