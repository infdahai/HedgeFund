use std::collections::HashMap;

use rayon::prelude::*;
use sysinfo::{System, SystemExt};

use crate::strategy::arbitrage::ArbitrageConfig;

use super::config::CliConfig;
use super::sim;
use super::types::{MarketReport, MarketTick};

#[derive(Debug, Clone)]
pub(crate) struct SweepResult {
    pub(crate) min_profit_rate: f64,
    pub(crate) min_liquidity: f64,
    pub(crate) gas_cost_usd: f64,
    pub(crate) total_trades: u64,
    pub(crate) pnl: f64,
    pub(crate) win_rate: f64,
    pub(crate) max_drawdown: f64,
    pub(crate) risk_adjusted: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct ValidationResult {
    pub(crate) config: SweepResult,
    pub(crate) train_pnl: f64,
    pub(crate) train_win_rate: f64,
    pub(crate) test_pnl: f64,
    pub(crate) test_win_rate: f64,
    pub(crate) gap_pct: f64,
    pub(crate) win_rate_gap_pct: f64,
}

pub(crate) fn run_sweep(
    markets: &HashMap<String, Vec<MarketTick>>,
    market_metadata: &HashMap<String, i64>,
    config: &CliConfig,
) -> Vec<SweepResult> {
    let mut sys = System::new_all();
    sys.refresh_memory();
    let mem_before_kb = sys.used_memory();
    tracing::info!(
        memory_used_mb = mem_before_kb / 1024,
        "parameter sweep started"
    );

    if let Some(max_mem_gb) = config.max_mem_gb {
        let current_mem_gb = mem_before_kb as f64 / (1024.0 * 1024.0);
        if current_mem_gb > max_mem_gb {
            tracing::error!(
                current_mem_gb = current_mem_gb,
                max_mem_gb = max_mem_gb,
                "memory limit exceeded, aborting sweep"
            );
            return Vec::new();
        }
    }

    let profit_rates = config
        .sweep_min_profit_rates
        .clone()
        .unwrap_or_else(|| vec![0.001, 0.003, 0.005, 0.007, 0.01]);
    let liquidities = config
        .sweep_min_liquidities
        .clone()
        .unwrap_or_else(|| vec![0.0, 50.0, 100.0, 200.0]);
    let gas_costs = config
        .sweep_gas_costs
        .clone()
        .unwrap_or_else(|| vec![config.gas_cost_usd]);

    let mut grid = Vec::new();
    for &profit_rate in &profit_rates {
        for &liquidity in &liquidities {
            for &gas_cost_usd in &gas_costs {
                grid.push((profit_rate, liquidity, gas_cost_usd));
            }
        }
    }

    let results = grid
        .par_iter()
        .map(|&(profit_rate, liquidity, gas_cost_usd)| {
            let summary = run_simulation(
                markets,
                market_metadata,
                config,
                profit_rate,
                liquidity,
                gas_cost_usd,
            );

            SweepResult {
                min_profit_rate: profit_rate,
                min_liquidity: liquidity,
                gas_cost_usd,
                total_trades: summary.total_trades,
                pnl: summary.pnl,
                win_rate: summary.win_rate,
                max_drawdown: summary.max_drawdown,
                risk_adjusted: if summary.max_drawdown > 0.0 {
                    summary.pnl / summary.max_drawdown
                } else {
                    0.0
                },
            }
        })
        .collect();

    sys.refresh_memory();
    let mem_after_kb = sys.used_memory();
    let mem_delta_mb = mem_after_kb.saturating_sub(mem_before_kb) / 1024;
    tracing::info!(
        memory_used_mb = mem_after_kb / 1024,
        memory_delta_mb = mem_delta_mb,
        "parameter sweep completed"
    );

    results
}

pub(crate) fn split_by_time(
    markets: &HashMap<String, Vec<MarketTick>>,
    train_ratio: f64,
) -> (
    HashMap<String, Vec<MarketTick>>,
    HashMap<String, Vec<MarketTick>>,
) {
    let mut train = HashMap::new();
    let mut test = HashMap::new();

    for (market_id, ticks) in markets {
        let mut sorted_ticks = ticks.clone();
        sorted_ticks.sort_by_key(|tick| tick.ts_us);

        let split_idx = (sorted_ticks.len() as f64 * train_ratio) as usize;
        let split_idx = split_idx.min(sorted_ticks.len());

        train.insert(market_id.clone(), sorted_ticks[..split_idx].to_vec());
        test.insert(market_id.clone(), sorted_ticks[split_idx..].to_vec());
    }

    (train, test)
}

pub(crate) fn validate_configs(
    train: &HashMap<String, Vec<MarketTick>>,
    test: &HashMap<String, Vec<MarketTick>>,
    market_metadata: &HashMap<String, i64>,
    config: &CliConfig,
    top_n: usize,
) -> Vec<ValidationResult> {
    let mut results = run_sweep(train, market_metadata, config);
    results.sort_by(|a, b| {
        b.risk_adjusted
            .partial_cmp(&a.risk_adjusted)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let top = results.into_iter().take(top_n.max(1));
    top.map(|candidate| {
        let test_summary = run_simulation(
            test,
            market_metadata,
            config,
            candidate.min_profit_rate,
            candidate.min_liquidity,
            candidate.gas_cost_usd,
        );

        let gap_pct = pct_gap(candidate.pnl, test_summary.pnl);
        let win_rate_gap_pct = pct_gap(candidate.win_rate, test_summary.win_rate);

        ValidationResult {
            train_pnl: candidate.pnl,
            train_win_rate: candidate.win_rate,
            test_pnl: test_summary.pnl,
            test_win_rate: test_summary.win_rate,
            gap_pct,
            win_rate_gap_pct,
            config: candidate,
        }
    })
    .collect()
}

pub(crate) fn data_period(markets: &HashMap<String, Vec<MarketTick>>) -> Option<(i64, i64)> {
    let mut min_ts: Option<i64> = None;
    let mut max_ts: Option<i64> = None;
    for ticks in markets.values() {
        for tick in ticks {
            min_ts = Some(min_ts.map_or(tick.ts_us, |value| value.min(tick.ts_us)));
            max_ts = Some(max_ts.map_or(tick.ts_us, |value| value.max(tick.ts_us)));
        }
    }
    match (min_ts, max_ts) {
        (Some(start), Some(end)) => Some((start, end)),
        _ => None,
    }
}

struct SummaryMetrics {
    total_trades: u64,
    pnl: f64,
    win_rate: f64,
    max_drawdown: f64,
}

fn run_simulation(
    markets: &HashMap<String, Vec<MarketTick>>,
    market_metadata: &HashMap<String, i64>,
    config: &CliConfig,
    min_profit_rate: f64,
    min_liquidity: f64,
    gas_cost_usd: f64,
) -> SummaryMetrics {
    let mut sim_config = config.clone();
    sim_config.min_profit_rate = min_profit_rate;
    sim_config.min_liquidity = min_liquidity;
    sim_config.gas_cost_usd = gas_cost_usd;

    let arb_config = ArbitrageConfig {
        min_profit_rate,
        min_liquidity,
        min_order_notional: config.min_order_notional,
        min_fee: config.min_fee,
        ..ArbitrageConfig::default()
    };

    let reports: Vec<MarketReport> = markets
        .iter()
        .map(|(market_id, ticks)| {
            let resolution_ts = market_metadata.get(market_id).copied();
            let market_info = sim::build_market_info(market_id, resolution_ts);
            sim::simulate_market(&market_info, ticks, &sim_config, &arb_config)
        })
        .collect();

    summarize_reports(&reports)
}

fn summarize_reports(reports: &[MarketReport]) -> SummaryMetrics {
    let total_trades: u64 = reports.iter().map(|report| report.total_trades).sum();
    let pnl: f64 = reports.iter().map(|report| report.pnl).sum();
    let max_drawdown = reports
        .iter()
        .map(|report| report.max_drawdown)
        .fold(0.0, f64::max);
    let winning: u64 = reports
        .iter()
        .map(|report| (report.win_rate * report.total_trades as f64) as u64)
        .sum();
    let win_rate = if total_trades > 0 {
        winning as f64 / total_trades as f64
    } else {
        0.0
    };

    SummaryMetrics {
        total_trades,
        pnl,
        win_rate,
        max_drawdown,
    }
}

fn pct_gap(train: f64, test: f64) -> f64 {
    if train.abs() < f64::EPSILON {
        0.0
    } else {
        (test - train) / train * 100.0
    }
}
