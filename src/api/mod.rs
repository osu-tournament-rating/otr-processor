pub mod api_structs;

use crate::api::api_structs::{LoginResponse, Match, MatchIdMapping, Player};
use reqwest::{
    header::{AUTHORIZATION, CONTENT_TYPE},
    Client, ClientBuilder, Error, Method
};
use serde::{de::DeserializeOwned, Serialize};

pub struct OtrApiClient {
    client: Client,
    token: String,
    api_root: String
}

impl OtrApiClient {
    /// Constructs API client based on provided token
    pub async fn new(priv_secret: &str, api_root: &str) -> Result<Self, Error> {
        let client = ClientBuilder::new().build()?;

        let token_response = Self::login(&client, priv_secret, api_root).await?;

        Ok(Self {
            client,
            token: token_response.token,
            api_root: api_root.to_owned()
        })
    }

    /// Constructs API client based environment variables
    /// see `env_example` in project directory
    ///
    /// # Note
    /// Method logs in as system user so it's expecting
    /// privileged token in environment variables
    pub async fn new_from_priv_env() -> Result<Self, Error> {
        OtrApiClient::new(
            &std::env::var("PRIVILEGED_SECRET").unwrap(),
            &std::env::var("API_ROOT").unwrap()
        )
        .await
    }

    /// Initial login request to fetch token
    pub async fn login(client: &Client, priv_secret: &str, api_root: &str) -> Result<LoginResponse, Error> {
        let link = format!("{}/login/system", api_root);

        let response = client
            .post(link)
            .header(AUTHORIZATION, priv_secret)
            .header(CONTENT_TYPE, "application/json")
            .send()
            .await?
            .json()
            .await?;

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
        let request_link = format!("{}{}", self.api_root, partial_url);

        let mut request = match method {
            Method::GET => self.client.get(request_link),
            Method::POST => self.client.post(request_link),
            _ => unimplemented!()
        };

        if let Some(body) = body {
            request = request.json(&body)
        }

        request
            .header(AUTHORIZATION, &self.token)
            .header(CONTENT_TYPE, "application/json")
            .send()
            .await?
            .json()
            .await
    }

    /// Get ids of matches
    pub async fn get_match_ids(&self, limit: Option<u32>) -> Result<Vec<u32>, Error> {
        let limit = limit.unwrap_or(0);
        let link = "/matches/ids";

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
        let link = "/matches/convert";

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
        let link = "/matches/id-mapping";

        self.make_request(Method::GET, link).await
    }

    // Get list of players
    pub async fn get_players(&self) -> Result<Vec<Player>, Error> {
        let link = "/players/ranks/all";

        self.make_request(Method::GET, link).await
    }
}

#[cfg(test)]
mod api_client_tests {
    use async_once_cell::OnceCell;

    use crate::api::OtrApiClient;

    static API_INSTANCE: OnceCell<OtrApiClient> = OnceCell::new();

    // Helper function that ensures OtrApi is not constructed
    // each time individual tests run
    async fn get_api() -> &'static OtrApiClient {
        API_INSTANCE
            .get_or_init(async {
                dotenv::dotenv().unwrap();

                OtrApiClient::new_from_priv_env()
                    .await
                    .expect("Failed to initialize OtrApi")
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
}
