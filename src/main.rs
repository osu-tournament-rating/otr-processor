mod api;
mod env;
mod model;
mod utils;

use tokio;


#[tokio::main]
async fn main() {
    let login_res = api::login().await.expect("Login should be valid before proceeding");
    let match_ids = api::get_match_ids(None, &login_res.token).await.expect("Match ids must be valid before proceeding");
    let matches = api::get_matches(match_ids, &login_res.token).await;
}
