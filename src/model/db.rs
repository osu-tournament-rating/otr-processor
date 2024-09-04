use crate::model::db_structs::{NewMatch, NewTournament};
use crate::model::structures::ruleset::Ruleset;
use std::sync::Arc;
use tokio_postgres::{Client, Error, NoTls};

#[derive(Clone)]
pub struct DbClient {
    client: Arc<Client>,
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
            client: Arc::new(client),
        })
    }

    pub async fn get_tournaments(&self) -> Vec<NewTournament> {
        let mut tournaments = Vec::new();
        let rows = self.client.query("
        SELECT
            t.id AS tournament_id, t.name AS tournament_name, t.ruleset AS tournament_ruleset,
            m.id AS match_id, m.name AS match_name, m.start_time AS match_start_time, m.end_time AS match_end_time, m.tournament_id AS match_tournament_id,
            g.id AS game_id, g.ruleset AS game_ruleset, g.start_time AS game_start_time, g.end_time AS game_end_time, g.match_id AS game_match_id,
            gs.id AS game_score_id, gs.player_id AS game_score_player_id, gs.game_id AS game_score_game_id, gs.score AS game_score_score
        FROM tournaments t
        LEFT JOIN matches m ON t.id = m.tournament_id
        LEFT JOIN games g ON m.id = g.match_id
        LEFT JOIN game_scores gs ON g.id = gs.game_id
        WHERE t.verification_status = 2 AND m.verification_status = 2", &[]).await.unwrap();

        // TODO: Add 'WHERE t.processing_status = 4' to the query
        // TODO: Change 'WHERE t.verification_status = 2' to 'WHERE t.verification_status = 4'
        // TODO: Change 'WHERE m.verification_status = 2' to 'WHERE m.verification_status = 4'
        for row in rows {
            let tournament_id: i32 = row.get("tournament_id");
            let match_id: i32 = row.get("match_id");
            let game_id: i32 = row.get("game_id");
            let game_score_id: i32 = row.get("game_score_id");

            let mut tournament = NewTournament {
                id: tournament_id,
                name: row.get("tournament_name"),
                ruleset: Ruleset::try_from(row.get::<&str, i32>("tournament_ruleset")).unwrap(),
                matches: vec![],
            };

            // Check if the match already exists
            let match_pos = tournament.matches.iter_mut().position(|m| m.id == match_id);

            let match_ = if let Some(pos) = match_pos {
                // If the match exists, return a mutable reference to it
                &mut tournament.matches[pos]
            } else {
                // If the match doesn't exist, push a new match and then return a mutable reference to it
                tournament.matches.push(NewMatch {
                    id: match_id,
                    name: row.get("match_name"),
                    start_time: row.get("match_start_time"),
                    end_time: row.get("match_end_time"),
                    ruleset: tournament.ruleset,
                    games: Vec::new(),
                });
                tournament.matches.last_mut().unwrap()
            };

            println!("Tournament: {:?}", tournament);
            tournaments.push(tournament);
        }

        tournaments
    }

    // Access the underlying Client
    pub fn client(&self) -> Arc<Client> {
        Arc::clone(&self.client)
    }
}
