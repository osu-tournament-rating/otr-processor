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

use crate::api::api_structs::OAuthResponse;

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
}

#[cfg(test)]
mod api_client_tests {
    use std::time::Duration;

    use async_once_cell::OnceCell;
    use httpmock::prelude::*;
    use serde_json::json;

    use crate::api::OtrApiClient;

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
    #[ignore]
    async fn test_api_client_login() {
        let _api = get_api().await;
    }

    #[tokio::test]
    #[ignore]
    async fn test_api_client_get_matches() {
        let api = get_api().await;

        let result = api.get_matches(1, 5).await.unwrap();

        assert_eq!(result.count as usize, result.results.len())
    }

    // Manually refresh token three times
    #[tokio::test]
    #[ignore]
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
    #[ignore]
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
    #[ignore]
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
