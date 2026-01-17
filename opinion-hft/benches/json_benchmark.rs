use criterion::{black_box, criterion_group, criterion_main, Criterion};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
struct OrderUpdate {
    order_id: String,
    status: String,
    shares: Option<String>,
    filled_shares: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct TradeRecord {
    order_id: String,
    tx_hash: Option<String>,
    amount: Option<String>,
}

fn bench_serde_json(c: &mut Criterion) {
    let mut group = c.benchmark_group("serde_json");

    let order_update_json = r#"{"order_id":"12345","status":"Finish","shares":"1000000000000000000","filled_shares":"1000000000000000000"}"#;
    let trade_record_json = r#"{"order_id":"12345","tx_hash":"0x1234567890abcdef","amount":"1000000000000000000"}"#;
    let market_data_json = r#"{"channel":"market.data.BTC-YES","data":{"bids":[["0.700","1000000000000000000"],["0.695","2000000000000000000"]],"asks":[["0.710","1000000000000000000"]]}}"#;

    group.bench_function("order_update_small", |b| {
        b.iter(|| {
            let _: OrderUpdate = serde_json::from_str(black_box(order_update_json)).unwrap();
        });
    });

    group.bench_function("trade_record_small", |b| {
        b.iter(|| {
            let _: TradeRecord = serde_json::from_str(black_box(trade_record_json)).unwrap();
        });
    });

    group.bench_function("market_data_medium", |b| {
        b.iter(|| {
            let val: serde_json::Value = serde_json::from_str(black_box(market_data_json)).unwrap();
            black_box(val);
        });
    });

    group.finish();
}

fn bench_simd_json(c: &mut Criterion) {
    let mut group = c.benchmark_group("simd_json");

    let order_update_json = r#"{"order_id":"12345","status":"Finish","shares":"1000000000000000000","filled_shares":"1000000000000000000"}"#;
    let trade_record_json = r#"{"order_id":"12345","tx_hash":"0x1234567890abcdef","amount":"1000000000000000000"}"#;
    let market_data_json = r#"{"channel":"market.data.BTC-YES","data":{"bids":[["0.700","1000000000000000000"],["0.695","2000000000000000000"]],"asks":[["0.710","1000000000000000000"]]}}"#;

    group.bench_function("order_update_small", |b| {
        b.iter(|| {
            let mut buffer = black_box(order_update_json).as_bytes().to_vec();
            let _: OrderUpdate = simd_json::from_slice(&mut buffer).unwrap();
        });
    });

    group.bench_function("trade_record_small", |b| {
        b.iter(|| {
            let mut buffer = black_box(trade_record_json).as_bytes().to_vec();
            let _: TradeRecord = simd_json::from_slice(&mut buffer).unwrap();
        });
    });

    group.bench_function("market_data_medium", |b| {
        b.iter(|| {
            let mut buffer = black_box(market_data_json).as_bytes().to_vec();
            let val: simd_json::OwnedValue = simd_json::to_owned_value(&mut buffer).unwrap();
            black_box(val);
        });
    });

    group.finish();
}

criterion_group!(benches, bench_serde_json, bench_simd_json);
criterion_main!(benches);
