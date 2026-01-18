use criterion::{criterion_group, criterion_main, Criterion};

fn signer_benchmark(_c: &mut Criterion) {}

criterion_group!(benches, signer_benchmark);
criterion_main!(benches);
