pub mod api_structs;

use std::{sync::Arc, time::Duration};

use crate::api::api_structs::{Match, MatchIdMapping, OAuthResponse, Player};
use reqwest::{
    header::{AUTHORIZATION, CONTENT_TYPE},
    Client, ClientBuilder, Error, Method
};
use serde::{de::DeserializeOwned, Serialize};
use tokio::sync::{
    oneshot::{Receiver, Sender},
    Mutex
};

/// A loop that automatically refreshes token
pub async fn refresh_token_loop(api: Arc<OtrApiBody>) {
    loop {
        // The first iteration assumes that token it's already valid
        // and doesn't need to be refreshed
        // so it's sleeps until expire time comes
        let lock = api.token.lock().await;
        let expire_in = lock.expire_in;
        drop(lock);

        // Refreshes in (3600 - 300) secs just in case
        tokio::time::sleep(Duration::from_secs(expire_in - 300)).await;

        let mut lock = api.token.lock().await;

        // Another loop to ensure token is actually updates
        // (without any errors)
        // If error anyway happened then it's going to retry
        // until successed
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
    /// Function that refresh token when called
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

    /// Wrapped in Mutex because we need
    /// a shared mutable access
    token: Mutex<OtrToken>
}

pub struct OtrApiClient {
    /// Wrapped in [`Arc`] because everything located in [`OtrApiBody`]
    /// need to be accessed in diffrent threads (shared reference)
    body: Arc<OtrApiBody>,

    /// Channel that's get sended on Drop to shutdown refresh token worker
    /// Why it's wrapped in `[Option]`? Because oneshot sender
    /// consumes itself on send.
    ///
    /// Detailed explanation:
    /// In Drop trait implementation we having a mutable reference
    /// on our struct (where sender is located), so when `send()`
    /// happens it consumes itself, but we still having that mutable
    /// reference and because of this we getting a compile error
    /// variabled getting moved.
    /// Workaround is pretty simple:
    /// Wrap [`Sender`] inside Option, so we can use [`std::mem::take`]
    /// to replace our sender with default value (in our case in None)
    /// and peacefully let sender consume itself :)
    refresh_tx: Option<Sender<()>>
}

impl Drop for OtrApiClient {
    fn drop(&mut self) {
        if let Some(tx) = std::mem::take(&mut self.refresh_tx) {
            // Dropping error because either `Ok` or `Err` indicates
            // that worker and loop is stoped
            // Ok - means channel is read and loop is stopped
            // Err - means Receiver was somehow dropped beforehand
            // which means that worker is not running under any
            // circumstances
            let _ = tx.send(());
        }
    }
}

impl OtrApiClient {
    /// Constructs API client based on provided token
    pub async fn new(api_root: &str, client_id: &str, client_secret: &str) -> Result<Self, Error> {
        let client = ClientBuilder::new().build()?;

        let token_response = Self::login(&client, api_root, client_id, client_secret).await?;

        let token = OtrToken {
            token: token_response.token,
            refresh_token: token_response.refresh_token,
            expire_in: token_response.expire_in
        };

        let body = Arc::new(OtrApiBody {
            client,
            api_root: api_root.to_owned(),
            token: Mutex::new(token)
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

        let mut response: OAuthResponse = client
            .post(link)
            .header(CONTENT_TYPE, "application/json")
            .send()
            .await?
            .json()
            .await?;

        // Putting `Bearer` just to save allocations
        // on every request made
        response.token.insert_str(0, "Bearer ");

        Ok(response)
    }

    /// Wrapper to make authorized requests without body
    ///
    /// See [OtrApiClient::make_request_with_body]
    ///
    /// # Examples
    /// 1. Fetch some endpoint
    /// ```
    /// let api = OtrApiClient::new("MYSECRET", "example.com/api");
    /// api.make_request(Method::GET, "/fetch_something");
    /// ```
    async fn make_request<T>(&self, method: Method, partial_url: &str) -> Result<T, Error>
    where
        T: DeserializeOwned
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
    /// let api = OtrApiClient::new("MYSECRET", "example.com/api");
    /// let my_numbers: Vec<32> = vec![1, 2, 3, 4, 5];
    /// api.make_request_with_body(Method::GET, "/fetch_something", Some(&my_numbers));
    /// ```
    async fn make_request_with_body<T, B>(&self, method: Method, partial_url: &str, body: Option<B>) -> Result<T, Error>
    where
        T: DeserializeOwned,
        B: Serialize
    {
        let request_link = format!("{}{}", self.body.api_root, partial_url);

        let mut request = match method {
            Method::GET => self.body.client.get(request_link),
            Method::POST => self.body.client.post(request_link),
            _ => unimplemented!()
        };

        if let Some(body) = body {
            request = request.json(&body)
        }

        let lock = &self.body.token.lock().await;

        request
            .header(AUTHORIZATION, &lock.token)
            .header(CONTENT_TYPE, "application/json")
            .send()
            .await?
            .json()
            .await
    }

    /// Get ids of matches
    pub async fn get_match_ids(&self, limit: Option<u32>) -> Result<Vec<u32>, Error> {
        let limit = limit.unwrap_or(0);
        let link = "/v1/matches/ids";

        let response = self.make_request(Method::GET, link).await?;

        if limit == 0 {
            return Ok(response);
        }

        let limited_response = response.into_iter().take(limit as usize).collect();

        Ok(limited_response)
    }

    /// Get matches based on provided list of match id's
    /// # Arguments
    /// * `match_ids` - valid id's of matches
    /// * `chunk_size` - amount of matches that is going to be fetched
    /// in one request. Done to reduce strain on API side. Recommended
    /// value is `250`
    pub async fn get_matches(&self, match_ids: &[u32], chunk_size: usize) -> Result<Vec<Match>, Error> {
        let link = "/v1/matches/convert";

        let mut data: Vec<Match> = Vec::new();

        for chunk in match_ids.chunks(chunk_size) {
            let response: Vec<Match> = self.make_request_with_body(Method::POST, link, Some(chunk)).await?;

            data.extend(response)
        }

        Ok(data)
    }

    /// Get list of match id mappings
    /// otr_match_id <-> osu_match_id
    pub async fn get_match_id_mapping(&self) -> Result<Vec<MatchIdMapping>, Error> {
        let link = "/v1/matches/id-mapping";

        self.make_request(Method::GET, link).await
    }

    // Get list of players
    pub async fn get_players(&self) -> Result<Vec<Player>, Error> {
        let link = "/v1/players/ranks/all";

        self.make_request(Method::GET, link).await
    }
}

#[cfg(test)]
mod api_client_tests {
    use std::time::Duration;

    use async_once_cell::OnceCell;

    use crate::api::OtrApiClient;

    static API_INSTANCE: OnceCell<OtrApiClient> = OnceCell::new();

    macro_rules! manually_refresh_token {
        ($api:expr) => {{
            let mut lock = $api.body.token.lock().await;
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
    async fn test_api_client_get_match_ids() {
        let api = get_api().await;

        let result = api.get_match_ids(None).await.unwrap();

        assert!(!result.is_empty());

        let result = api.get_match_ids(Some(10)).await.unwrap();

        assert!(result.len() == 10);
    }

    #[tokio::test]
    async fn test_api_client_get_players() {
        let api = get_api().await;

        let result = api.get_players().await.unwrap();

        assert!(!result.is_empty())
    }

    #[tokio::test]
    async fn test_api_client_get_matches() {
        let api = get_api().await;

        let match_ids = api.get_match_ids(Some(10)).await.unwrap();

        assert!(match_ids.len() == 10);

        let result = api.get_matches(&match_ids, 250).await.unwrap();

        assert!(result.len() == match_ids.len())
    }

    #[tokio::test]
    async fn test_api_get_match_id_mapping() {
        let api = get_api().await;

        let result = api.get_match_id_mapping().await.unwrap();

        assert!(!result.is_empty())
    }

    // Manually refresh token three times
    #[tokio::test]
    async fn test_refresh_token() {
        let api = get_api().await;

        let lock = api.body.token.lock().await;
        let initial_token = lock.token.clone();
        drop(lock);

        // Sleeps added here to avoid getting same token
        // because it runs too fast (maybe it's a bug?)
        let first_token = manually_refresh_token!(api);
        tokio::time::sleep(Duration::from_secs(1)).await;
        let second_token = manually_refresh_token!(api);
        tokio::time::sleep(Duration::from_secs(1)).await;
        let third_token = manually_refresh_token!(api);

        assert!(initial_token != first_token);
        assert!(first_token != second_token);
        assert!(second_token != third_token);
    }
}
