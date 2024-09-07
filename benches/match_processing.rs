use criterion::{criterion_group, criterion_main, Criterion};
use itertools::Itertools;
use otr_processor::{
    model::otr_model::OtrModel,
    utils::test_utils::{generate_country_mapping_player_ratings, generate_default_initial_ratings, generate_matches}
};

fn process_matches(count_players: usize, count_matches: usize) {
    let initial_ratings = generate_default_initial_ratings(count_players.try_into().unwrap());
    let matches = generate_matches(
        count_matches.try_into().unwrap(),
        &initial_ratings.iter().map(|r| r.player_id).collect_vec()
    );
    let country_mapping = generate_country_mapping_player_ratings(initial_ratings.as_slice(), "US");

    let mut model = OtrModel::new(initial_ratings.as_slice(), &country_mapping);
    model.process(&matches);
}

fn group_call(c: &mut Criterion) {
    let mut group = c.benchmark_group("match-processing");
    group.sample_size(25);
    group.bench_function("process: p=10,m=10", |b| b.iter(|| process_matches(10, 10)));
    group.bench_function("process: p=20,m=20", |b| b.iter(|| process_matches(20, 20)));
    group.bench_function("process: p=30,m=30", |b| b.iter(|| process_matches(30, 30)));
    group.finish();
}

criterion_group!(benches, group_call);
criterion_main!(benches);
