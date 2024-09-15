use crate::{model::structures::ruleset::Ruleset, utils::progress_utils::progress_bar};
use indexmap::map::Slice;
use itertools::Itertools;
use postgres_types::ToSql;
use std::{collections::HashMap, sync::Arc};
use tokio_postgres::{Client, Error, NoTls, Row};

use super::db_structs::{Game, GameScore, Match, Player, PlayerRating, RatingAdjustment, RulesetData};

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
        WHERE t.verification_status = 4 AND m.verification_status = 4 AND g.verification_status = 4
        AND gs.verification_status = 4
        ORDER BY gs.id", &[]).await.unwrap();

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
                    ruleset_data: match self.ruleset_data_from_row(&row) {
                        Some(data) => Some(vec![data]),
                        None => None
                    }
                };
                players.push(player);
                current_player_id = row.get("player_id");
            } else {
                // Same player, new ruleset data

                let data = self.ruleset_data_from_row(&row);
                match data {
                    Some(ruleset_data) => players
                        .last_mut()
                        .unwrap()
                        .ruleset_data
                        .clone()
                        .unwrap_or_default()
                        .push(ruleset_data),
                    None => ()
                }
            }
        }

        players
    }

    fn ruleset_data_from_row(&self, row: &Row) -> Option<RulesetData> {
        let ruleset = row.try_get::<_, i32>("ruleset");
        let global_rank = row.try_get::<_, i32>("global_rank");
        let earliest_global_rank = row.try_get::<_, Option<i32>>("earliest_global_rank");

        if ruleset.is_ok() && global_rank.is_ok() && earliest_global_rank.is_ok() {
            let parsed_ruleset = Ruleset::try_from(ruleset.unwrap());
            if parsed_ruleset.is_err() {
                // Return nothing
                return None;
            }

            return Some(RulesetData {
                ruleset: Ruleset::try_from(parsed_ruleset.unwrap()).unwrap(),
                global_rank: global_rank.unwrap(),
                earliest_global_rank: earliest_global_rank.unwrap()
            });
        }

        None
    }

    pub async fn save_results(&self, player_ratings: &[PlayerRating]) {
        self.truncate_rating_adjustments().await;
        self.truncate_player_ratings().await;

        let p_bar = progress_bar(player_ratings.len() as u64, "Saving player ratings to db".to_string()).unwrap();

        let mut mapping: HashMap<i32, Vec<RatingAdjustment>> = HashMap::new();
        let parent_ids = self.save_player_ratings(&player_ratings).await;

        p_bar.inc(1);
        p_bar.finish();

        for (i, rating) in player_ratings.iter().enumerate() {
            let parent_id = parent_ids.get(i).unwrap();
            mapping.insert(*parent_id, rating.adjustments.clone());
        }

        println!("Adjustment parent_id mapping created");

        self.save_rating_adjustments(&mapping).await;
    }

    /// Save all rating adjustments in a single batch query
    async fn save_rating_adjustments(&self, adjustment_mapping: &HashMap<i32, Vec<RatingAdjustment>>) {
        // Prepare the base query
        let base_query = "INSERT INTO rating_adjustments (player_id, player_rating_id, match_id, \
        rating_before, rating_after, volatility_before, volatility_after, timestamp, adjustment_type) \
        VALUES ";

        // Collect parameters for batch insertion
        let mut values: Vec<String> = Vec::new();

        let p_bar = progress_bar(adjustment_mapping.len() as u64, "Creating rating adjustment queries".to_string()).unwrap();
        for (player_rating_id, adjustments) in adjustment_mapping.iter() {
            for adjustment in adjustments {
                // Create a tuple for each adjustment
                let match_id = adjustment.match_id.map_or("NULL".to_string(), |id| id.to_string());

                let value_tuple = format!(
                    "({}, {}, {}, {}, {}, {}, {}, '{}', {})",
                    adjustment.player_id,
                    player_rating_id,
                    match_id,
                    adjustment.rating_before,
                    adjustment.rating_after,
                    adjustment.volatility_before,
                    adjustment.volatility_after,
                    adjustment.timestamp.format("%Y-%m-%d %H:%M:%S"), // Assuming timestamp is NaiveDateTime
                    adjustment.adjustment_type as i32
                );
                values.push(value_tuple);
            }

            p_bar.inc(1);
        }

        p_bar.finish();

        // Combine the query with all the values
        let full_query = format!("{}{}", base_query, values.join(", "));
        let empty: Vec<String> = Vec::new();

        // Execute the batch query
        self.client
            .execute_raw(&full_query, &empty)
            .await
            .expect("Failed to execute bulk insert");
    }

    /// Saves multiple PlayerRatings, returning a vector of primary keys
    async fn save_player_ratings(&self, player_ratings: &[PlayerRating]) -> Vec<i32> {
        // Create a list of value placeholders
        let mut query = "INSERT INTO player_ratings (player_id, ruleset, rating, volatility, \
                     percentile, global_rank, country_rank) VALUES"
            .to_string();
        let mut value_placeholders: Vec<String> = Vec::new();

        for (i, rating) in player_ratings.iter().enumerate() {
            // Directly embed the values into the query string
            value_placeholders.push(format!(
                "({}, {}, {}, {}, {}, {}, {})",
                rating.player_id,
                rating.ruleset as i32,
                rating.rating,
                rating.volatility,
                rating.percentile,
                rating.global_rank,
                rating.country_rank
            ));
        }

        query += &value_placeholders.join(", ");
        query += " RETURNING id";

        // Execute the batch insert
        let rows = self.client.query(query.as_str(), &[]).await.unwrap();

        // Collect and return the IDs
        rows.iter().map(|row| row.get("id")).collect()
    }

    async fn truncate_player_ratings(&self) {
        self.client
            .execute("TRUNCATE TABLE player_ratings CASCADE", &[])
            .await
            .unwrap();
        println!("Truncated player_ratings table!");
    }

    pub async fn set_match_processing_status_done(&self, matches: &[Match]) {
        let bar = progress_bar(
            matches.len() as u64,
            "Updating processing status for all matches".to_string()
        )
        .unwrap();
        for match_ in matches {
            self.client
                .execute(
                    "UPDATE matches SET processing_status = 5 WHERE match_id = $1",
                    &[&match_.id]
                )
                .await
                .unwrap();

            bar.inc(1)
        }
    }

    async fn truncate_rating_adjustments(&self) {
        self.client
            .execute("TRUNCATE TABLE rating_adjustments CASCADE", &[])
            .await
            .unwrap();

        println!("Truncated rating_adjustments table!");
    }

    // Access the underlying Client
    pub fn client(&self) -> Arc<Client> {
        Arc::clone(&self.client)
    }
}
