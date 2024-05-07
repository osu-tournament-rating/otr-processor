use otr_processor::{
    api,
    model::{self, hash_country_mappings, structures::processing::RatingCalculationResult}
};

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

    let mut result = model::calculate_ratings(ratings, &matches, &plackett_luce);

    // Print top 100 players
    result
        .base_ratings
        .sort_by(|a, b| b.rating.mu.partial_cmp(&a.rating.mu).unwrap());

    println!("top 100");
    for (i, player) in result.base_ratings.iter().take(100).enumerate() {
        println!(
            "{}: {} - {} (mode: {:?})",
            i + 1,
            player.player_id,
            player.rating,
            player.mode
        );
    }

    println!("{:?}", result.game_win_records.first());
    println!("{:?}", result.match_win_records.first());
    println!("Total object size: {:?}", std::mem::size_of_val(&result.game_win_records));
}
