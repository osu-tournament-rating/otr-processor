use reqwest::{Client, ClientBuilder, Error};
use reqwest::header::{HeaderMap, HeaderValue};
use serde::{Deserialize};

use crate::env;

#[derive(Deserialize)]
#[derive(Debug)]
pub struct LoginResponse {
    token: String
}

fn client() -> Client {
    ClientBuilder::new()
        .default_headers(auth_headers())
        .build()
        .expect("Valid client configuration")
}

fn auth_headers() -> HeaderMap {
    let env = env::get_env(); // Assuming get_env is defined and returns EnvironmentVariables
    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", HeaderValue::from_static("application/json"));

    // Use `from_str` and handle the Result it returns
    let auth_value = HeaderValue::from_str(&env.privileged_secret)
        .expect("Invalid header value for Authorization");

    headers.insert("Authorization", auth_value);

    headers
}


pub async fn login() -> Result<LoginResponse, Error> {
    let client = client();

    let env = env::get_env();
    let response: LoginResponse = client
        .post(format!("{}/login/system", env.api_root))
        .send()
        .await? // Propagate the error if .send() fails
        .json()
        .await?;

    println!("{:#?}", response);

    Ok(response) // Return Ok wrapping the response
}

