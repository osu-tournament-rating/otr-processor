use otr_processor::{
    api::{self, api_structs::Match, OtrApiClient},
    model::{self, hash_country_mappings, structures::match_verification_status::MatchVerificationStatus::Verified},
    utils::progress_utils::indeterminate_bar
};

#[tokio::main]
async fn main() {
    dotenv::dotenv().unwrap();

    println!("Getting otr client");
    let api = api::OtrApiClient::new_from_env().await.unwrap();

    println!("Getting matches");
    let matches = get_all_matches().await;

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
    println!(
        "Total object size: {:?}",
        std::mem::size_of_val(&result.game_win_records)
    );
}

/// Repeatedly calls the /matches GET endpoint and returns all matches
async fn get_all_matches() -> Vec<Match> {
    let bar = indeterminate_bar("Fetching matches".to_string());
    let mut matches = vec![];
    let client = OtrApiClient::new_from_env().await.unwrap();

    let chunk_size = 250;
    let mut total = chunk_size;
    for page in 1.. {
        let mut result = client.get_matches(page, chunk_size).await.unwrap();
        matches.append(&mut result.results);

        if result.next.is_none() {
            break;
        }

        bar.set_message(format!("[{}] Fetched {} matches from page {}", total, chunk_size, page));

        bar.inc(1);
        total += chunk_size;
    }

    bar.finish();

    if matches.is_empty() {
        panic!("Expected matches to be populated")
    }

    // Sort matches by start time
    matches.sort_by(|a, b| a.start_time.cmp(&b.start_time));

    // Remove all matches that are invalid
    matches.retain(|x| x.verification_status == Verified);

    println!("Retained {} verified matches", matches.len());

    matches
}
