#[macro_use]
extern crate lazy_static;

mod api;
mod env;
mod model;
mod utils;

use indicatif::ProgressBar;

use crate::model::{match_costs, structures::match_cost::MatchCost};

#[tokio::main]
async fn main() {
    dotenv::dotenv().unwrap();

    let api = api::OtrApiClient::new_from_env()
        .await
        .expect("Failed to intialize otr api");

    let match_ids = api
        .get_match_ids(None)
        .await
        .expect("Match ids must be valid before proceeding");

    let matches = api
        .get_matches(&match_ids, 250)
        .await
        .expect("Matches need to be loaded before continuing");

    let players = api.get_players().await.expect("Ranks must be identified");
    let country_mappings = api
        .get_player_country_mapping()
        .await
        .expect("Country mappings must be identified");

    // Model
    let plackett_luce = model::create_model();
    let ratings = model::create_initial_ratings(&matches, &players);
    let result = model::calc_ratings(&ratings, &country_mappings, &matches, &plackett_luce);

    println!("{:?} ratings processed", result.base_ratings.len());
    // println!("{:?}", mcs)
}
