use anyhow::Result;
use rayon::prelude::*;

use crate::strategy::arbitrage::ArbitrageConfig;
use crate::telemetry;

mod config;
mod io;
mod metadata;
mod microstructure;
mod report;
mod sim;
mod types;

pub fn run() -> Result<()> {
    let _guards = telemetry::init_telemetry();
    let config = config::parse_args()?;
    let df = io::load_snapshots(&config)?;
    let rows = io::rows_from_df(&df)?;
    let markets = io::build_market_ticks(rows);
    let micro_reports = microstructure::build_microstructure_reports(&markets, &config);
    let market_metadata = metadata::load_market_metadata(config.market_metadata.as_deref())?;

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
            sim::simulate_market(&market_info, ticks, &config, &arb_config)
        })
        .collect();

    let mut trades = Vec::new();
    for report in &reports {
        trades.extend(report.trades.clone());
    }

    report::write_trade_report(&config.output, &trades)?;
    let confirmed = io::load_confirmed_orders(config.confirmed_db.as_deref()).unwrap_or_default();
    report::print_summary(&reports, &trades, &micro_reports, &confirmed, &config);

    Ok(())
}
