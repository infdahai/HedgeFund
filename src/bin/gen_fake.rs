use std::env;
use std::fs;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use chrono::{NaiveDate, NaiveDateTime, Timelike, Utc};
use polars::prelude::*;

struct GenConfig {
    date: String,
    hour: String,
    minutes: u32,
    step_ms: u32,
    output_dir: String,
}

fn main() -> Result<()> {
    let config = parse_args()?;
    let start_ts_us = build_start_timestamp(&config.date, &config.hour)?;
    let path = build_output_path(&config.output_dir, &config.date, &config.hour);
    fs::create_dir_all(&path).with_context(|| format!("create output dir: {}", path.display()))?;

    let (mut df, total_rows) = build_dataframe(start_ts_us, &config)?;

    let file_path = path.join("part-0.parquet");
    let file = fs::File::create(&file_path)?;
    ParquetWriter::new(file).finish(&mut df)?;

    println!("mock parquet generated:");
    println!("  path: {}", file_path.display());
    println!("  rows: {}", total_rows);
    println!("  date: {} hour: {}", config.date, config.hour);
    println!("  minutes: {} step_ms: {}", config.minutes, config.step_ms);
    println!("  opportunity window: 5 seconds (market mkt_opportunity_002)");
    Ok(())
}

fn parse_args() -> Result<GenConfig> {
    let now = Utc::now();
    let mut date = now.format("%Y-%m-%d").to_string();
    let mut hour = format!("{:02}", now.hour());
    let mut minutes = 10;
    let mut step_ms = 100;
    let mut output_dir = "data/parquet/orderbook_snapshots_v1".to_string();

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--date" => date = args.next().context("--date requires YYYY-MM-DD")?,
            "--hour" => hour = args.next().context("--hour requires HH")?,
            "--minutes" => {
                minutes = args
                    .next()
                    .context("--minutes requires value")?
                    .parse()
                    .context("invalid --minutes")?;
            }
            "--step-ms" => {
                step_ms = args
                    .next()
                    .context("--step-ms requires value")?
                    .parse()
                    .context("invalid --step-ms")?;
            }
            "--output-dir" => output_dir = args.next().context("--output-dir requires value")?,
            "--help" => {
                print_help();
                std::process::exit(0);
            }
            _ => return Err(anyhow!("unknown argument: {arg}")),
        }
    }

    Ok(GenConfig {
        date,
        hour,
        minutes,
        step_ms,
        output_dir,
    })
}

fn print_help() {
    println!(
        "gen_fake --date YYYY-MM-DD --hour HH [--minutes N] [--step-ms N] [--output-dir PATH]\n\nDefaults:\n  --minutes 10\n  --step-ms 100\n  --output-dir data/parquet/orderbook_snapshots_v1"
    );
}

fn build_start_timestamp(date: &str, hour: &str) -> Result<i64> {
    let date = NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .with_context(|| format!("invalid date {date}"))?;
    let hour: u32 = hour
        .parse()
        .with_context(|| format!("invalid hour {hour}"))?;
    let dt = NaiveDateTime::new(
        date,
        chrono::NaiveTime::from_hms_opt(hour, 0, 0)
            .ok_or_else(|| anyhow!("invalid hour {hour}"))?,
    );
    let ts = dt.and_utc().timestamp_micros();
    Ok(ts)
}

fn build_output_path(base: &str, date: &str, hour: &str) -> std::path::PathBuf {
    Path::new(base)
        .join(format!("date={}", date))
        .join(format!("hour={}", hour))
}

fn build_dataframe(start_ts_us: i64, config: &GenConfig) -> Result<(DataFrame, usize)> {
    let steps = (config.minutes as i64) * 60_000 / config.step_ms as i64;
    let mut market_ids: Vec<String> = Vec::with_capacity(steps as usize * 4);
    let mut outcomes: Vec<String> = Vec::with_capacity(steps as usize * 4);
    let mut best_bids: Vec<f64> = Vec::with_capacity(steps as usize * 4);
    let mut best_asks: Vec<f64> = Vec::with_capacity(steps as usize * 4);
    let mut bid_sizes: Vec<f64> = Vec::with_capacity(steps as usize * 4);
    let mut ask_sizes: Vec<f64> = Vec::with_capacity(steps as usize * 4);
    let mut timestamps: Vec<i64> = Vec::with_capacity(steps as usize * 4);

    for i in 0..steps {
        let ts = start_ts_us + i * (config.step_ms as i64) * 1000;

        // Market 1: boring (no arbitrage)
        push_snapshot(
            "mkt_boring_001",
            ts,
            0.4,
            0.6,
            0.4,
            0.6,
            &mut market_ids,
            &mut outcomes,
            &mut best_bids,
            &mut best_asks,
            &mut bid_sizes,
            &mut ask_sizes,
            &mut timestamps,
        );

        // Market 2: opportunity window
        let is_opportunity = (1000..1050).contains(&i);
        let (yes_bid, yes_ask, no_bid, no_ask) = if is_opportunity {
            (0.3, 0.4, 0.3, 0.4) // sum ask = 0.8 => arb
        } else {
            (0.5, 0.6, 0.5, 0.6) // sum ask = 1.2 => no arb
        };

        push_snapshot(
            "mkt_opportunity_002",
            ts,
            yes_bid,
            yes_ask,
            no_bid,
            no_ask,
            &mut market_ids,
            &mut outcomes,
            &mut best_bids,
            &mut best_asks,
            &mut bid_sizes,
            &mut ask_sizes,
            &mut timestamps,
        );
    }

    let df = DataFrame::new(vec![
        Series::new("market_id".into(), market_ids).into(),
        Series::new("outcome".into(), outcomes).into(),
        Series::new("best_bid".into(), best_bids).into(),
        Series::new("best_ask".into(), best_asks).into(),
        Series::new("bid_size".into(), bid_sizes).into(),
        Series::new("ask_size".into(), ask_sizes).into(),
        Series::new("local_recv_ts_us".into(), timestamps).into(),
    ])?;
    let total_rows = df.height();
    Ok((df, total_rows))
}

#[allow(clippy::too_many_arguments)]
fn push_snapshot(
    market_id: &str,
    ts: i64,
    yes_bid: f64,
    yes_ask: f64,
    no_bid: f64,
    no_ask: f64,
    market_ids: &mut Vec<String>,
    outcomes: &mut Vec<String>,
    best_bids: &mut Vec<f64>,
    best_asks: &mut Vec<f64>,
    bid_sizes: &mut Vec<f64>,
    ask_sizes: &mut Vec<f64>,
    timestamps: &mut Vec<i64>,
) {
    push_row(
        market_id, "YES", yes_bid, yes_ask, 1000.0, 1000.0, ts, market_ids, outcomes, best_bids,
        best_asks, bid_sizes, ask_sizes, timestamps,
    );
    push_row(
        market_id, "NO", no_bid, no_ask, 1000.0, 1000.0, ts, market_ids, outcomes, best_bids,
        best_asks, bid_sizes, ask_sizes, timestamps,
    );
}

#[allow(clippy::too_many_arguments)]
fn push_row(
    market_id: &str,
    outcome: &str,
    best_bid: f64,
    best_ask: f64,
    bid_size: f64,
    ask_size: f64,
    ts: i64,
    market_ids: &mut Vec<String>,
    outcomes: &mut Vec<String>,
    best_bids: &mut Vec<f64>,
    best_asks: &mut Vec<f64>,
    bid_sizes: &mut Vec<f64>,
    ask_sizes: &mut Vec<f64>,
    timestamps: &mut Vec<i64>,
) {
    market_ids.push(market_id.to_string());
    outcomes.push(outcome.to_string());
    best_bids.push(best_bid);
    best_asks.push(best_ask);
    bid_sizes.push(bid_size);
    ask_sizes.push(ask_size);
    timestamps.push(ts);
}
