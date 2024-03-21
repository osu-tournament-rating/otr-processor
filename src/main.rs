use otr_processor::{api, model::{self, hash_country_mappings}};

#[tokio::main]
async fn main() {
    dotenv::dotenv().unwrap();

    let api = api::OtrApiClient::new_from_env()
        .await
        .unwrap();


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
            if player_rating.country.len() == 0 {
                player_rating.country.push_str(&country)
            } else {
                panic!("WTF!@#$!@");
            }
        }
    }

    let mut result = model::calc_ratings_v2(&ratings, &matches, &plackett_luce);

    model::calc_post_match_info(&mut ratings, &mut result);

    //println!("{:?} ratings processed", result.base_ratings.len());
    //println!("{:?}", mcs);
}
