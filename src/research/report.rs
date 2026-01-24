use std::collections::HashSet;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::Utc;
use serde::Serialize;

use super::config::CliConfig;
use super::sweep::{SweepResult, ValidationResult};
use super::types::{ConfirmedOrderRecord, MarketReport, MicrostructureReport, TradeRecord};

pub(crate) fn print_summary(
    reports: &[MarketReport],
    trades: &[TradeRecord],
    micro_reports: &[MicrostructureReport],
    confirmed_orders: &[ConfirmedOrderRecord],
    config: &CliConfig,
) {
    let total_trades: u64 = reports.iter().map(|r| r.total_trades).sum();
    let total_pnl: f64 = reports.iter().map(|r| r.pnl).sum();
    let total_fees: f64 = reports.iter().map(|r| r.total_fees).sum();
    let max_drawdown = reports.iter().map(|r| r.max_drawdown).fold(0.0, f64::max);
    let avg_latency = reports
        .iter()
        .map(|r| r.avg_confirm_latency_us)
        .fold(0.0, |acc, v| acc + v)
        / reports.len().max(1) as f64;
    let avg_capital_turnover = reports
        .iter()
        .map(|r| r.capital_turnover)
        .fold(0.0, |acc, v| acc + v)
        / reports.len().max(1) as f64;
    let avg_return_on_capital_employed = reports
        .iter()
        .map(|r| r.return_on_capital_employed)
        .fold(0.0, |acc, v| acc + v)
        / reports.len().max(1) as f64;

    println!("research summary:");
    println!("  markets: {}", reports.len());
    println!("  trades: {}", total_trades);
    println!("  pnl: {:.6}", total_pnl);
    println!("  fees: {:.6}", total_fees);
    println!("  max_drawdown: {:.4}", max_drawdown);
    println!("  avg_confirm_latency_us: {:.1}", avg_latency);
    println!("  capital_turnover: {:.4}", avg_capital_turnover);
    println!(
        "  return_on_capital_employed: {:.4}",
        avg_return_on_capital_employed
    );
    println!("  initial_balance: {:.2}", config.initial_balance);
    println!("  output_rows: {}", trades.len());

    if !micro_reports.is_empty() {
        print_microstructure_summary(micro_reports);
    }

    if !confirmed_orders.is_empty() {
        let unique_markets = confirmed_orders
            .iter()
            .map(|record| record.market_id.as_str())
            .collect::<HashSet<_>>()
            .len();
        println!(
            "  confirmed_orders: {} (markets={})",
            confirmed_orders.len(),
            unique_markets
        );
    }

    for report in reports.iter().filter(|r| r.total_trades > 0) {
        println!(
            "  market {} trades {} win_rate {:.2} pnl {:.6}",
            report.market_id, report.total_trades, report.win_rate, report.pnl
        );
    }
}

pub(crate) fn write_trade_report(path: &str, trades: &[TradeRecord]) -> Result<()> {
    let output_path = Path::new(path);
    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).with_context(|| format!("create output dir {parent:?}"))?;
        }
    }

    let mut contents =
        String::from("market_id,ts_us,arb_type,total_spend,expected_profit,balance_after\n");
    for trade in trades {
        contents.push_str(&format!(
            "{},{},{},{:.6},{:.6},{:.6}\n",
            trade.market_id,
            trade.ts_us,
            trade.arb_type,
            trade.total_spend,
            trade.expected_profit,
            trade.balance_after
        ));
    }

    fs::write(output_path, contents).with_context(|| format!("write report {path}"))?;
    Ok(())
}

pub(crate) fn write_microstructure_report(
    path: &str,
    reports: &[MicrostructureReport],
) -> Result<()> {
    let output_path = Path::new(path);
    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).with_context(|| format!("create output dir {parent:?}"))?;
        }
    }

    let mut contents = String::from(
        "market_id,liquidity_availability_pct,scan_skip_rate,spread_p50,spread_p95,slippage_100_shares,opportunity_frequency_per_hour,opportunity_avg_duration_ms,opportunity_p95_duration_ms,opportunity_count\n",
    );
    for report in reports {
        contents.push_str(&format!(
            "{},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.3},{:.3},{}\n",
            report.market_id,
            report.liquidity_availability_pct,
            report.scan_skip_rate,
            report.spread_p50,
            report.spread_p95,
            report.slippage_100_shares,
            report.opportunity_frequency_per_hour,
            report.opportunity_avg_duration_ms,
            report.opportunity_p95_duration_ms,
            report.opportunity_count
        ));
    }

    fs::write(output_path, contents).with_context(|| format!("write report {path}"))?;
    Ok(())
}

pub(crate) fn write_sweep_report(path: &str, results: &[SweepResult]) -> Result<()> {
    let output_path = Path::new(path);
    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).with_context(|| format!("create output dir {parent:?}"))?;
        }
    }

    let mut contents = String::from(
        "min_profit_rate,min_liquidity,gas_cost_usd,total_trades,pnl,win_rate,max_drawdown,risk_adjusted\n",
    );
    for result in results {
        contents.push_str(&format!(
            "{:.6},{:.6},{:.6},{},{:.6},{:.6},{:.6},{:.6}\n",
            result.min_profit_rate,
            result.min_liquidity,
            result.gas_cost_usd,
            result.total_trades,
            result.pnl,
            result.win_rate,
            result.max_drawdown,
            result.risk_adjusted
        ));
    }

    fs::write(output_path, contents).with_context(|| format!("write report {path}"))?;
    Ok(())
}

pub(crate) fn print_microstructure_summary(reports: &[MicrostructureReport]) {
    println!("  microstructure_reports: {}", reports.len());
    for report in reports {
        println!(
            "  micro {} liquidity_pct {:.2} scan_skip_rate {:.2} spread_p50 {:.4} spread_p95 {:.4} slippage_100 {:.4} opps/hr {:.4} opp_avg_ms {:.1} opp_p95_ms {:.1} opp_count {}",
            report.market_id,
            report.liquidity_availability_pct,
            report.scan_skip_rate,
            report.spread_p50,
            report.spread_p95,
            report.slippage_100_shares,
            report.opportunity_frequency_per_hour,
            report.opportunity_avg_duration_ms,
            report.opportunity_p95_duration_ms,
            report.opportunity_count
        );
    }
}

pub(crate) fn print_validation_report(best: &ValidationResult) {
    let gap = best.gap_pct;
    let win_gap = best.win_rate_gap_pct;
    let recommendation = if gap.abs() < 15.0 {
        "Config stable (gap < 15%)"
    } else {
        "Config unstable (gap >= 15%)"
    };

    println!("=== Validation Report ===");
    println!(
        "Best config: min_profit_rate={:.6}, min_liquidity={:.2}, gas_cost_usd={:.2}",
        best.config.min_profit_rate, best.config.min_liquidity, best.config.gas_cost_usd
    );
    println!();
    println!("        Train     Test      Gap");
    println!(
        "PnL     {:.2}     {:.2}    {:+.1}%",
        best.train_pnl, best.test_pnl, gap
    );
    println!(
        "WinRate {:.2}      {:.2}     {:+.1}%",
        best.train_win_rate, best.test_win_rate, win_gap
    );
    println!();
    println!("Recommendation: {recommendation}");
}

#[derive(Serialize)]
struct RecommendedConfig {
    generated_at: String,
    data_period: DataPeriod,
    recommended: ConfigParams,
    validation: ValidationMetrics,
}

#[derive(Serialize)]
struct DataPeriod {
    start: String,
    end: String,
}

#[derive(Serialize)]
struct ConfigParams {
    min_profit_rate: f64,
    min_liquidity: f64,
    gas_cost_usd: f64,
}

#[derive(Serialize)]
struct ValidationMetrics {
    train_pnl: f64,
    test_pnl: f64,
    gap_pct: f64,
}

pub(crate) fn write_recommended_config(
    path: &str,
    best: &ValidationResult,
    data_start: &str,
    data_end: &str,
) -> Result<()> {
    let config = RecommendedConfig {
        generated_at: Utc::now().to_rfc3339(),
        data_period: DataPeriod {
            start: data_start.to_string(),
            end: data_end.to_string(),
        },
        recommended: ConfigParams {
            min_profit_rate: best.config.min_profit_rate,
            min_liquidity: best.config.min_liquidity,
            gas_cost_usd: best.config.gas_cost_usd,
        },
        validation: ValidationMetrics {
            train_pnl: best.train_pnl,
            test_pnl: best.test_pnl,
            gap_pct: best.gap_pct,
        },
    };

    let json = serde_json::to_string_pretty(&config)?;
    fs::write(path, json).with_context(|| format!("write report {path}"))?;
    Ok(())
}
