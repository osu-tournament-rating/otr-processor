use otr_processor::{
    api,
    model::{self, hash_country_mappings, structures::processing::RatingCalculationResult}
};
use otr_processor::api::OtrApiClient;

#[tokio::main]
async fn main() {
    dotenv::dotenv().unwrap();

    println!("Getting otr client");
    let api = api::OtrApiClient::new_from_env().await.unwrap();

    println!("Gettings match ids");
    let match_ids = api
        .get_match_ids(None)
        .await
        .expect("Match ids must be valid before proceeding");

    println!("Getting matches");
    let matches = api
        .get_matches(&match_ids, 250)
        .await
        .expect("Matches need to be loaded before continuing");

    println!("Getting players");
    let players = api.get_players().await.expect("Ranks must be identified");
    let country_mappings = api
        .get_player_country_mapping()
        .await
        .expect("Country mappings must be identified");

    // let worst = players.iter().find(|x| x.id == 6666).unwrap();

    // Model
    let plackett_luce = model::create_model();
    let country_hash = hash_country_mappings(&country_mappings);
    let mut ratings = model::create_initial_ratings(&matches, &players);

    // let mut counter = HashMap::new();
    //
    // for rating in &ratings {
    // counter.entry(rating.rating.mu as u32).and_modify(|x| *x += 1).or_insert(1);
    // }

    // dbg!(counter);

    // Filling PlayerRating with their country
    for player_rating in ratings.iter_mut() {
        if let Some(Some(country)) = country_hash.get(&player_rating.player_id) {
            if player_rating.country.is_empty() {
                player_rating.country.push_str(country)
            } else {
                panic!("WTF!@#$!@");
            }
        }
    }

    let result = model::calculate_ratings(ratings, &matches, &plackett_luce);
    upload_stats(&result).await;

    println!(":steamhappy:")
}

async fn upload_stats(result: &RatingCalculationResult) {
    let client = OtrApiClient::new_from_env().await.unwrap();

    // Delete stats
    client.delete_all_stats().await.unwrap();

    // Post all stats
    client.post_base_stats(&result.base_stats).await.unwrap();
    client.post_adjustments(&result.adjustments).await.unwrap();
    client.post_player_match_stats(&result.player_match_stats).await.unwrap();
    client.post_match_rating_stats(&result.rating_stats).await.unwrap();
    client.post_game_win_records(&result.game_win_records).await.unwrap();
    client.post_match_win_records(&result.match_win_records).await.unwrap();
}
