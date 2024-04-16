use otr_processor::{
    api,
    model::{self, hash_country_mappings}
};

#[tokio::main]
async fn main() {
    dotenv::dotenv().unwrap();

    let api = api::OtrApiClient::new_from_env().await.unwrap();

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
    let country_hash = hash_country_mappings(&country_mappings);
    let mut ratings = model::create_initial_ratings(&matches, &players);

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

    // let mut copied_initial_ratings = ratings.clone();
    //
    // model::calculate_player_adjustments(&ratings, &copied_initial_ratings);

    // println!("{:?} ratings processed", result.base_ratings.len());
    // println!("{:?}", mcs);

    // Print top 100 players
    let mut sorted_ratings = result.base_ratings.clone();
    sorted_ratings.sort_by(|a, b| b.rating.mu.partial_cmp(&a.rating.mu).unwrap());

    for (i, player) in sorted_ratings.iter().take(100).enumerate() {
        println!("{}: {} - {}", i + 1, player.player_id, player.rating);
    }
}
