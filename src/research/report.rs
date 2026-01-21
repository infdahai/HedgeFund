use std::collections::HashSet;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use super::config::CliConfig;
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

    println!("research summary:");
    println!("  markets: {}", reports.len());
    println!("  trades: {}", total_trades);
    println!("  pnl: {:.6}", total_pnl);
    println!("  fees: {:.6}", total_fees);
    println!("  max_drawdown: {:.4}", max_drawdown);
    println!("  avg_confirm_latency_us: {:.1}", avg_latency);
    println!("  initial_balance: {:.2}", config.initial_balance);
    println!("  output_rows: {}", trades.len());

    if !micro_reports.is_empty() {
        println!("  microstructure_reports: {}", micro_reports.len());
        for report in micro_reports {
            println!(
                "  micro {} liquidity_pct {:.2} spread_p50 {:.4} spread_p95 {:.4} slippage_100 {:.4} opps/hr {:.4}",
                report.market_id,
                report.liquidity_availability_pct,
                report.spread_p50,
                report.spread_p95,
                report.slippage_100_shares,
                report.opportunity_frequency_per_hour
            );
        }
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
