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

    let api = api::OtrApiClient::new_from_priv_env()
        .await
        .expect("Failed to intialize otr api");

    let match_ids = api
        .get_match_ids(Some(100))
        .await
        .expect("Match ids must be valid before proceeding");

    let matches = api
        .get_matches(&match_ids, 250)
        .await
        .expect("Matches need to be loaded before continuing");

    // let players = api.get_players().await.expect("Ranks must be identified");

    // Model
    // let ratings = model::model::create_initial_ratings(matches, players);

    let bar = ProgressBar::new(matches.len() as u64);

    let mut mcs: Vec<Vec<MatchCost>> = Vec::new();
    for m in matches {
        let mc = match_costs(&m.games);

        bar.inc(1);

        match mc {
            Some(match_costs) => mcs.push(match_costs),
            None => continue
        }
    }

    bar.finish();

    // println!("{:?}", mcs)
}
