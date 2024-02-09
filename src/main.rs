mod api;
mod env;
mod model;
mod utils;

use tokio;
use crate::model::model::match_costs;
use crate::model::structures::match_cost::MatchCost;

#[tokio::main]
async fn main() {
    let login_res = api::login_async().await.expect("Login should be valid before proceeding");
    let match_ids = api::get_match_ids_async(None, &login_res.token).await.expect("Match ids must be valid before proceeding");
    let players = api::get_players_async(&login_res.token).await.expect("Ranks must be identified");
    // let match_mapping = api::get_match_id_mapping_async(&login_res.token).await.expect("Match id mapping should be valid before processing");
    let matches = api::get_matches_async(match_ids, &login_res.token).await.unwrap();

    // Model
    //let ratings = model::model::create_initial_ratings(matches, players);
    let mut mcs: Vec<Vec<MatchCost>> = Vec::new();
    for m in matches {
        let mc = match_costs(&m);

        match mc {
            Some(match_costs) => mcs.push(match_costs),
            None => continue
        }
    }

    println!("{:?}", mcs)
}
