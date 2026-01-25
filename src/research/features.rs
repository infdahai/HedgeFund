use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use polars::prelude::*;

use super::config::CliConfig;

const HORIZON_10S_US: i64 = 10_000_000;
const HORIZON_30S_US: i64 = 30_000_000;
const DEPTH_LEVELS: usize = 5;

#[derive(Debug, Clone)]
struct FeatureRow {
    market_id: String,
    pair: String,
    outcome: String,
    ts_us: i64,
    mid_price: f64,
    spread_bps: f64,
    bid_depth_l1: f64,
    ask_depth_l1: f64,
    obi_l1: f64,
    bid_depth_l5: f64,
    ask_depth_l5: f64,
    obi_l5: f64,
    next_mid_10s: Option<f64>,
    next_mid_30s: Option<f64>,
}

pub(crate) fn write_features(df: &DataFrame, config: &CliConfig) -> Result<()> {
    let mut grouped = extract_rows(df)?;
    for rows in grouped.values_mut() {
        rows.sort_by_key(|row| row.ts_us);
        apply_forward_prices(rows);
    }

    let output_path = config
        .output_features
        .as_deref()
        .unwrap_or("reports/features.csv");
    let output_path = Path::new(output_path);
    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).with_context(|| format!("create output dir {parent:?}"))?;
        }
    }

    let mut rows: Vec<FeatureRow> = grouped.into_values().flatten().collect();
    rows.sort_by(|a, b| {
        a.market_id
            .cmp(&b.market_id)
            .then_with(|| a.ts_us.cmp(&b.ts_us))
    });

    let mut contents = String::from(
        "market_id,pair,outcome,ts_us,mid_price,spread_bps,bid_depth_l1,ask_depth_l1,obi_l1,bid_depth_l5,ask_depth_l5,obi_l5,next_mid_10s,next_mid_30s\n",
    );
    for row in rows {
        contents.push_str(&format!(
            "{},{},{},{},{:.6},{:.4},{:.4},{:.4},{:.6},{:.4},{:.4},{:.6},{},{}\n",
            row.market_id,
            row.pair,
            row.outcome,
            row.ts_us,
            row.mid_price,
            row.spread_bps,
            row.bid_depth_l1,
            row.ask_depth_l1,
            row.obi_l1,
            row.bid_depth_l5,
            row.ask_depth_l5,
            row.obi_l5,
            option_f64(row.next_mid_10s),
            option_f64(row.next_mid_30s)
        ));
    }

    fs::write(output_path, contents).with_context(|| format!("write features {output_path:?}"))?;
    Ok(())
}

fn extract_rows(df: &DataFrame) -> Result<HashMap<String, Vec<FeatureRow>>> {
    let market_ids = df
        .column("market_id")
        .context("missing market_id")?
        .str()
        .context("invalid market_id type")?;
    let pairs = df
        .column("pair")
        .context("missing pair")?
        .str()
        .context("invalid pair type")?;
    let outcomes = df.column("outcome").ok().and_then(|col| col.str().ok());

    let local_recv = df
        .column("local_recv_ts_us")
        .ok()
        .and_then(|col| col.i64().ok());
    let exchange_ts = df
        .column("exchange_ts_us")
        .ok()
        .and_then(|col| col.i64().ok());
    let snapshot_ts = df.column("snapshot_ts").ok().and_then(|col| col.i64().ok());

    let best_bid_col = df.column("best_bid").ok().and_then(|col| col.f64().ok());
    let best_ask_col = df.column("best_ask").ok().and_then(|col| col.f64().ok());
    let bids_col = df.column("bids").ok();
    let asks_col = df.column("asks").ok();

    let mut grouped: HashMap<String, Vec<FeatureRow>> = HashMap::new();
    for idx in 0..df.height() {
        let market_id = match market_ids.get(idx) {
            Some(value) => value.to_string(),
            None => continue,
        };
        let pair = match pairs.get(idx) {
            Some(value) => value.to_string(),
            None => continue,
        };
        let outcome = outcomes
            .and_then(|col| col.get(idx).map(|value| value.to_string()))
            .unwrap_or_else(|| derive_outcome(&pair));

        let ts_us = if let Some(col) = &local_recv {
            col.get(idx).unwrap_or(0)
        } else if let Some(col) = &exchange_ts {
            col.get(idx).unwrap_or(0)
        } else if let Some(col) = &snapshot_ts {
            col.get(idx).unwrap_or(0).saturating_mul(1000)
        } else {
            0
        };
        if ts_us <= 0 {
            continue;
        }

        let best_bid = if let Some(col) = &best_bid_col {
            col.get(idx).unwrap_or(0.0)
        } else {
            bids_col
                .and_then(|col| level_at(col, idx, 0))
                .map(|(price, _)| price)
                .unwrap_or(0.0)
        };
        let best_ask = if let Some(col) = &best_ask_col {
            col.get(idx).unwrap_or(0.0)
        } else {
            asks_col
                .and_then(|col| level_at(col, idx, 0))
                .map(|(price, _)| price)
                .unwrap_or(0.0)
        };

        if best_bid <= 0.0 || best_ask <= 0.0 {
            continue;
        }

        let bid_depth_l1 = bids_col
            .and_then(|col| depth_size(col, idx, 1))
            .unwrap_or(0.0);
        let ask_depth_l1 = asks_col
            .and_then(|col| depth_size(col, idx, 1))
            .unwrap_or(0.0);
        let bid_depth_l5 = bids_col
            .and_then(|col| depth_size(col, idx, DEPTH_LEVELS))
            .unwrap_or(bid_depth_l1);
        let ask_depth_l5 = asks_col
            .and_then(|col| depth_size(col, idx, DEPTH_LEVELS))
            .unwrap_or(ask_depth_l1);

        let mid_price = (best_bid + best_ask) / 2.0;
        let spread = (best_ask - best_bid).max(0.0);
        let spread_bps = if mid_price > 0.0 {
            spread / mid_price * 10_000.0
        } else {
            0.0
        };
        let obi_l1 = imbalance(bid_depth_l1, ask_depth_l1);
        let obi_l5 = imbalance(bid_depth_l5, ask_depth_l5);

        let row = FeatureRow {
            market_id: market_id.clone(),
            pair: pair.clone(),
            outcome,
            ts_us,
            mid_price,
            spread_bps,
            bid_depth_l1,
            ask_depth_l1,
            obi_l1,
            bid_depth_l5,
            ask_depth_l5,
            obi_l5,
            next_mid_10s: None,
            next_mid_30s: None,
        };
        let key = format!("{}::{}", market_id, pair);
        grouped.entry(key).or_default().push(row);
    }

    if grouped.is_empty() {
        return Err(anyhow!("no valid snapshot rows for feature extraction"));
    }

    Ok(grouped)
}

fn apply_forward_prices(rows: &mut [FeatureRow]) {
    let len = rows.len();
    let mut j10 = 0usize;
    let mut j30 = 0usize;
    for i in 0..len {
        let ts = rows[i].ts_us;
        if j10 < i {
            j10 = i;
        }
        if j30 < i {
            j30 = i;
        }
        while j10 < len && rows[j10].ts_us < ts + HORIZON_10S_US {
            j10 += 1;
        }
        while j30 < len && rows[j30].ts_us < ts + HORIZON_30S_US {
            j30 += 1;
        }
        if j10 < len {
            rows[i].next_mid_10s = Some(rows[j10].mid_price);
        }
        if j30 < len {
            rows[i].next_mid_30s = Some(rows[j30].mid_price);
        }
    }
}

fn imbalance(bid: f64, ask: f64) -> f64 {
    let total = bid + ask;
    if total > 0.0 {
        (bid - ask) / total
    } else {
        0.0
    }
}

fn option_f64(value: Option<f64>) -> String {
    value.map(|v| format!("{v:.6}")).unwrap_or_default()
}

fn derive_outcome(pair: &str) -> String {
    if let Some((_, suffix)) = pair.rsplit_once('-') {
        suffix.trim().to_uppercase()
    } else {
        "UNKNOWN".to_string()
    }
}

fn level_at(column: &Column, idx: usize, level: usize) -> Option<(f64, f64)> {
    let series = column.as_materialized_series();
    let list = series.list().ok()?;
    let entry = list.get_as_series(idx)?;
    let struct_series = entry.struct_().ok()?;
    let price_field = struct_series.field_by_name("p").ok()?;
    let price_series = price_field.f64().ok()?;
    let size_field = struct_series.field_by_name("s").ok()?;
    let size_series = size_field.f64().ok()?;
    if level >= price_series.len() {
        return None;
    }
    Some((price_series.get(level)?, size_series.get(level)?))
}

fn depth_size(column: &Column, idx: usize, depth: usize) -> Option<f64> {
    let series = column.as_materialized_series();
    let list = series.list().ok()?;
    let entry = list.get_as_series(idx)?;
    let struct_series = entry.struct_().ok()?;
    let size_field = struct_series.field_by_name("s").ok()?;
    let size_series = size_field.f64().ok()?;
    let max = depth.min(size_series.len());
    let mut sum = 0.0;
    for i in 0..max {
        if let Some(value) = size_series.get(i) {
            sum += value;
        }
    }
    Some(sum)
}
