use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use glob::glob;
use polars::prelude::*;
use rusqlite::Connection;
use tracing::warn;

use super::config::CliConfig;
use super::types::{ConfirmedOrderRecord, MarketTick, SnapshotRow, TickBook};

pub(crate) fn load_snapshots(config: &CliConfig) -> Result<DataFrame> {
    let mut frames = Vec::new();
    let mut skipped = 0usize;
    let mut not_ready = 0usize;

    for entry in glob(&config.input).context("invalid glob pattern")? {
        let path = entry.context("invalid glob entry")?;
        if !partition_ready(&path) {
            not_ready += 1;
            continue;
        }
        let mut lf = match LazyFrame::scan_parquet(&path, Default::default()) {
            Ok(frame) => frame,
            Err(err) => {
                skipped += 1;
                warn!(target: "research", "skip corrupt parquet file: {:?}, error: {}", path, err);
                continue;
            }
        };

        if let Some(ref market_id) = config.market_id {
            lf = lf.filter(col("market_id").eq(lit(market_id.as_str())));
        }
        frames.push(lf);
    }

    if frames.is_empty() {
        return Err(anyhow!(
            "no valid parquet files found (skipped={}, not_ready={})",
            skipped,
            not_ready
        ));
    }

    if skipped > 0 {
        warn!(target: "research", "parquet files skipped: {}", skipped);
    }
    if not_ready > 0 {
        warn!(target: "research", "parquet partitions not ready: {}", not_ready);
    }

    let df = concat(frames, UnionArgs::default())?
        .with_streaming(true)
        .collect()?;
    Ok(df)
}

pub(crate) fn load_scan_skip_rates(config: &CliConfig) -> Result<(HashMap<String, f64>, f64)> {
    let mut frames = Vec::new();
    let mut skipped = 0usize;
    let mut not_ready = 0usize;

    for entry in glob(&config.scan_stats_input).context("invalid scan_stats glob pattern")? {
        let path = entry.context("invalid scan_stats glob entry")?;
        if !partition_ready(&path) {
            not_ready += 1;
            continue;
        }
        let lf = match LazyFrame::scan_parquet(&path, Default::default()) {
            Ok(frame) => frame,
            Err(err) => {
                skipped += 1;
                warn!(
                    target: "research",
                    "skip corrupt scan_stats parquet file: {:?}, error: {}",
                    path,
                    err
                );
                continue;
            }
        };
        frames.push(lf);
    }

    if frames.is_empty() {
        warn!(
            target: "research",
            "no scan_stats parquet files found for pattern: {}",
            config.scan_stats_input
        );
        if skipped > 0 {
            warn!(target: "research", "scan_stats parquet files skipped: {}", skipped);
        }
        if not_ready > 0 {
            warn!(
                target: "research",
                "scan_stats partitions not ready: {}",
                not_ready
            );
        }
        return Ok((HashMap::new(), 0.0));
    }

    if skipped > 0 {
        warn!(target: "research", "scan_stats parquet files skipped: {}", skipped);
    }
    if not_ready > 0 {
        warn!(
            target: "research",
            "scan_stats partitions not ready: {}",
            not_ready
        );
    }

    let df = concat(frames, UnionArgs::default())?
        .with_streaming(true)
        .collect()?;

    let has_market_id = df.column("market_id").is_ok();
    if !has_market_id {
        let rate = compute_scan_skip_rate(&df)?;
        return Ok((HashMap::new(), rate));
    }

    let grouped = df
        .clone()
        .lazy()
        .group_by([col("market_id")])
        .agg([
            col("yesno_checked").sum().alias("yesno_checked"),
            col("yesno_found").sum().alias("yesno_found"),
            col("mint_sell_checked").sum().alias("mint_sell_checked"),
            col("mint_sell_found").sum().alias("mint_sell_found"),
            col("neg_risk_checked").sum().alias("neg_risk_checked"),
            col("neg_risk_found").sum().alias("neg_risk_found"),
        ])
        .collect()?;

    let market_ids = grouped
        .column("market_id")
        .context("missing scan_stats market_id")?
        .str()
        .context("invalid scan_stats market_id type")?;

    let mut per_market = HashMap::new();
    let mut global_rate = 0.0;
    let mut has_global_row = false;

    for idx in 0..grouped.height() {
        let market_id = match market_ids.get(idx) {
            Some(value) => value.to_string(),
            None => continue,
        };
        let total_checked = value_u64_at(grouped.column("yesno_checked")?, idx)?
            + value_u64_at(grouped.column("mint_sell_checked")?, idx)?
            + value_u64_at(grouped.column("neg_risk_checked")?, idx)?;
        let total_found = value_u64_at(grouped.column("yesno_found")?, idx)?
            + value_u64_at(grouped.column("mint_sell_found")?, idx)?
            + value_u64_at(grouped.column("neg_risk_found")?, idx)?;
        let rate = if total_checked > 0 {
            let skipped = total_checked.saturating_sub(total_found);
            skipped as f64 / total_checked as f64 * 100.0
        } else {
            0.0
        };

        if market_id == "ALL" {
            global_rate = rate;
            has_global_row = true;
            continue;
        }
        per_market.insert(market_id, rate);
    }

    if !has_global_row {
        global_rate = compute_scan_skip_rate(&df)?;
    }

    Ok((per_market, global_rate))
}

pub(crate) fn rows_from_df(df: &DataFrame) -> Result<Vec<SnapshotRow>> {
    let market_ids = df
        .column("market_id")
        .context("missing market_id")?
        .str()
        .context("invalid market_id type")?;
    let local_recv = df
        .column("local_recv_ts_us")
        .ok()
        .and_then(|col| col.i64().ok());
    let snapshot_ts = df.column("snapshot_ts").ok().and_then(|col| col.i64().ok());
    if local_recv.is_none() && snapshot_ts.is_none() {
        return Err(anyhow!(
            "missing timestamp column (expected local_recv_ts_us or snapshot_ts)"
        ));
    }
    let best_bid = df.column("best_bid").ok().and_then(|col| col.f64().ok());
    let best_ask = df.column("best_ask").ok().and_then(|col| col.f64().ok());
    let bid_size = df.column("bid_size").ok().and_then(|col| col.f64().ok());
    let ask_size = df.column("ask_size").ok().and_then(|col| col.f64().ok());
    let bids = df.column("bids").ok();
    let asks = df.column("asks").ok();
    let outcomes = df.column("outcome").ok().and_then(|col| col.str().ok());
    let pairs = df.column("pair").ok().and_then(|col| col.str().ok());

    let mut rows = Vec::with_capacity(df.height());
    for idx in 0..df.height() {
        let market_id = match market_ids.get(idx) {
            Some(value) => value.to_string(),
            None => continue,
        };
        let ts_us = if let Some(col) = &local_recv {
            match col.get(idx) {
                Some(value) => value,
                None => continue,
            }
        } else if let Some(col) = &snapshot_ts {
            match col.get(idx) {
                Some(value) => value.saturating_mul(1000),
                None => continue,
            }
        } else {
            continue;
        };

        let (best_bid, bid_size) = if let Some(col) = &best_bid {
            let price = match col.get(idx) {
                Some(value) => value,
                None => continue,
            };
            let size = bid_size
                .as_ref()
                .and_then(|sizes| sizes.get(idx))
                .unwrap_or(0.0);
            (price, size)
        } else if let Some(col) = &bids {
            match extract_top_from_list_series(col, idx) {
                Some(value) => value,
                None => continue,
            }
        } else {
            continue;
        };

        let (best_ask, ask_size) = if let Some(col) = &best_ask {
            let price = match col.get(idx) {
                Some(value) => value,
                None => continue,
            };
            let size = ask_size
                .as_ref()
                .and_then(|sizes| sizes.get(idx))
                .unwrap_or(0.0);
            (price, size)
        } else if let Some(col) = &asks {
            match extract_top_from_list_series(col, idx) {
                Some(value) => value,
                None => continue,
            }
        } else {
            continue;
        };

        let outcome = outcomes
            .and_then(|col| col.get(idx).map(|value| value.to_string()))
            .or_else(|| {
                pairs
                    .and_then(|col| col.get(idx).map(|value| value.to_string()))
                    .and_then(derive_outcome)
            })
            .unwrap_or_else(|| "UNKNOWN".to_string());

        let outcome_upper = outcome.to_uppercase();
        if outcome_upper != "YES" && outcome_upper != "NO" {
            continue;
        }

        if best_bid <= 0.0 || best_ask <= 0.0 {
            continue;
        }

        rows.push(SnapshotRow {
            market_id,
            outcome: outcome_upper,
            ts_us,
            best_bid: round_price(best_bid),
            best_ask: round_price(best_ask),
            bid_size,
            ask_size,
        });
    }

    Ok(rows)
}

pub(crate) fn build_market_ticks(rows: Vec<SnapshotRow>) -> HashMap<String, Vec<MarketTick>> {
    let mut markets: HashMap<String, BTreeMap<i64, TickAccumulator>> = HashMap::new();

    for row in rows {
        let tick = markets
            .entry(row.market_id.clone())
            .or_default()
            .entry(row.ts_us)
            .or_default();

        let book = TickBook {
            best_bid: row.best_bid,
            best_ask: row.best_ask,
            bid_size: row.bid_size,
            ask_size: row.ask_size,
        };

        if row.outcome == "YES" {
            tick.yes = Some(book);
        } else {
            tick.no = Some(book);
        }
    }

    let mut result = HashMap::new();
    for (market_id, map) in markets {
        let mut ticks = Vec::new();
        for (ts, acc) in map {
            if let (Some(yes), Some(no)) = (acc.yes, acc.no) {
                ticks.push(MarketTick { ts_us: ts, yes, no });
            }
        }
        result.insert(market_id, ticks);
    }

    result
}

pub(crate) fn load_confirmed_orders(db_path: Option<&str>) -> Result<Vec<ConfirmedOrderRecord>> {
    let Some(path) = db_path else {
        return Ok(Vec::new());
    };

    let conn = Connection::open(path).with_context(|| format!("open confirmed db {path}"))?;
    let mut stmt = conn.prepare(
        "SELECT market_id, side, expected_price, actual_fill, confirmation_time_ms, timestamp_ms
         FROM confirmed_orders",
    )?;
    let rows = stmt.query_map([], |row| {
        let market_id: String = row.get(0)?;
        let side: String = row.get(1)?;
        let expected_price: f64 = row.get(2)?;
        let actual_fill: String = row.get(3)?;
        let confirmation_time_ms: u64 = row.get(4)?;
        let timestamp_ms: i64 = row.get(5)?;
        Ok(ConfirmedOrderRecord {
            market_id,
            side,
            expected_price,
            actual_fill,
            confirmation_time_ms,
            timestamp_ms,
        })
    })?;

    let mut records = Vec::new();
    for row in rows {
        records.push(row?);
    }
    Ok(records)
}

#[derive(Default)]
struct TickAccumulator {
    yes: Option<TickBook>,
    no: Option<TickBook>,
}

fn partition_ready(path: &Path) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    let success = parent.join("_SUCCESS");
    success.exists()
}

fn sum_u64(df: &DataFrame, name: &str) -> Result<u64> {
    let series = df
        .column(name)
        .with_context(|| format!("missing scan_stats column {name}"))?;
    if let Ok(col) = series.u64() {
        return Ok(col.get(0).unwrap_or(0));
    }
    if let Ok(col) = series.i64() {
        return Ok(col.get(0).unwrap_or(0).max(0) as u64);
    }
    if let Ok(col) = series.f64() {
        return Ok(col.get(0).unwrap_or(0.0).max(0.0) as u64);
    }
    Err(anyhow!("unsupported scan_stats column type: {name}"))
}

fn value_u64_at(column: &Column, idx: usize) -> Result<u64> {
    let series = column.as_materialized_series();
    if let Ok(col) = series.u64() {
        return Ok(col.get(idx).unwrap_or(0));
    }
    if let Ok(col) = series.i64() {
        return Ok(col.get(idx).unwrap_or(0).max(0) as u64);
    }
    if let Ok(col) = series.f64() {
        return Ok(col.get(idx).unwrap_or(0.0).max(0.0) as u64);
    }
    Err(anyhow!(
        "unsupported scan_stats column type: {}",
        series.name()
    ))
}

fn compute_scan_skip_rate(df: &DataFrame) -> Result<f64> {
    let total_checked = sum_u64(df, "yesno_checked")?
        + sum_u64(df, "mint_sell_checked")?
        + sum_u64(df, "neg_risk_checked")?;
    let total_found = sum_u64(df, "yesno_found")?
        + sum_u64(df, "mint_sell_found")?
        + sum_u64(df, "neg_risk_found")?;
    if total_checked == 0 {
        return Ok(0.0);
    }
    let skipped = total_checked.saturating_sub(total_found);
    Ok(skipped as f64 / total_checked as f64 * 100.0)
}

fn derive_outcome(pair: String) -> Option<String> {
    let upper = pair.to_uppercase();
    if upper.ends_with("-YES") {
        Some("YES".to_string())
    } else if upper.ends_with("-NO") {
        Some("NO".to_string())
    } else {
        None
    }
}

fn extract_top_from_list_series(column: &Column, idx: usize) -> Option<(f64, f64)> {
    let series = column.as_materialized_series();
    let list = series.list().ok()?;
    let entry = list.get_as_series(idx)?;
    if entry.is_empty() {
        return None;
    }
    let struct_series = entry.struct_().ok()?;
    let price_field = struct_series.field_by_name("p").ok()?;
    let price_series = price_field.f64().ok()?;
    let size_field = struct_series.field_by_name("s").ok()?;
    let size_series = size_field.f64().ok()?;
    Some((price_series.get(0)?, size_series.get(0)?))
}

fn round_price(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}
