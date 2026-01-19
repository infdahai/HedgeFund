use criterion::{black_box, criterion_group, criterion_main, Criterion};
use opinion_hft::data::websocket::client::WebSocketClient;
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
    let order_update_json = r#"{"order_id":"12345","status":"Finish","shares":"1000000000000000000","filled_shares":"1000000000000000000"}"#;
    let trade_record_json =
        r#"{"order_id":"12345","tx_hash":"0x1234567890abcdef","amount":"1000000000000000000"}"#;

    let mut group = c.benchmark_group("json_parsing");

    group.bench_function("serde_json_order_update", |b| {
        b.iter(|| {
            let _: OrderUpdate = serde_json::from_str(black_box(order_update_json)).unwrap();
        });
    });

    group.bench_function("serde_json_trade_record", |b| {
        b.iter(|| {
            let _: TradeRecord = serde_json::from_str(black_box(trade_record_json)).unwrap();
        });
    });

    group.finish();
}

fn bench_simd_json(c: &mut Criterion) {
    let order_update_json = r#"{"order_id":"12345","status":"Finish","shares":"1000000000000000000","filled_shares":"1000000000000000000"}"#;
    let trade_record_json =
        r#"{"order_id":"12345","tx_hash":"0x1234567890abcdef","amount":"1000000000000000000"}"#;

    let mut group = c.benchmark_group("json_parsing");

    group.bench_function("simd_json_order_update", |b| {
        b.iter(|| {
            let mut buffer = black_box(order_update_json).as_bytes().to_vec();
            let _: OrderUpdate = simd_json::from_slice(&mut buffer).unwrap();
        });
    });

    group.bench_function("simd_json_trade_record", |b| {
        b.iter(|| {
            let mut buffer = black_box(trade_record_json).as_bytes().to_vec();
            let _: TradeRecord = simd_json::from_slice(&mut buffer).unwrap();
        });
    });

    group.finish();
}

fn bench_websocket_message(c: &mut Criterion) {
    let market_data_json = r#"{"channel":"market.data.BTC-YES","data":{"bids":[["0.700","1000000000000000000"]],"asks":[["0.800","1000000000000000000"]]}}"#;
    let order_update_json = r#"{"channel":"trade.order.update","data":{"order_id":"12345","status":"Finish","shares":"1000000000000000000","filled_shares":"1000000000000000000"}}""#;
    let trade_record_json = r#"{"channel":"trade.record.new","data":{"order_id":"12345","tx_hash":"0x1234567890abcdef","amount":"1000000000000000000"}}"#;

    let mut group = c.benchmark_group("websocket_messages");

    group.bench_function("market_data_serde", |b| {
        b.iter(|| {
            let event = WebSocketClient::parse_message(black_box(market_data_json));
            assert!(event.is_some());
        });
    });

    group.bench_function("order_update_serde", |b| {
        b.iter(|| {
            let event = WebSocketClient::parse_message(black_box(order_update_json));
            assert!(event.is_some());
        });
    });

    group.bench_function("trade_record_serde", |b| {
        b.iter(|| {
            let event = WebSocketClient::parse_message(black_box(trade_record_json));
            assert!(event.is_some());
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_serde_json,
    bench_simd_json,
    bench_websocket_message
);
criterion_main!(benches);
