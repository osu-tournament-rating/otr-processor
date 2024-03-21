use std::fmt::Display;

use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};
use openskill::rating::Rating;
use otr_processor::{api::api_structs::{Match, PlayerCountryMapping}, model::{match_costs, ranks_from_match_costs, structures::{player_rating::PlayerRating, mode::Mode}, create_model, hash_country_mappings, calc_ratings_v2, ProcessedMatchData, calc_post_match_info}};


fn match_from_json(json: &str) -> Match {
    serde_json::from_str(json).unwrap()
}

#[derive(Debug, Clone)]
struct TestInput {
    initial_ratings: Vec<PlayerRating>, 
    match_adjs: Vec<ProcessedMatchData>
}

impl Display for TestInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "None")
    }
}

pub fn criterion_benchmark(c: &mut Criterion) {
    let mut match_data = match_from_json(include_str!("../test_data/match_2v2.json"));

        match_data.start_time = Some(chrono::offset::Utc::now().fixed_offset());
        match_data.end_time = Some(chrono::offset::Utc::now().fixed_offset());

        let match_costs = match_costs(&match_data.games).unwrap();
        let ranks = ranks_from_match_costs(&match_costs);

        let player_ids = match_costs.iter().map(|mc| mc.player_id).collect::<Vec<i32>>();
        let mut initial_ratings = vec![];
        let mut country_mappings: Vec<PlayerCountryMapping> = vec![];

        let mut offset = 0.0;
        for id in player_ids {
            initial_ratings.push(PlayerRating {
                player_id: id,
                mode: Mode::Osu,
                rating: Rating {
                    mu: 1500.0 + offset,
                    sigma: 200.0
                },
                global_ranking: 0,
                country_ranking: 0,
                country: "US".to_string(),
            });
            country_mappings.push(PlayerCountryMapping {
                player_id: id,
                country: Some("US".to_string())
            });

            offset += 1.0;
        }

        let country_mappings_hash = hash_country_mappings(&country_mappings);
        let model = create_model();

        let match_adjs = calc_ratings_v2(&initial_ratings, &[match_data], &model);

        let input = TestInput {
            initial_ratings,
            match_adjs,
        };

        //model::calc_post_match_info(&mut ratings, &mut result);

    
        c.bench_with_input(BenchmarkId::new("calc_post_match", input.clone()), &input, |b, s| {
            let mut input = input.clone();
            b.iter(|| calc_post_match_info(&mut input.initial_ratings, &mut input.match_adjs));
        });
        /*

    c.bench_with_input(BenchmarkId::new("calc_rankings_old", input.clone()), &input, |b, s| {
        let mut input = input.clone();
        b.iter(|| calc_rankings_old(&mut input.ratings, &input.country_hash));
    });
    */
    
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
