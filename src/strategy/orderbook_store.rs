use dashmap::DashMap;
use serde::Deserialize;
use serde_json::Value;
use std::cmp::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::strategy::market_scanner::{MarketOrderbook, OrderbookSide};

#[derive(Debug, Clone)]
pub struct LocalOrderbook {
    pub orderbook: MarketOrderbook,
    pub last_update: Instant,
}

#[derive(Clone, Default)]
pub struct LocalOrderbookStore {
    books: Arc<DashMap<String, LocalOrderbook>>,
}

impl LocalOrderbookStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update_from_ws(&self, pair: &str, data: &str) -> Option<MarketOrderbook> {
        let parsed: WsOrderbook = serde_json::from_str(data).ok()?;
        let mut bids = parse_side(&parsed.bids, Side::Bid);
        let mut asks = parse_side(&parsed.asks, Side::Ask);

        if bids.is_empty() && asks.is_empty() {
            return None;
        }

        bids.sort_by(|a, b| b.price.partial_cmp(&a.price).unwrap_or(Ordering::Equal));
        asks.sort_by(|a, b| a.price.partial_cmp(&b.price).unwrap_or(Ordering::Equal));

        let orderbook = MarketOrderbook {
            pair: pair.to_string(),
            bids,
            asks,
        };

        self.books.insert(
            pair.to_string(),
            LocalOrderbook {
                orderbook: orderbook.clone(),
                last_update: Instant::now(),
            },
        );

        Some(orderbook)
    }

    pub fn get(&self, pair: &str) -> Option<MarketOrderbook> {
        self.books.get(pair).map(|entry| entry.orderbook.clone())
    }

    pub fn upsert(&self, pair: &str, orderbook: MarketOrderbook) {
        self.books.insert(
            pair.to_string(),
            LocalOrderbook {
                orderbook,
                last_update: Instant::now(),
            },
        );
    }

    pub fn apply_depth_diff(&self, pair: &str, side: &str, price: f64, size: f64) -> bool {
        let is_bid = match side.to_lowercase().as_str() {
            "bids" | "bid" => true,
            "asks" | "ask" => false,
            _ => return false,
        };

        let mut orderbook = self
            .books
            .get(pair)
            .map(|entry| entry.orderbook.clone())
            .unwrap_or_else(|| MarketOrderbook {
                pair: pair.to_string(),
                bids: Vec::new(),
                asks: Vec::new(),
            });

        let levels = if is_bid {
            &mut orderbook.bids
        } else {
            &mut orderbook.asks
        };

        let mut matched_index = None;
        for (idx, level) in levels.iter().enumerate() {
            if (level.price - price).abs() < 1e-12 {
                matched_index = Some(idx);
                break;
            }
        }

        if size <= 0.0 {
            if let Some(idx) = matched_index {
                levels.remove(idx);
            }
        } else if let Some(idx) = matched_index {
            levels[idx].size = size;
        } else {
            levels.push(OrderbookSide { price, size });
        }

        if is_bid {
            levels.sort_by(|a, b| b.price.partial_cmp(&a.price).unwrap_or(Ordering::Equal));
        } else {
            levels.sort_by(|a, b| a.price.partial_cmp(&b.price).unwrap_or(Ordering::Equal));
        }

        self.upsert(pair, orderbook);
        true
    }

    pub fn last_update(&self, pair: &str) -> Option<Instant> {
        self.books.get(pair).map(|entry| entry.last_update)
    }

    pub fn is_stale(&self, pair: &str, max_age: Duration) -> bool {
        self.last_update(pair)
            .map(|ts| ts.elapsed() > max_age)
            .unwrap_or(true)
    }
}

#[derive(Debug, Deserialize)]
struct WsOrderbook {
    #[serde(default)]
    bids: Vec<Value>,
    #[serde(default)]
    asks: Vec<Value>,
}

#[derive(Debug, Deserialize)]
struct WsEntry {
    #[serde(default)]
    price: Option<Value>,
    #[serde(default)]
    size: Option<Value>,
    #[serde(default)]
    amount: Option<Value>,
    #[serde(default)]
    qty: Option<Value>,
}

#[derive(Debug, Clone, Copy)]
enum Side {
    Bid,
    Ask,
}

fn parse_side(entries: &[Value], _side: Side) -> Vec<OrderbookSide> {
    entries
        .iter()
        .filter_map(parse_entry)
        .filter(|entry| entry.price > 0.0 && entry.size > 0.0)
        .collect()
}

fn parse_entry(value: &Value) -> Option<OrderbookSide> {
    match value {
        Value::Array(items) => {
            if items.len() < 2 {
                return None;
            }
            let price = parse_f64(&items[0])?;
            let size = parse_f64(&items[1])?;
            Some(OrderbookSide { price, size })
        }
        Value::Object(_) => {
            let entry: WsEntry = serde_json::from_value(value.clone()).ok()?;
            let price_val = entry.price?;
            let size_val = entry.size.or(entry.amount).or(entry.qty)?;
            let price = parse_f64(&price_val)?;
            let size = parse_f64(&size_val)?;
            Some(OrderbookSide { price, size })
        }
        _ => None,
    }
}

fn parse_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(num) => num.as_f64(),
        Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    }
}
