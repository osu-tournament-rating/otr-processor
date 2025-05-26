use super::db_structs::{
    Game, GameScore, Match, Player, PlayerHighestRank, PlayerRating, RatingAdjustment, ReplicationRole, RulesetData
};
use crate::{model::structures::ruleset::Ruleset, utils::progress_utils::progress_bar};
use itertools::Itertools;
use log::{error, info};
use postgres_types::ToSql;
use std::{collections::HashMap, sync::Arc};
use tokio_postgres::{Client, Error, NoTls, Row};

#[derive(Clone)]
pub struct DbClient {
    client: Arc<Client>,
    ignore_constraints: bool
}

impl DbClient {
    // Connect to the database and return a DbClient instance
    pub async fn connect(connection_str: &str, ignore_constraints: bool) -> Result<Self, Error> {
        let (client, connection) = tokio_postgres::connect(connection_str, NoTls).await?;

        // Spawn the connection object to run in the background
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                error!("connection error: {}", e);
            }
        });

        Ok(DbClient {
            client: Arc::new(client),
            ignore_constraints
        })
    }

    pub async fn get_matches(&self) -> Vec<Match> {
        let mut matches_map: HashMap<i32, Match> = HashMap::new();
        let mut games_map: HashMap<i32, Game> = HashMap::new();
        let mut scores_map: HashMap<i32, GameScore> = HashMap::new();

        // Link match ids and game ids
        let mut match_games_link_map: HashMap<i32, Vec<i32>> = HashMap::new();

        // Link game ids and score ids
        let mut game_scores_link_map: HashMap<i32, Vec<i32>> = HashMap::new();

        // The WHERE query here does the following:
        //
        // 1. Only consider verified tournaments with verified matches that have a processing_status of 'NeedsProcessorData'.
        // 2. From these matches, we only include the games and scores which are verified.
        //
        //  We can safely assume that for all matches awaiting processor data every
        //     game and game score is completely done with processing
        info!("Fetching matches...");
        let rows = self.client.query("
            SELECT
                t.id AS tournament_id, t.name AS tournament_name, t.ruleset AS tournament_ruleset,
                m.id AS match_id, m.name AS match_name, m.start_time AS match_start_time, m.end_time AS match_end_time, m.tournament_id AS match_tournament_id,
                g.id AS game_id, g.ruleset AS game_ruleset, g.start_time AS game_start_time, g.end_time AS game_end_time, g.match_id AS game_match_id,
                gs.id AS game_score_id, gs.player_id AS game_score_player_id, gs.game_id AS game_score_game_id, gs.score AS game_score_score, gs.placement AS game_score_placement
            FROM tournaments t
                     JOIN matches m ON t.id = m.tournament_id
                     JOIN games g ON m.id = g.match_id
                     JOIN game_scores gs ON g.id = gs.game_id
            WHERE t.verification_status = 4 AND m.verification_status = 4 AND g.verification_status = 4
              AND gs.verification_status = 4 AND m.processing_status = 4
            ORDER BY gs.id;", &[]).await.unwrap();

        info!("Matches fetched, iterating...");

        for row in rows {
            let match_id = row.get::<_, i32>("match_id");
            let game_id = row.get::<_, i32>("game_id");
            let score_id = row.get::<_, i32>("game_score_id");

            matches_map
                .entry(match_id)
                .or_insert_with(|| Self::match_from_row(&row));

            games_map.entry(game_id).or_insert_with(|| Self::game_from_row(&row));
            scores_map.entry(score_id).or_insert_with(|| Self::score_from_row(&row));

            // Link ids back to parents
            match_games_link_map.entry(match_id).or_default().push(game_id);
            game_scores_link_map.entry(game_id).or_default().push(score_id);
        }

        info!("Linking ids...");
        for (game_id, mut score_ids) in game_scores_link_map {
            score_ids.dedup();

            for score_id in score_ids {
                games_map
                    .get_mut(&game_id)
                    .unwrap()
                    .scores
                    .push(scores_map.get(&score_id).unwrap().clone());
            }
        }

        for (match_id, mut game_ids) in match_games_link_map {
            game_ids.dedup();

            for game_id in game_ids {
                matches_map
                    .get_mut(&match_id)
                    .unwrap()
                    .games
                    .push(games_map.get(&game_id).unwrap().clone());
            }
        }

        let mut matches = matches_map.values().cloned().collect_vec();
        matches.sort_by(|a, b| a.start_time.cmp(&b.start_time));

        info!("Match fetching complete");
        matches
    }

    pub async fn rollback_processing_statuses(&self) {
        if self.ignore_constraints {
            self.set_replication(ReplicationRole::Replica).await;
        }

        let tournament_id_sql =
            "SELECT tournament_id FROM matches WHERE processing_status = 5 AND verification_status = 4;";

        let mut tournament_update_sql = Vec::new();
        let mut match_update_sql = Vec::new();
        let id_result = self.client.query(tournament_id_sql, &[]).await;

        if id_result.is_ok() {
            for row in id_result.unwrap().iter() {
                tournament_update_sql.push(format!(
                    "UPDATE tournaments SET processing_status = 4 \
                WHERE id = {};\n",
                    row.get::<_, i32>(0)
                ));
                match_update_sql.push(format!(
                    "UPDATE matches SET processing_status = 4 WHERE \
                    processing_status = 5 AND tournament_id = {} AND verification_status = 4;\n",
                    row.get::<_, i32>(0)
                ))
            }
        } else {
            if self.ignore_constraints {
                self.set_replication(ReplicationRole::Origin).await;
            }
            panic!("Failed to fetch tournament ids");
        }

        let p_bar = progress_bar(2, "Rolling back tournament processing statuses".to_string()).unwrap();

        // Update tournaments
        self.client
            .batch_execute(tournament_update_sql.join("\n").as_str())
            .await
            .expect("Failed to batch execute tournament processing status rollback");

        p_bar.inc(1);
        p_bar.set_message("Rolling back match processing statuses");

        // Update matches
        self.client
            .batch_execute(match_update_sql.join("\n").as_str())
            .await
            .expect("Failed to execute match processing status rollback");

        p_bar.inc(1);

        if self.ignore_constraints {
            self.set_replication(ReplicationRole::Origin).await;
        }

        p_bar.finish_with_message("Completed processing status rollback for tournaments and matches")
    }

    fn match_from_row(row: &Row) -> Match {
        Match {
            id: row.get("match_id"),
            name: row.get("match_name"),
            start_time: row.get("match_start_time"),
            end_time: row.get("match_end_time"),
            ruleset: Ruleset::try_from(row.get::<_, i32>("tournament_ruleset")).unwrap(),
            games: Vec::new()
        }
    }

    fn game_from_row(row: &Row) -> Game {
        Game {
            id: row.get("game_id"),
            ruleset: Ruleset::try_from(row.get::<_, i32>("game_ruleset")).unwrap(),
            start_time: row.get("game_start_time"),
            end_time: row.get("game_end_time"),
            scores: Vec::new()
        }
    }

    fn score_from_row(row: &Row) -> GameScore {
        GameScore {
            id: row.get("game_score_id"),
            player_id: row.get("game_score_player_id"),
            game_id: row.get("game_score_game_id"),
            score: row.get("game_score_score"),
            placement: row.get("game_score_placement")
        }
    }

    /// Fetches all players from the database with their ruleset data.
    /// If a player has no ruleset data, they will not be included.
    /// This is important because a player could be in the system but still
    /// be waiting for the dataworker to process their data.
    pub async fn get_players(&self) -> Vec<Player> {
        info!("Fetching players...");
        let mut players: Vec<Player> = Vec::new();
        let rows = self
            .client
            .query(
                "SELECT p.id AS player_id, p.username AS username, \
        p.country AS country, prd.ruleset AS ruleset, prd.earliest_global_rank AS earliest_global_rank,\
          prd.global_rank AS global_rank FROM players p \
        JOIN player_osu_ruleset_data prd ON prd.player_id = p.id \
        ORDER BY p.id",
                &[]
            )
            .await
            .unwrap();

        let mut current_player_id = -1;
        let mut current_ruleset_data: Vec<RulesetData> = Vec::new();

        for (i, row) in rows.iter().enumerate() {
            let player_id = row.get::<_, i32>("player_id");

            // If we're at the end of the loop, or the player id has changed, save the previous player's ruleset data.
            if player_id != current_player_id || i == rows.len() - 1 {
                // If they had no ruleset data, the `current_ruleset_data` vector will be empty.
                if current_player_id != -1 {
                    if let Some(last_player) = players.last_mut() {
                        last_player.ruleset_data = Some(current_ruleset_data.clone());
                    }
                }

                // Start a new player
                current_player_id = player_id;

                // Clear out previous player's ruleset data
                current_ruleset_data.clear();

                let player = Player {
                    id: row.get("player_id"),
                    username: row.get("username"),
                    country: row.get("country"),
                    // Saved when the player id changes or the last row is reached.
                    ruleset_data: Some(Vec::new())
                };
                players.push(player);
            }

            // Push this row's ruleset data
            if let Some(ruleset_data) = self.ruleset_data_from_row(row) {
                current_ruleset_data.push(ruleset_data);
            }
        }

        info!("Players fetched");
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
                ruleset: parsed_ruleset.unwrap(),
                global_rank: global_rank.unwrap(),
                earliest_global_rank: earliest_global_rank.unwrap()
            });
        }

        None
    }

    pub async fn save_results(&self, player_ratings: &[PlayerRating]) {
        self.truncate_table("rating_adjustments").await;
        self.truncate_table("player_ratings").await;

        self.save_ratings_and_adjustments_with_mapping(&player_ratings).await;
        self.insert_or_update_highest_ranks(player_ratings).await;
    }

    async fn save_ratings_and_adjustments_with_mapping(&self, player_ratings: &&[PlayerRating]) {
        let p_bar = progress_bar(player_ratings.len() as u64, "Saving player ratings to db".to_string()).unwrap();

        let mut mapping: HashMap<i32, Vec<RatingAdjustment>> = HashMap::new();
        let parent_ids = self.save_player_ratings(player_ratings).await;

        p_bar.inc(1);
        p_bar.finish();

        for (i, rating) in player_ratings.iter().enumerate() {
            let parent_id = parent_ids.get(i).unwrap();
            mapping.insert(*parent_id, rating.adjustments.clone());
        }

        info!("Adjustment parent_id mapping created");

        self.save_rating_adjustments(&mapping).await;

        info!("Rating adjustments saved");
    }

    /// Save all rating adjustments in a single batch query
    async fn save_rating_adjustments(&self, adjustment_mapping: &HashMap<i32, Vec<RatingAdjustment>>) {
        // Prepare the base query
        let base_query = "INSERT INTO rating_adjustments (player_id, ruleset, player_rating_id, match_id, \
        rating_before, rating_after, volatility_before, volatility_after, timestamp, adjustment_type) \
        VALUES ";

        // Collect parameters for batch insertion
        let mut values: Vec<String> = Vec::new();

        let p_bar = progress_bar(
            adjustment_mapping.len() as u64,
            "Creating rating adjustment queries".to_string()
        )
        .unwrap();
        for (player_rating_id, adjustments) in adjustment_mapping.iter() {
            for adjustment in adjustments {
                // Create a tuple for each adjustment
                let match_id = adjustment.match_id.map_or("NULL".to_string(), |id| id.to_string());

                let value_tuple = format!(
                    "({}, {}, {}, {}, {}, {}, {}, {}, '{}', {})",
                    adjustment.player_id,
                    adjustment.ruleset as i32,
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

        for rating in player_ratings.iter() {
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

        if value_placeholders.is_empty() {
            error!("No player_rating data to save to database");
            panic!();
        }

        // Execute the batch insert
        let rows = self.client.query(query.as_str(), &[]).await.unwrap();

        // Collect and return the IDs
        rows.iter().map(|row| row.get("id")).collect()
    }

    async fn insert_or_update_highest_ranks(&self, player_ratings: &[PlayerRating]) {
        info!("Fetching all highest ranks");
        let current_highest_ranks = self.get_highest_ranks().await;

        info!("Found {} highest ranks", current_highest_ranks.len());
        // If the current rank is None, create it. If the current rank is Some and
        // either the PlayerRating's global rank or country rank is higher than the current highest
        // rank, update it.
        //
        // Only update values which are higher than the current highest rank

        let pbar = progress_bar(player_ratings.len() as u64, "Updating highest ranks".to_string()).unwrap();

        for rating in player_ratings {
            if let Some(Some(current_rank)) = current_highest_ranks.get(&(rating.player_id, rating.ruleset)) {
                if rating.global_rank < current_rank.global_rank {
                    self.update_highest_rank(rating.player_id, rating).await;
                }
            } else {
                self.insert_highest_rank(rating.player_id, rating).await;
            }

            pbar.inc(1);
        }
    }

    async fn get_highest_ranks(&self) -> HashMap<(i32, Ruleset), Option<PlayerHighestRank>> {
        let query = "SELECT * FROM player_highest_ranks";
        let row = self.client.query(query, &[]).await.ok();

        match row {
            Some(rows) => {
                let mut map: HashMap<(i32, Ruleset), Option<PlayerHighestRank>> = HashMap::new();
                for row in rows {
                    let player_id = row.get::<_, i32>("player_id");
                    let ruleset = Ruleset::try_from(row.get::<_, i32>("ruleset")).unwrap();
                    map.insert(
                        (player_id, ruleset),
                        Some(PlayerHighestRank {
                            id: row.get("id"),
                            player_id,
                            global_rank: row.get("global_rank"),
                            global_rank_date: row.get("global_rank_date"),
                            country_rank: row.get("country_rank"),
                            country_rank_date: row.get("country_rank_date"),
                            ruleset
                        })
                    );
                }

                map
            }
            None => HashMap::new()
        }
    }

    async fn insert_highest_rank(&self, player_id: i32, player_rating: &PlayerRating) {
        let timestamp = player_rating.adjustments.last().unwrap().timestamp;
        let query = "INSERT INTO player_highest_ranks (player_id, ruleset, global_rank, global_rank_date, country_rank, country_rank_date) VALUES ($1, $2, $3, $4, $5, $6)";
        let values: &[&(dyn ToSql + Sync)] = &[
            &player_id,
            &(player_rating.ruleset as i32),
            &player_rating.global_rank,
            &timestamp,
            &player_rating.country_rank,
            &timestamp
        ];

        self.client.execute(query, values).await.unwrap();
    }

    async fn update_highest_rank(&self, player_id: i32, player_rating: &PlayerRating) {
        let timestamp = player_rating.adjustments.last().unwrap().timestamp;
        let query = "UPDATE player_highest_ranks SET global_rank = $1, global_rank_date = $2, country_rank = $3, country_rank_date = $4 WHERE player_id = $5 AND ruleset = $6";
        let values: &[&(dyn ToSql + Sync)] = &[
            &player_rating.global_rank,
            &timestamp,
            &player_rating.country_rank,
            &timestamp,
            &player_id,
            &(player_rating.ruleset as i32)
        ];

        self.client.execute(query, values).await.unwrap();
    }

    pub async fn roll_forward_processing_statuses(&self, matches: &[Match]) {
        info!("Updating processing status for all matches");

        let data = matches.iter().map(|f| f.id).collect_vec();
        let match_id_str = data.into_iter().join(",");

        let match_update_sql = format!(
            "UPDATE matches SET processing_status \
        = 5 WHERE id = ANY(ARRAY[{}])",
            match_id_str
        );

        self.client.execute(match_update_sql.as_str(), &[]).await.unwrap();
    }

    async fn truncate_table(&self, table: &str) {
        self.client
            .execute(
                format!("TRUNCATE TABLE {} RESTART IDENTITY CASCADE", table).as_str(),
                &[]
            )
            .await
            .unwrap();

        info!("Truncated the {} table!", table);
    }

    async fn set_replication(&self, replication_role: ReplicationRole) {
        let role_str = match replication_role {
            ReplicationRole::Replica => "replica",
            ReplicationRole::Origin => "origin"
        };

        self.client
            .execute(&format!("SET session_replication_role = '{}';", role_str), &[])
            .await
            .unwrap();

        info!("Executed SET session_replication_role = '{}';", role_str)
    }

    // Access the underlying Client
    pub fn client(&self) -> Arc<Client> {
        Arc::clone(&self.client)
    }
}
