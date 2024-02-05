mod api_structs;

use std::any::Any;
use indicatif::ProgressBar;
use reqwest::{Client, ClientBuilder, Error};
use reqwest::header::{HeaderMap, HeaderValue};
use serde::{Deserialize};
use crate::api::api_structs::MatchDTO;
use crate::utils::progress_utils::progress_bar;

use crate::env;

#[derive(Deserialize)]
#[derive(Debug)]
pub struct LoginResponse {
    pub token: String
}

fn client(headers: Option<HeaderMap>) -> Client {
    let valid_headers = headers.unwrap_or(privileged_headers());

    ClientBuilder::new()
        .default_headers(valid_headers)
        .build()
        .expect("Valid client configuration")
}

fn privileged_headers() -> HeaderMap {
    let env = env::get_env(); // Assuming get_env is defined and returns EnvironmentVariables
    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", HeaderValue::from_static("application/json"));

    // Use `from_str` and handle the Result it returns
    let auth_value = HeaderValue::from_str(&env.privileged_secret)
        .expect("Authorization header should be valid.");

    headers.insert("Authorization", auth_value);

    headers
}

fn authorized_headers(token: &String) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", HeaderValue::from_static("application/json"));

    // Use `from_str` and handle the Result it returns
    let auth_value = HeaderValue::from_str(&token)
        .expect("Authorization header should be valid.");

    headers.insert("Authorization", auth_value);

    headers
}

pub async fn login() -> Result<LoginResponse, Error> {
    let client = client(None);

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

pub async fn get_match_ids(limit: Option<i32>, token: &String) -> Result<Vec<i32>, Error> {
    let size = limit.unwrap_or(0);

    let client = client(Some(authorized_headers(&token)));
    let env = env::get_env();
    let response: Vec<i32> = client
        .get(format!("{}/matches/ids", env.api_root))
        .send()
        .await?
        .json()
        .await?;

    println!("{:#?}", response);

    if size == 0 {
        return Ok(response);
    }

    let take = response.into_iter().take(size as usize).collect();
    Ok(take)
}

pub async fn get_matches(match_ids: Vec<i32>, token: &String) -> Result<Vec<MatchDTO>, Error> {
    let chunk_size = 250;
    let pbar_size = (match_ids.len() / chunk_size) as u64;
    let client = client(Some(authorized_headers(&token)));
    let env = env::get_env();
    let mut match_data: Vec<MatchDTO> = Vec::new();

    let bar = progress_bar(pbar_size);
    bar.println("Fetching match data...");

    // Group matches into 250 different lists, then form
    // ret with all values. This is to reduce API strain.
    for chunk in match_ids.chunks(chunk_size) {
        let response: Vec<MatchDTO> = client
            .post(format!("{}/matches/convert", env.api_root))
            .json(&chunk)
            .send()
            .await?
            .json()
            .await?;

        match_data.extend(response);
        bar.inc(1);
    }

    bar.finish();
    Ok(match_data)
}

