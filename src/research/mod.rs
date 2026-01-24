use anyhow::{anyhow, Result};
use chrono::{TimeZone, Utc};
use rayon::prelude::*;
use std::collections::HashMap;
use sysinfo::{Pid, ProcessExt, System, SystemExt};
use tracing::info;

use crate::strategy::arbitrage::ArbitrageConfig;
use crate::telemetry;

mod config;
mod io;
mod metadata;
mod microstructure;
mod report;
mod sim;
mod sweep;
mod types;

pub fn run() -> Result<()> {
    let _guards = telemetry::init_telemetry();
    let config = config::parse_args()?;
    let df = io::load_snapshots(&config)?;
    let rows = io::rows_from_df(&df)?;
    let markets = io::build_market_ticks(rows);
    let market_metadata = metadata::load_market_metadata(config.market_metadata.as_deref())?;

    let total_ticks: usize = markets.values().map(|ticks| ticks.len()).sum();
    let estimated_bytes = total_ticks.saturating_mul(std::mem::size_of::<types::MarketTick>());

    match config.mode {
        config::RunMode::Backtest => run_backtest(&markets, &market_metadata, &config),
        config::RunMode::Microstructure => run_microstructure(&markets, &config),
        config::RunMode::Sweep => {
            info!(
                total_ticks = total_ticks,
                estimated_mb = (estimated_bytes as f64) / (1024.0 * 1024.0),
                "loaded market ticks for sweep"
            );
            log_memory_usage(config.max_mem_gb)?;
            let results = sweep::run_sweep(&markets, &market_metadata, &config);
            report::write_sweep_report(&config.output, &results)
        }
        config::RunMode::Validate => {
            info!(
                total_ticks = total_ticks,
                estimated_mb = (estimated_bytes as f64) / (1024.0 * 1024.0),
                "loaded market ticks for validation"
            );
            log_memory_usage(config.max_mem_gb)?;
            let (train, test) = sweep::split_by_time(&markets, 0.7);
            let validation_results =
                sweep::validate_configs(&train, &test, &market_metadata, &config, 3);
            let best = validation_results
                .first()
                .ok_or_else(|| anyhow!("validation produced no results"))?;
            report::print_validation_report(best);

            let (data_start, data_end) = sweep::data_period(&markets)
                .map(|(start, end)| (format_date(start), format_date(end)))
                .unwrap_or_else(|| ("unknown".to_string(), "unknown".to_string()));
            report::write_recommended_config(
                "reports/recommended_config.json",
                best,
                &data_start,
                &data_end,
            )
        }
    }
}

fn run_backtest(
    markets: &HashMap<String, Vec<types::MarketTick>>,
    market_metadata: &HashMap<String, i64>,
    config: &config::CliConfig,
) -> Result<()> {
    let (scan_skip_rates, default_rate) = io::load_scan_skip_rates(config)?;
    let micro_reports = microstructure::build_microstructure_reports(
        markets,
        config,
        &scan_skip_rates,
        default_rate,
    );
    let arb_config = ArbitrageConfig {
        min_profit_rate: config.min_profit_rate,
        min_liquidity: config.min_liquidity,
        min_order_notional: config.min_order_notional,
        min_fee: config.min_fee,
        ..ArbitrageConfig::default()
    };

    let reports: Vec<types::MarketReport> = markets
        .par_iter()
        .map(|(market_id, ticks)| {
            let resolution_ts = market_metadata.get(market_id).copied();
            let market_info = sim::build_market_info(market_id, resolution_ts);
            sim::simulate_market(&market_info, ticks, config, &arb_config)
        })
        .collect();

    let mut trades = Vec::new();
    for report in &reports {
        trades.extend(report.trades.clone());
    }

    report::write_trade_report(&config.output, &trades)?;
    let confirmed = io::load_confirmed_orders(config.confirmed_db.as_deref()).unwrap_or_default();
    report::print_summary(&reports, &trades, &micro_reports, &confirmed, config);

    Ok(())
}

fn run_microstructure(
    markets: &HashMap<String, Vec<types::MarketTick>>,
    config: &config::CliConfig,
) -> Result<()> {
    let (scan_skip_rates, default_rate) = io::load_scan_skip_rates(config)?;
    let reports = microstructure::build_microstructure_reports(
        markets,
        config,
        &scan_skip_rates,
        default_rate,
    );
    let output = config
        .output_microstructure
        .as_deref()
        .unwrap_or("reports/microstructure.csv");
    report::write_microstructure_report(output, &reports)?;
    report::print_microstructure_summary(&reports);
    Ok(())
}

fn format_date(ts_us: i64) -> String {
    let secs = ts_us.div_euclid(1_000_000);
    let micros = ts_us.rem_euclid(1_000_000) as u32;
    let nanos = micros * 1000;
    Utc.timestamp_opt(secs, nanos)
        .single()
        .map(|value| value.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn log_memory_usage(max_mem_gb: Option<f64>) -> Result<()> {
    let mut system = System::new();
    let pid = Pid::from_u32(std::process::id());
    system.refresh_process(pid);

    let Some(process) = system.process(pid) else {
        info!("memory usage unavailable for research process");
        return Ok(());
    };

    let rss_kb = process.memory();
    let rss_gb = rss_kb as f64 / (1024.0 * 1024.0);
    info!(rss_gb = rss_gb, "research memory usage");

    if let Some(max) = max_mem_gb {
        if rss_gb > max {
            return Err(anyhow!(
                "memory usage {:.2} GB exceeds max-mem-gb {:.2} GB",
                rss_gb,
                max
            ));
        }
    }

    Ok(())
}
