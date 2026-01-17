use std::time::Instant;
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

fn main() {
    let order_update_json = r#"{"order_id":"12345","status":"Finish","shares":"1000000000000000000","filled_shares":"1000000000000000000"}"#;
    let trade_record_json = r#"{"order_id":"12345","tx_hash":"0x1234567890abcdef","amount":"1000000000000000000"}"#;
    let market_data_json = r#"{"channel":"market.data.BTC-YES","data":{"bids":[["0.700","1000000000000000000"],["0.695","2000000000000000000"]],"asks":[["0.710","1000000000000000000"]]}}"#;

    const ITERATIONS: usize = 10_000;

    println!("=== JSON Parsing Benchmark ({} iterations) ===\n", ITERATIONS);

    println!("Benchmark: serde_json::from_str (OrderUpdate)");
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        let _: OrderUpdate = serde_json::from_str(order_update_json).unwrap();
    }
    let serde_time = start.elapsed();
    println!("  Total: {:?}", serde_time);
    println!("  Avg per iteration: {:?}\n", serde_time / ITERATIONS as u32);

    println!("Benchmark: simd_json::from_slice (OrderUpdate)");
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        let mut buffer = order_update_json.as_bytes().to_vec();
        let _: OrderUpdate = simd_json::from_slice(&mut buffer).unwrap();
    }
    let simd_time = start.elapsed();
    println!("  Total: {:?}", simd_time);
    println!("  Avg per iteration: {:?}\n", simd_time / ITERATIONS as u32);

    let speedup = serde_time.as_nanos() as f64 / simd_time.as_nanos() as f64;
    println!("  Speedup: {:.2}x\n", speedup);

    println!("Benchmark: serde_json::from_str (TradeRecord)");
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        let _: TradeRecord = serde_json::from_str(trade_record_json).unwrap();
    }
    let serde_time = start.elapsed();
    println!("  Total: {:?}", serde_time);
    println!("  Avg per iteration: {:?}\n", serde_time / ITERATIONS as u32);

    println!("Benchmark: simd_json::from_slice (TradeRecord)");
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        let mut buffer = trade_record_json.as_bytes().to_vec();
        let _: TradeRecord = simd_json::from_slice(&mut buffer).unwrap();
    }
    let simd_time = start.elapsed();
    println!("  Total: {:?}", simd_time);
    println!("  Avg per iteration: {:?}\n", simd_time / ITERATIONS as u32);

    let speedup = serde_time.as_nanos() as f64 / simd_time.as_nanos() as f64;
    println!("  Speedup: {:.2}x\n", speedup);

    println!("Benchmark: serde_json::from_str (MarketData)");
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        let val: serde_json::Value = serde_json::from_str(market_data_json).unwrap();
        let _ = val;
    }
    let serde_time = start.elapsed();
    println!("  Total: {:?}", serde_time);
    println!("  Avg per iteration: {:?}\n", serde_time / ITERATIONS as u32);

    println!("Benchmark: simd_json::to_owned_value (MarketData)");
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        let mut buffer = market_data_json.as_bytes().to_vec();
        let val: simd_json::OwnedValue = simd_json::to_owned_value(&mut buffer).unwrap();
        let _ = val;
    }
    let simd_time = start.elapsed();
    println!("  Total: {:?}", simd_time);
    println!("  Avg per iteration: {:?}\n", simd_time / ITERATIONS as u32);

    let speedup = serde_time.as_nanos() as f64 / simd_time.as_nanos() as f64;
    println!("  Speedup: {:.2}x\n", speedup);

    println!("=== Target: < 0.05ms (50μs) per parse ===");
    let avg_us = simd_time.as_micros() as f64 / ITERATIONS as f64;
    if avg_us < 50.0 {
        println!("✅ PASS: Average {:.2}μs < 50μs", avg_us);
    } else {
        println!("❌ FAIL: Average {:.2}μs >= 50μs", avg_us);
    }
}
