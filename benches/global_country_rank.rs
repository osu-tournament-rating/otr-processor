use std::{
    collections::{HashMap, HashSet},
    fmt::Display
};

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use openskill::rating::Rating;
use rand::{
    distributions::{Alphanumeric, DistString},
    prelude::*
};

use otr_processor::model::{
    calculate_country_ranks, get_country_rank, get_global_rank,
    structures::{player_rating::PlayerRating, ruleset::Ruleset}
};

#[derive(Debug, Clone)]
struct TestInput {
    ratings: Vec<PlayerRating>,
    country_hash: HashMap<i32, Option<String>>
}

impl Display for TestInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Users: {}", self.ratings.len())
    }
}

pub fn calc_rankings_old(existing_ratings: &mut [PlayerRating], country_hash: &HashMap<i32, Option<String>>) {
    for player in existing_ratings.iter() {
        let _global_rank = get_global_rank(&player.rating.mu, &player.player_id, existing_ratings);

        let _country_rank = get_country_rank(&player.rating.mu, &player.player_id, country_hash, existing_ratings);
    }
}

pub fn criterion_benchmark(c: &mut Criterion) {
    const AMOUNT_OF_COUNTRIES: usize = 50;
    const AMOUNT_OF_USERS_IN_ONE_COUNTRY: usize = 10;

    let mut rng = SmallRng::seed_from_u64(727);
    let mut countries = HashSet::new();

    // Generate 50 random countries
    while countries.len() < AMOUNT_OF_COUNTRIES {
        let country = Alphanumeric.sample_string(&mut rng, 2);

        countries.insert(country);
    }

    // For each country generate 10 users
    let mut ratings = Vec::with_capacity(AMOUNT_OF_COUNTRIES * AMOUNT_OF_USERS_IN_ONE_COUNTRY);

    for country in countries {
        for _ in 0..AMOUNT_OF_USERS_IN_ONE_COUNTRY {
            ratings.push(PlayerRating {
                player_id: rng.gen_range(0..100_000), // Random player id
                ruleset: Ruleset::Osu,
                rating: Rating {
                    mu: rng.gen_range(0.0..2000.0),
                    sigma: 200.0
                },
                global_ranking: 0,
                country_ranking: 0,
                country: country.clone()
            })
        }
    }

    // Generate country mappings
    let mut country_hash: HashMap<i32, Option<String>> = HashMap::new();

    for player in ratings.iter() {
        country_hash.insert(player.player_id, Some(player.country.clone()));
    }

    let input = TestInput { ratings, country_hash };

    c.bench_with_input(BenchmarkId::new("calc_rankings_new", input.clone()), &input, |b, s| {
        let mut input = input.clone();
        b.iter(|| calculate_country_ranks(&mut input.ratings, Ruleset::Osu));
    });

    c.bench_with_input(BenchmarkId::new("calc_rankings_old", input.clone()), &input, |b, s| {
        let mut input = input.clone();
        b.iter(|| calc_rankings_old(&mut input.ratings, &input.country_hash));
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
