mod api;
mod env;
mod model;
mod utils;

use tokio;


#[tokio::main]
async fn main() {
    let login_res = api::login_async().await.expect("Login should be valid before proceeding");
   // let match_ids = api::get_match_ids_async(None, &login_res.token).await.expect("Match ids must be valid before proceeding");
    let players = api::get_players_async(&login_res.token).await.expect("Ranks must be identified");
    let match_mapping = api::get_match_id_mapping_async(&login_res.token).await.expect("Match id mapping should be valid before processing");
    println!("{:?}", match_mapping)
    //let matches = api::get_matches_async(match_ids, &login_res.token).await;
}
