use criterion::{criterion_group, criterion_main, Criterion};
use opinion_hft::config::{ApiConfig, Environment};
use opinion_hft::data::rest::OpinionClient;
use std::time::Duration;

fn bench_client_creation(c: &mut Criterion) {
    let config = ApiConfig::default();

    c.bench_function("create_client_local", |b| {
        b.iter(|| {
            OpinionClient::new(&config, Environment::Local).unwrap()
        })
    });

    c.bench_function("create_client_aws", |b| {
        b.iter(|| {
            OpinionClient::new(&config, Environment::Aws).unwrap()
        })
    });
}

criterion_group!(benches, bench_client_creation);
criterion_main!(benches);
