use super::db_structs::{
    Game, GameScore, Match, Player, PlayerHighestRank, PlayerRating, RatingAdjustment, ReplicationRole, RulesetData,
    TournamentInfo
};
use crate::{model::structures::ruleset::Ruleset, utils::progress_utils::progress_bar};
use bytes::Bytes;
use futures::SinkExt;
use itertools::Itertools;
use log::{debug, error, info, warn};
use postgres_types::ToSql;
use std::{collections::HashMap, sync::Arc, time::Instant};
use tokio::runtime::Handle;
use tokio_postgres::{Client, Error, NoTls, Row};

enum TransactionState {
    Pending,
    Committed,
    RolledBack
}

pub struct DbTransactionGuard {
    client: Arc<Client>,
    state: TransactionState
}

impl DbTransactionGuard {
    fn new(client: Arc<Client>) -> Self {
        Self {
            client,
            state: TransactionState::Pending
        }
    }

    pub async fn commit(&mut self) -> Result<(), Error> {
        match self.state {
            TransactionState::Pending => {
                self.client.batch_execute("COMMIT").await?;
                self.state = TransactionState::Committed;
                Ok(())
            }
            TransactionState::Committed => {
                warn!("Transaction commit called after it was already committed; ignoring");
                Ok(())
            }
            TransactionState::RolledBack => {
                warn!("Transaction commit called after rollback; ignoring");
                Ok(())
            }
        }
    }

    pub async fn rollback(&mut self) -> Result<(), Error> {
        match self.state {
            TransactionState::Pending => {
                self.client.batch_execute("ROLLBACK").await?;
                self.state = TransactionState::RolledBack;
                Ok(())
            }
            TransactionState::RolledBack => {
                warn!("Transaction rollback called repeatedly; ignoring");
                Ok(())
            }
            TransactionState::Committed => {
                warn!("Transaction rollback requested after commit; ignoring");
                Ok(())
            }
        }
    }
}

impl Drop for DbTransactionGuard {
    fn drop(&mut self) {
        if matches!(self.state, TransactionState::Pending) {
            let client = Arc::clone(&self.client);
            if Handle::try_current().is_ok() {
                tokio::spawn(async move {
                    if let Err(e) = client.batch_execute("ROLLBACK").await {
                        error!("Failed to rollback transaction during drop: {}", e);
                    } else {
                        info!("ROLLBACK TRANSACTION (triggered in Drop)");
                    }
                });
            } else {
                error!("Runtime unavailable to rollback dangling transaction; transaction left pending");
            }
        }
    }
}

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
        // 1. Only consider verified tournaments with verified matches.
        // 2. From these matches, we only include the games and scores which are verified.
        //
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
              AND gs.verification_status = 4
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

        // Log any matches that have no games or games with no scores
        for match_ in &matches {
            if match_.games.is_empty() {
                log::error!(
                    "Match ID {} ('{}') has no games! This match will be skipped during processing.",
                    match_.id,
                    match_.name
                );
            } else {
                for game in &match_.games {
                    if game.scores.is_empty() {
                        log::error!(
                            "Game ID {} in Match ID {} ('{}') has no scores! This game will cause issues during processing.",
                            game.id, match_.id, match_.name
                        );
                    }
                }
            }
        }

        info!("Match fetching complete - {} matches will be processed", matches.len());

        matches
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

        if let (Ok(ruleset_val), Ok(global_rank_val), Ok(earliest_global_rank_val)) =
            (ruleset, global_rank, earliest_global_rank)
        {
            let parsed_ruleset = Ruleset::try_from(ruleset_val);
            if parsed_ruleset.is_err() {
                // Return nothing
                return None;
            }

            return Some(RulesetData {
                ruleset: parsed_ruleset.unwrap(),
                global_rank: global_rank_val,
                earliest_global_rank: earliest_global_rank_val
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
        let p_bar = progress_bar(player_ratings.len() as u64, "Saving player ratings to db".to_string());

        let mut mapping: HashMap<i32, Vec<RatingAdjustment>> = HashMap::new();
        let parent_ids = self.save_player_ratings(player_ratings).await;

        if let Some(ref bar) = p_bar {
            bar.inc(1);
            bar.finish();
        }

        for (i, rating) in player_ratings.iter().enumerate() {
            let parent_id = parent_ids.get(i).unwrap();
            mapping.insert(*parent_id, rating.adjustments.clone());
        }

        info!("Adjustment parent_id mapping created");

        self.save_rating_adjustments(&mapping).await;

        info!("Rating adjustments saved");
    }

    /// Save all rating adjustments in a single batch query using PostgreSQL COPY
    async fn save_rating_adjustments(&self, adjustment_mapping: &HashMap<i32, Vec<RatingAdjustment>>) {
        if adjustment_mapping.is_empty() {
            return;
        }

        let copy_query = "COPY rating_adjustments (player_id, ruleset, player_rating_id, match_id, \
        rating_before, rating_after, volatility_before, volatility_after, timestamp, adjustment_type) \
        FROM STDIN WITH (FORMAT TEXT, DELIMITER E'\\t')";

        let p_bar = progress_bar(adjustment_mapping.len() as u64, "Saving rating adjustments".to_string());

        let sink = self
            .client
            .copy_in(copy_query)
            .await
            .expect("Failed to initiate COPY IN operation");

        tokio::pin!(sink);

        for (player_rating_id, adjustments) in adjustment_mapping.iter() {
            for adjustment in adjustments {
                let match_id_str = adjustment
                    .match_id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "\\N".to_string());

                let row_data = format!(
                    "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                    adjustment.player_id,
                    adjustment.ruleset as i32,
                    player_rating_id,
                    match_id_str,
                    adjustment.rating_before,
                    adjustment.rating_after,
                    adjustment.volatility_before,
                    adjustment.volatility_after,
                    adjustment.timestamp.format("%Y-%m-%d %H:%M:%S"),
                    adjustment.adjustment_type as i32
                );

                let data_bytes = Bytes::from(row_data.into_bytes());
                sink.send(data_bytes)
                    .await
                    .expect("Failed to send data to COPY operation");
            }
            if let Some(ref bar) = p_bar {
                bar.inc(1);
            }
        }

        sink.close().await.expect("Failed to finalize COPY operation");

        if let Some(bar) = p_bar {
            bar.finish();
        }
    }

    /// Saves multiple PlayerRatings using COPY for efficiency, returning a vector of primary keys
    async fn save_player_ratings(&self, player_ratings: &[PlayerRating]) -> Vec<i32> {
        if player_ratings.is_empty() {
            error!("No player_rating data to save to database");
            panic!();
        }

        let copy_query = "COPY player_ratings (player_id, ruleset, rating, volatility, \
            percentile, global_rank, country_rank) \
            FROM STDIN WITH (FORMAT TEXT, DELIMITER E'\\t')";

        let sink = self
            .client
            .copy_in(copy_query)
            .await
            .expect("Failed to initiate COPY IN operation for player_ratings");

        tokio::pin!(sink);

        let p_bar = progress_bar(player_ratings.len() as u64, "Saving player ratings".to_string());

        for rating in player_ratings {
            let row_data = format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                rating.player_id,
                rating.ruleset as i32,
                rating.rating,
                rating.volatility,
                rating.percentile,
                rating.global_rank,
                rating.country_rank
            );

            let data_bytes = Bytes::from(row_data.into_bytes());
            sink.send(data_bytes)
                .await
                .expect("Failed to send data to COPY operation");

            if let Some(ref bar) = p_bar {
                bar.inc(1);
            }
        }

        sink.close()
            .await
            .expect("Failed to finalize COPY operation for player_ratings");

        if let Some(bar) = p_bar {
            bar.finish();
        }

        // Query back the IDs - we need to match on the unique combination of fields
        // Since we just inserted these records, we can order by ID and take the last N records
        let count = player_ratings.len() as i64;
        let rows = self
            .client
            .query("SELECT id FROM player_ratings ORDER BY id DESC LIMIT $1", &[&count])
            .await
            .unwrap();

        // Reverse to get them in insertion order
        let mut ids: Vec<i32> = rows.iter().map(|row| row.get("id")).collect();
        ids.reverse();
        ids
    }

    async fn insert_or_update_highest_ranks(&self, player_ratings: &[PlayerRating]) {
        info!("Fetching all highest ranks");
        let current_highest_ranks = self.get_highest_ranks().await;

        info!("Found {} highest ranks", current_highest_ranks.len());

        let pbar = progress_bar(player_ratings.len() as u64, "Processing highest ranks".to_string());

        let mut new_ranks = Vec::new();
        let mut updates = Vec::new();

        for rating in player_ratings {
            if let Some(Some(current_rank)) = current_highest_ranks.get(&(rating.player_id, rating.ruleset)) {
                if rating.global_rank < current_rank.global_rank {
                    updates.push(rating);
                }
            } else {
                new_ranks.push(rating);
            }
            if let Some(ref bar) = pbar {
                bar.inc(1);
            }
        }

        if let Some(bar) = pbar {
            bar.finish_with_message("Processed highest ranks classification");
        }

        // Batch insert new ranks using COPY
        if !new_ranks.is_empty() {
            self.batch_insert_highest_ranks(&new_ranks).await;
        }

        // Update existing ranks
        if !updates.is_empty() {
            let update_pbar = progress_bar(updates.len() as u64, "Updating existing highest ranks".to_string());
            for rating in updates {
                self.update_highest_rank(rating.player_id, rating).await;
                if let Some(ref bar) = update_pbar {
                    bar.inc(1);
                }
            }
            if let Some(bar) = update_pbar {
                bar.finish();
            }
        }
    }

    async fn batch_insert_highest_ranks(&self, player_ratings: &[&PlayerRating]) {
        let copy_query = "COPY player_highest_ranks (player_id, ruleset, global_rank, global_rank_date, country_rank, country_rank_date) \
            FROM STDIN WITH (FORMAT TEXT, DELIMITER E'\\t')";

        let sink = self
            .client
            .copy_in(copy_query)
            .await
            .expect("Failed to initiate COPY IN operation for player_highest_ranks");

        tokio::pin!(sink);

        let p_bar = progress_bar(player_ratings.len() as u64, "Inserting new highest ranks".to_string());

        for rating in player_ratings {
            let timestamp = rating.adjustments.last().unwrap().timestamp;
            let row_data = format!(
                "{}\t{}\t{}\t{}\t{}\t{}\n",
                rating.player_id,
                rating.ruleset as i32,
                rating.global_rank,
                timestamp.format("%Y-%m-%d %H:%M:%S"),
                rating.country_rank,
                timestamp.format("%Y-%m-%d %H:%M:%S")
            );

            let data_bytes = Bytes::from(row_data.into_bytes());
            sink.send(data_bytes)
                .await
                .expect("Failed to send data to COPY operation");

            if let Some(ref bar) = p_bar {
                bar.inc(1);
            }
        }

        sink.close()
            .await
            .expect("Failed to finalize COPY operation for player_highest_ranks");

        if let Some(bar) = p_bar {
            bar.finish();
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

    pub async fn get_tournament_info_for_matches(&self, matches: &[Match]) -> HashMap<i32, TournamentInfo> {
        let mut tournament_info: HashMap<i32, TournamentInfo> = HashMap::new();

        if matches.is_empty() {
            return tournament_info;
        }

        // Get unique match IDs
        let match_ids: Vec<i32> = matches.iter().map(|m| m.id).collect();
        let match_id_str = match_ids.iter().map(|id| id.to_string()).join(",");

        // Query to get tournament information for the processed matches
        let query = format!(
            "SELECT DISTINCT 
                t.id AS tournament_id,
                t.name AS tournament_name,
                COUNT(DISTINCT m.id) AS match_count,
                COUNT(DISTINCT gs.player_id) AS player_count
            FROM tournaments t
            JOIN matches m ON t.id = m.tournament_id
            JOIN games g ON m.id = g.match_id
            JOIN game_scores gs ON g.id = gs.game_id
            WHERE m.id = ANY(ARRAY[{}])
            GROUP BY t.id, t.name",
            match_id_str
        );

        match self.client.query(&query, &[]).await {
            Ok(rows) => {
                for row in rows {
                    let tournament_id: i32 = row.get("tournament_id");
                    let info = TournamentInfo {
                        id: tournament_id,
                        name: row.get("tournament_name"),
                        match_count: row.get::<_, i64>("match_count") as i32,
                        player_count: row.get::<_, i64>("player_count") as i32
                    };
                    tournament_info.insert(tournament_id, info);
                }
            }
            Err(e) => {
                log::error!("Failed to fetch tournament information: {}", e);
            }
        }

        tournament_info
    }

    async fn truncate_table(&self, table: &str) {
        self.client
            .execute(format!("TRUNCATE TABLE {table} RESTART IDENTITY CASCADE").as_str(), &[])
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
            .execute(&format!("SET session_replication_role = '{role_str}';"), &[])
            .await
            .unwrap();

        info!("Executed SET session_replication_role = '{role_str}';")
    }

    // Access the underlying Client
    pub fn client(&self) -> Arc<Client> {
        Arc::clone(&self.client)
    }

    pub async fn begin_transaction(&self) -> Result<DbTransactionGuard, Error> {
        self.client.batch_execute("BEGIN").await?;
        Ok(DbTransactionGuard::new(Arc::clone(&self.client)))
    }

    /// Calculate and update placements for all game scores
    /// Verified scores get placement based on score ranking (1=highest)
    /// Non-verified scores get placement=0
    pub async fn calculate_and_update_game_score_placements(&self) {
        info!("Calculating game score placements...");
        let timer = Instant::now();

        let mut total_scores: Option<i64> = None;
        if log::log_enabled!(log::Level::Debug) {
            if let Ok(stats_row) = self
                .client
                .query_one(
                    "
                    SELECT 
                        COUNT(*) AS total_scores,
                        COUNT(*) WHERE verification_status = 4 AS verified_scores
                    FROM game_scores
                ",
                    &[]
                )
                .await
            {
                let total: i64 = stats_row.get("total_scores");
                let verified: i64 = stats_row.get("verified_scores");
                let pending = total - verified;

                debug!(
                    "Starting placement recalculation for {} scores ({} verified, {} pending verification)",
                    total, verified, pending
                );

                total_scores = Some(total);
            }
        }

        if total_scores.is_none() {
            info!("Starting placement recalculation for game scores");
        }

        let updated_count = self
            .client
            .execute(
                "
                UPDATE game_scores AS gs
                SET placement = np.new_placement,
                    updated = NOW() AT TIME ZONE 'UTC'
                FROM (
                    SELECT
                        id,
                        CASE
                            WHEN verification_status = 4 THEN
                                SUM(CASE WHEN verification_status = 4 THEN 1 ELSE 0 END)
                                    OVER (
                                        PARTITION BY game_id
                                        ORDER BY score DESC, id
                                        ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW
                                    )
                            ELSE 0
                        END AS new_placement
                    FROM game_scores
                ) AS np
                WHERE gs.id = np.id
                    AND gs.placement IS DISTINCT FROM np.new_placement
            ",
                &[]
            )
            .await
            .unwrap();

        let elapsed_secs = timer.elapsed().as_secs_f64();

        match (updated_count, total_scores) {
            (0, Some(total)) => info!(
                "Placements already up to date; scanned {} scores in {:.3}s",
                total, elapsed_secs
            ),
            (0, None) => info!("Placements already up to date in {:.3}s", elapsed_secs),
            (_, Some(total)) => info!(
                "Updated placements for {} game scores in {:.3}s (out of {})",
                updated_count, elapsed_secs, total
            ),
            (_, None) => info!(
                "Updated placements for {} game scores in {:.3}s",
                updated_count, elapsed_secs
            )
        }
    }
}
