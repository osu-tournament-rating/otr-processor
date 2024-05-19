use std::{sync::Arc, time::Duration};

use reqwest::{
    header::{AUTHORIZATION, CONTENT_TYPE},
    Client, ClientBuilder, Error, Method
};
use serde::{de::DeserializeOwned, Serialize};
use tokio::sync::{
    oneshot::{Receiver, Sender},
    RwLock
};

use crate::{
    api::api_structs::{
        BaseStats, GameWinRecord, MatchRatingStats, MatchWinRecord, OAuthResponse, Player, PlayerCountryMapping,
        PlayerMatchStats, RatingAdjustment
    },
    utils::progress_utils::progress_bar
};

use self::api_structs::MatchPagedResult;

pub mod api_structs;

/// A loop that automatically refreshes token
pub async fn refresh_token_loop(api: Arc<OtrApiBody>) {
    loop {
        // The first iteration assumes that the refresh token
        //  is already valid, so it sleeps until the expiration time
        let lock = api.token.read().await;
        let expire_in = lock.expire_in;
        drop(lock);

        tokio::time::sleep(Duration::from_secs(expire_in)).await;

        let mut lock = api.token.write().await;

        // Another loop to ensure token is updated correctly.
        // Loops continuously if errors occur.
        loop {
            let res = lock.refresh_token(&api.api_root, &api.client).await;

            match res {
                Ok(_) => break,
                Err(e) => {
                    println!("{:?}", e);
                }
            }
        }

        drop(lock)
    }
}

pub async fn refresh_token_worker(api: Arc<OtrApiBody>, receiver: Receiver<()>) {
    tokio::select! {
        _ = refresh_token_loop(api) => {}
        _ = receiver => {}
    }
}

pub struct OtrToken {
    pub token: String,
    pub refresh_token: String,
    pub expire_in: u64
}

impl OtrToken {
    /// Refreshes access token when called
    pub async fn refresh_token(&mut self, api_root: &str, client: &Client) -> Result<(), Error> {
        let link = format!("{}/v1/oauth/refresh?refreshToken={}", api_root, self.refresh_token);

        let mut response: OAuthResponse = client
            .post(link)
            .header(CONTENT_TYPE, "application/json")
            .send()
            .await?
            .json()
            .await?;

        response.token.insert_str(0, "Bearer ");

        self.token = response.token;
        self.refresh_token = response.refresh_token;
        self.expire_in = response.expire_in;

        Ok(())
    }
}

pub struct OtrApiBody {
    client: Client,
    api_root: String,

    /// Wrapped in RwLock because we need
    /// a shared mutable access
    token: RwLock<OtrToken>
}

pub struct OtrApiClient {
    /// Wrapped in [`Arc`] because everything located in [`OtrApiBody`]
    /// needs to be accessed in different threads (shared reference)
    body: Arc<OtrApiBody>,

    /// Channel that gets sent on `Drop` to shut down the refresh token worker.
    /// Why is it wrapped in an `[Option]`? Because the oneshot sender
    /// consumes itself upon sending.
    ///
    /// Detailed explanation:
    /// In the `Drop` trait implementation, we have a mutable reference
    /// to our struct (where the sender is located). So, when `send()`
    /// occurs, it consumes itself, but we still hold that mutable
    /// reference, and because of this, we encounter a compile error
    /// indicating the variable has been moved.
    /// The workaround is pretty simple:
    ///     Wrap the [`Sender`] inside an `Option`, so we can use [`std::mem::take`]
    ///     to replace our sender with a default value (in our case, [`None`])
    ///     and allow the sender to consume itself peacefully.
    refresh_tx: Option<Sender<()>>
}

impl Drop for OtrApiClient {
    fn drop(&mut self) {
        if let Some(tx) = std::mem::take(&mut self.refresh_tx) {
            // Dropping send() result because either `Ok` or `Err` indicates
            // that the worker and loop are stopped.
            // Ok - means the channel is read and the loop is stopped.
            // Err - means the receiver was somehow dropped beforehand,
            // which means that the worker is not running under any circumstances.
            let _ = tx.send(());
        }
    }
}

impl OtrApiClient {
    /// Constructs API client based on provided token
    pub async fn new(api_root: &str, client_id: &str, client_secret: &str) -> Result<Self, Error> {
        let client = ClientBuilder::new().timeout(Duration::from_secs(10)).build()?;

        let token_response = Self::login(&client, api_root, client_id, client_secret).await?;

        let token = OtrToken {
            token: token_response.token,
            refresh_token: token_response.refresh_token,
            expire_in: token_response.expire_in
        };

        let body = Arc::new(OtrApiBody {
            client,
            api_root: api_root.to_owned(),
            token: RwLock::new(token)
        });

        let (refresh_tx, rx) = tokio::sync::oneshot::channel::<()>();

        // Spawn a refresh token worker
        tokio::spawn(refresh_token_worker(body.clone(), rx));

        Ok(Self {
            refresh_tx: Some(refresh_tx),
            body
        })
    }

    /// Constructs API client based environment variables
    /// see `env_example` in project directory
    ///
    /// # Note
    /// Method logs in as system user so it's expecting
    /// client id and secret in environment variables
    pub async fn new_from_env() -> Result<Self, Error> {
        OtrApiClient::new(
            &std::env::var("API_ROOT").unwrap(),
            &std::env::var("CLIENT_ID").unwrap(),
            &std::env::var("CLIENT_SECRET").unwrap()
        )
        .await
    }

    /// Initial login request to fetch token
    pub async fn login(
        client: &Client,
        api_root: &str,
        client_id: &str,
        client_secret: &str
    ) -> Result<OAuthResponse, Error> {
        let link = format!(
            "{}/v1/oauth/token?clientId={}&clientSecret={}",
            api_root, client_id, client_secret
        );

        let response = client
            .post(link)
            .header(CONTENT_TYPE, "application/json")
            .send()
            .await?;

        let mut json: OAuthResponse = response.json().await?;

        // Putting `Bearer` just to save allocations
        // on every request made
        json.token.insert_str(0, "Bearer ");

        Ok(json)
    }

    /// Wrapper to make authorized requests without body
    ///
    /// See [OtrApiClient::make_request_with_body]
    ///
    /// # Examples
    /// 1. Fetch some endpoint
    /// ```
    /// use reqwest::Method;
    /// use otr_processor::api::OtrApiClient;
    /// let api = OtrApiClient::new("example.com/api/v1", "CLIENT_ID", "CLIENT_SECRET");
    /// // api.make_request(Method::GET, "/fetch_something");
    /// ```
    async fn make_request<T>(&self, method: Method, partial_url: &str) -> Result<T, Error>
    where
        T: DeserializeOwned + Default
    {
        self.make_request_with_body(method, partial_url, None::<u8>).await
    }

    /// Wrapper to make authorized requests with provided body
    ///
    /// # Url
    /// URL constructed like this `{1}{2}`
    ///
    /// Where
    /// 1. API root. Provided when initializing [`OtrApiClient`]
    /// 2. Partial URL that corresponds to endpoint
    ///
    /// # Body
    /// Body should be serializable, see [serde::Serialize]
    ///
    /// # Note
    ///
    /// `/` must present at the beginning of the
    /// partial URL
    ///
    /// # Examples
    /// 1. Make request to some endpoint with `Vec<32>` as body
    /// ```
    /// use reqwest::Method;
    /// use otr_processor::api::OtrApiClient;
    /// let api = OtrApiClient::new("example.com/api/v1", "CLIENT_ID", "CLIENT_SECRET");
    /// let my_numbers: Vec<i32> = vec![1, 2, 3, 4, 5];
    /// // (This commented code doesn't pass doc compilation test ?) let result = api.make_request_with_body(Method::GET, "/fetch_something", Some(&my_numbers));
    /// ```
    async fn make_request_with_body<T, B>(&self, method: Method, partial_url: &str, body: Option<B>) -> Result<T, Error>
    where
        T: DeserializeOwned + Default,
        B: Serialize
    {
        let request_link = format!("{}{}", self.body.api_root, partial_url);

        let mut request = match method {
            Method::GET => self.body.client.get(request_link),
            Method::POST => self.body.client.post(request_link),
            Method::DELETE => self.body.client.delete(request_link),
            _ => unimplemented!()
        };

        if let Some(body) = body {
            request = request.json(&body)
        }

        let lock = &self.body.token.read().await;

        let resp = request
            .header(AUTHORIZATION, &lock.token)
            .header(CONTENT_TYPE, "application/json")
            .send()
            .await?;

        match resp.json().await {
            Ok(res) => Ok(res),
            Err(err) if err.is_decode() => Ok(T::default()),
            Err(e) => Err(e)
        }
    }

    /// Get matches based on provided list of match id's
    /// # Arguments
    /// * `page` - The page number (the response is a paged result)
    /// * `chunk_size` - amount of matches that is going to be fetched
    /// in one request. Done to reduce strain on API side. Recommended
    /// value is `250`
    pub async fn get_matches(&self, page: usize, chunk_size: usize) -> Result<MatchPagedResult, Error> {
        let link = format!("/v1/matches?page={}&limit={}", page, chunk_size);

        self.make_request(Method::GET, &link).await
    }

    /// Get list of players
    pub async fn get_players(&self) -> Result<Vec<Player>, Error> {
        let link = "/v1/players/ranks/all";

        self.make_request(Method::GET, link).await
    }

    /// Get list of player country mappings
    pub async fn get_player_country_mapping(&self) -> Result<Vec<PlayerCountryMapping>, Error> {
        let link = "/v1/players/country-mapping";

        self.make_request(Method::GET, link).await
    }

    /// Post RatingAdjustments
    pub async fn post_adjustments(&self, adjustments: &[RatingAdjustment]) -> Result<(), Error> {
        let link = "/v1/stats/ratingadjustments";

        let bar = progress_bar(adjustments.len() as u64, "Posting rating adjustments".to_string());

        let body = adjustments.chunks(5000);
        Ok(for chunk in body {
            self.make_request_with_body::<(), &[RatingAdjustment]>(Method::POST, link, Some(chunk))
                .await?;
            bar.inc(chunk.len() as u64);
        })
    }

    /// Post PlayerMatchStats
    pub async fn post_player_match_stats(&self, player_match_stats: &[PlayerMatchStats]) -> Result<(), Error> {
        let link = "/v1/stats/matchstats";

        let bar = progress_bar(
            player_match_stats.len() as u64,
            "Posting player match stats".to_string()
        );

        let body = player_match_stats.chunks(5000);
        Ok(for chunk in body {
            self.make_request_with_body::<(), &[PlayerMatchStats]>(Method::POST, link, Some(chunk))
                .await?;
            bar.inc(chunk.len() as u64);
        })
    }

    /// Post MatchRatingStats
    pub async fn post_match_rating_stats(&self, match_rating_stats: &[MatchRatingStats]) -> Result<(), Error> {
        let link = "/v1/stats/ratingstats";

        let bar = progress_bar(
            match_rating_stats.len() as u64,
            "Posting match rating stats".to_string()
        );

        let body = match_rating_stats.chunks(5000);
        Ok(for chunk in body {
            self.make_request_with_body::<(), &[MatchRatingStats]>(Method::POST, link, Some(chunk))
                .await?;
            bar.inc(chunk.len() as u64);
        })
    }

    /// Post BaseStats
    pub async fn post_base_stats(&self, base_stats: &[BaseStats]) -> Result<(), Error> {
        let link = "/v1/stats/basestats";

        let bar = progress_bar(base_stats.len() as u64, "Posting base stats".to_string());

        let body = base_stats.chunks(5000);
        Ok(for chunk in body {
            self.make_request_with_body::<(), &[BaseStats]>(Method::POST, link, Some(chunk))
                .await?;
            bar.inc(chunk.len() as u64);
        })
    }

    /// Post GameWinRecords
    pub async fn post_game_win_records(&self, game_win_records: &[GameWinRecord]) -> Result<(), Error> {
        let link = "/v1/stats/gamewinrecords";

        let bar = progress_bar(game_win_records.len() as u64, "Posting game win records".to_string());

        let body = game_win_records.chunks(5000);
        Ok(for chunk in body {
            self.make_request_with_body::<(), &[GameWinRecord]>(Method::POST, link, Some(chunk))
                .await?;
            bar.inc(chunk.len() as u64);
        })
    }

    /// Post MatchWinRecords
    pub async fn post_match_win_records(&self, match_win_records: &[MatchWinRecord]) -> Result<(), Error> {
        let link = "/v1/stats/matchwinrecords";

        let bar = progress_bar(match_win_records.len() as u64, "Posting match win records".to_string());

        let body = match_win_records.chunks(5000);
        Ok(for chunk in body {
            self.make_request_with_body::<(), &[MatchWinRecord]>(Method::POST, link, Some(chunk))
                .await?;
            bar.inc(chunk.len() as u64);
        })
    }

    /// Delete all stats
    pub async fn delete_all_stats(&self) -> Result<(), Error> {
        let link = "/v1/stats";

        self.make_request::<()>(Method::DELETE, link).await
    }
}

#[cfg(test)]
mod api_client_tests {
    use std::time::Duration;

    use async_once_cell::OnceCell;
    use chrono::{FixedOffset, Utc};
    use httpmock::prelude::*;
    use serde_json::json;

    use crate::{
        api::{
            api_structs::{
                BaseStats, GameWinRecord, MatchRatingStats, MatchWinRecord, PlayerMatchStats, RatingAdjustment
            },
            OtrApiClient
        },
        model::structures::{match_type::MatchType, ruleset::Ruleset}
    };

    static API_INSTANCE: OnceCell<OtrApiClient> = OnceCell::new();

    macro_rules! manually_refresh_token {
        ($api:expr) => {{
            let mut lock = $api.body.token.write().await;
            lock.refresh_token(&$api.body.api_root, &$api.body.client)
                .await
                .unwrap();
            let token = lock.token.clone();
            drop(lock);

            token
        }};
    }

    // Helper function that ensures OtrApi is not constructed
    // each time individual tests run
    async fn get_api() -> &'static OtrApiClient {
        API_INSTANCE
            .get_or_init(async {
                dotenv::dotenv().unwrap();

                OtrApiClient::new_from_env().await.expect("Failed to initialize OtrApi")
            })
            .await
    }

    #[tokio::test]
    async fn test_api_client_login() {
        let _api = get_api().await;
    }

    #[tokio::test]
    async fn test_api_client_get_players() {
        let api = get_api().await;

        let result = api.get_players().await.unwrap();

        assert!(!result.is_empty())
    }
    #[tokio::test]
    async fn test_api_client_post_rating_adjustments() {
        let api = get_api().await;

        let payload = vec![RatingAdjustment {
            player_id: 440,
            mode: Ruleset::Osu,
            rating_adjustment_amount: 3.123,
            volatility_adjustment_amount: 2.123,
            rating_before: 1000.0,
            rating_after: 1003.123,
            volatility_before: 100.0,
            volatility_after: 102.123,
            rating_adjustment_type: 0,
            timestamp: Utc::now().with_timezone(&FixedOffset::east_opt(0).unwrap())
        }];

        api.post_adjustments(&payload)
            .await
            .expect("Failed to POST adjustments");
    }

    #[tokio::test]
    async fn test_api_client_post_player_match_stats() {
        let api = get_api().await;

        let payload = vec![PlayerMatchStats {
            player_id: 440,
            match_id: 1,
            won: true,
            average_score: 502013.15,
            average_misses: 3.2,
            average_accuracy: 97.32,
            average_placement: 2.1,
            games_won: 5,
            games_lost: 3,
            games_played: 6,
            teammate_ids: vec![6666],
            opponent_ids: vec![334]
        }];

        api.post_player_match_stats(&payload)
            .await
            .expect("Failed to POST player match stats");
    }

    #[tokio::test]
    async fn test_api_client_post_match_rating_stats() {
        let api = get_api().await;

        let payload = vec![MatchRatingStats {
            match_id: 1,
            match_cost: 1.754,
            rating_before: 1270.3,
            rating_after: 1302.7,
            rating_change: 1302.7 - 1270.3,
            volatility_before: 104.23,
            volatility_after: 98.2,
            volatility_change: 104.23 - 98.2,
            global_rank_before: 743,
            global_rank_after: 730,
            global_rank_change: -13,
            country_rank_before: 30,
            country_rank_after: 20,
            country_rank_change: -10,
            percentile_before: 93.0,
            percentile_after: 93.6,
            percentile_change: 0.6,
            average_teammate_rating: Some(1125.4),
            player_id: 440,
            average_opponent_rating: Some(1420.5)
        }];

        api.post_match_rating_stats(&payload)
            .await
            .expect("Failed to POST match rating stats");
    }

    #[tokio::test]
    async fn test_api_client_post_base_stats() {
        let api = get_api().await;

        let payload = vec![BaseStats {
            player_id: 440,
            mode: Ruleset::Osu,
            rating: 1302.7,
            volatility: 98.2,
            global_rank: 730,
            country_rank: 20,
            percentile: 93.6,
            match_cost_average: 1.375
        }];

        api.post_base_stats(&payload).await.expect("Failed to POST base stats");
    }

    #[tokio::test]
    async fn test_api_client_get_matches() {
        let api = get_api().await;

        let result = api.get_matches(1, 5).await.unwrap();

        assert_eq!(result.count as usize, result.results.len())
    }

    #[tokio::test]
    async fn test_api_client_post_game_win_records() {
        let api = get_api().await;

        let result = api.get_matches(1, 5).await.unwrap();
        let payload = vec![GameWinRecord {
            game_id: 450905,
            winners: vec![440],
            losers: vec![6666],
            winner_team: 1,
            loser_team: 2
        }];

        api.post_game_win_records(&payload)
            .await
            .expect("Failed to POST game win records");
    }

    #[tokio::test]
    async fn test_api_client_post_match_win_records() {
        let api = get_api().await;

        let payload = vec![MatchWinRecord {
            match_id: 57243,
            loser_roster: vec![440],
            winner_roster: vec![6666],
            loser_points: 0,
            winner_points: 6,
            winner_team: Some(2),
            loser_team: Some(1),
            match_type: Some(MatchType::Team) // TeamVS
        }];

        api.post_match_win_records(&payload)
            .await
            .expect("Failed to POST match win records");
    }

    // DANGEROUS
    // #[tokio::test]
    // async fn test_api_client_delete_all_stats() {
    //     let api = get_api().await;
    //
    //     api.delete_all_stats().await.expect("Failed to DELETE all stats");
    // }

    // Manually refresh token three times
    #[tokio::test]
    async fn test_refresh_token() {
        let api = get_api().await;

        let lock = api.body.token.read().await;
        let initial_token = lock.token.clone();
        drop(lock);

        let first_token = manually_refresh_token!(api);
        let second_token = manually_refresh_token!(api);
        let third_token = manually_refresh_token!(api);

        assert_ne!(initial_token, first_token);
        assert_ne!(first_token, second_token);
        assert_ne!(second_token, third_token);
    }

    #[tokio::test]
    async fn test_login_mocked() {
        let server = MockServer::start();

        let login = server.mock(|when, then| {
            when.path("/v1/oauth/token");
            then.status(200)
                .json_body(json!({ "accessToken": "123", "refreshToken": "321", "accessExpiration": 1111 }));
        });

        let api = OtrApiClient::new(&format!("http://127.0.0.1:{}", server.port()), "123", "321")
            .await
            .expect("Failed to initialize OtrApi");

        login.assert();

        let lock = api.body.token.read().await;

        assert_eq!(lock.token, "Bearer 123");
        assert_eq!(lock.refresh_token, "321");
        assert_eq!(lock.expire_in, 1111);
    }

    #[tokio::test]
    async fn test_login_refresh_worker() {
        let server = MockServer::start();

        let login = server.mock(|when, then| {
            when.path("/v1/oauth/token");
            then.status(200)
                .json_body(json!({ "accessToken": "old_token", "refreshToken": "123", "accessExpiration": 2 }));
        });

        let api = OtrApiClient::new(&format!("http://127.0.0.1:{}", server.port()), "123", "321")
            .await
            .expect("Failed to initialize OtrApi");

        login.assert();

        let refresh = server.mock(|when, then| {
            when.path("/v1/oauth/refresh").query_param_exists("refreshToken");
            then.status(200)
                .json_body(json!({ "accessToken": "new_token", "refreshToken": "another", "accessExpiration": 1000 }));
        });

        tokio::time::sleep(Duration::from_secs(3)).await;

        refresh.assert();

        let lock = api.body.token.read().await;
        assert_eq!(lock.token, "Bearer new_token");
        assert_eq!(lock.refresh_token, "another");
        assert_eq!(lock.expire_in, 1000);
    }

    #[tokio::test]
    async fn test_login_refresh_worker_hits() {
        let server = MockServer::start();

        let login = server.mock(|when, then| {
            when.path("/v1/oauth/token");
            then.status(200)
                .json_body(json!({ "accessToken": "old_token", "refreshToken": "123", "accessExpiration": 0 }));
        });

        let api = OtrApiClient::new(&format!("http://127.0.0.1:{}", server.port()), "123", "321")
            .await
            .expect("Failed to initialize OtrApi");

        login.assert();

        let refresh = server.mock(|when, then| {
            when.path("/v1/oauth/refresh").query_param_exists("refreshToken");
            then.status(200)
                .json_body(json!({ "accessToken": "new_token", "refreshToken": "another", "accessExpiration": 1 }));
        });

        tokio::time::sleep(Duration::from_secs(3)).await;

        refresh.assert_hits(3);
    }
}
