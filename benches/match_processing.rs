use criterion::{Criterion, criterion_group, criterion_main};
use otr_processor::model::otr_model::OtrModel;
use otr_processor::utils::test_utils::{generate_country_mapping, generate_default_initial_ratings, generate_matches};

fn process_matches(count_players: usize, count_matches: usize) {
    let initial_ratings = generate_default_initial_ratings(count_players.try_into().unwrap());
    let matches = generate_matches(count_matches.try_into().unwrap(), initial_ratings.as_slice());
    let country_mapping = generate_country_mapping(initial_ratings.as_slice(), "US");

    let mut model = OtrModel::new(initial_ratings.as_slice(), &country_mapping);
    model.process(&matches);
}

fn group_call(c: &mut Criterion) {
    let mut group = c.benchmark_group("match-processing");
    group.sample_size(25);
    group.bench_function("process: p=10,m=10", |b| b.iter(|| process_matches(10, 10)));
    group.bench_function("process: p=100,m=10", |b| b.iter(|| process_matches(100, 10)));
    group.bench_function("process: p=1000,m=10", |b| b.iter(|| process_matches(1000, 10)));
    group.bench_function("process: p=10,m=100", |b| b.iter(|| process_matches(10, 100)));
    group.bench_function("process: p=10,m=1000", |b| b.iter(|| process_matches(10, 1000)));
    group.bench_function("process: p=100,m=100", |b| b.iter(|| process_matches(100, 100)));
    group.bench_function("process: p=1000,m=1000", |b| b.iter(|| process_matches(1000, 1000)));
    group.finish();
}

criterion_group!(benches, group_call);
criterion_main!(benches);
