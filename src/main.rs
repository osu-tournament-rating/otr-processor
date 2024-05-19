use otr_processor::{
    api::{api_structs::Match, OtrApiClient},
    model::{
        self, hash_country_mappings,
        structures::{
            match_verification_status::MatchVerificationStatus::Verified, processing::RatingCalculationResult
        }
    },
    utils::progress_utils::indeterminate_bar
};

#[tokio::main]
async fn main() {
    dotenv::dotenv().unwrap();

    println!("Getting otr client");
    let api = OtrApiClient::new_from_env().await.unwrap();

    println!("Getting matches");
    let matches = get_all_matches().await;

    println!("Getting players");
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
                panic!("Player has no country");
            }
        }
    }

    let result = model::calculate_ratings(ratings, &matches, &plackett_luce);
    upload_stats(&result).await;

    println!("Processing complete!")
}

async fn upload_stats(result: &RatingCalculationResult) {
    let client = OtrApiClient::new_from_env().await.unwrap();

    // Delete stats
    client.delete_all_stats().await.unwrap();

    // Post all stats
    client.post_base_stats(&result.base_stats).await.unwrap();
    client.post_adjustments(&result.adjustments).await.unwrap();
    client
        .post_player_match_stats(&result.player_match_stats)
        .await
        .unwrap();
    client.post_match_rating_stats(&result.rating_stats).await.unwrap();
    client.post_game_win_records(&result.game_win_records).await.unwrap();
    client.post_match_win_records(&result.match_win_records).await.unwrap();
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

        bar.set_message(format!("[{}] Fetched {} matches from page {}", total, result.count, page));

        bar.inc(1);
        total += result.count as usize;
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
