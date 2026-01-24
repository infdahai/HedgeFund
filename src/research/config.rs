use std::env;

use anyhow::{anyhow, Context, Result};

pub(crate) const DEFAULT_GAS_COST_USD: f64 = 0.60;

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) enum RunMode {
    #[default]
    Backtest,
    Microstructure,
    Sweep,
    Validate,
    Features,
}

#[derive(Debug, Clone)]
pub(crate) struct CliConfig {
    pub(crate) mode: RunMode,
    pub(crate) input: String,
    pub(crate) output: String,
    pub(crate) scan_stats_input: String,
    pub(crate) output_microstructure: Option<String>,
    pub(crate) output_features: Option<String>,
    pub(crate) market_id: Option<String>,
    pub(crate) confirmed_db: Option<String>,
    pub(crate) market_metadata: Option<String>,
    pub(crate) initial_balance: f64,
    pub(crate) min_profit_rate: f64,
    pub(crate) min_liquidity: f64,
    pub(crate) min_order_notional: f64,
    pub(crate) min_fee: f64,
    pub(crate) gas_cost_usd: f64,
    pub(crate) confirm_delay_us: i64,
    pub(crate) max_mem_gb: Option<f64>,
    pub(crate) sweep_min_profit_rates: Option<Vec<f64>>,
    pub(crate) sweep_min_liquidities: Option<Vec<f64>>,
    pub(crate) sweep_gas_costs: Option<Vec<f64>>,
}

pub(crate) fn parse_args() -> Result<CliConfig> {
    let mut mode = RunMode::Backtest;
    let mut input = "data/parquet/orderbook_snapshots_v1/date=*/hour=*/*.parquet".to_string();
    let mut output = "reports/potential_trades.csv".to_string();
    let mut scan_stats_input = "data/parquet/scan_stats_v1/date=*/hour=*/*.parquet".to_string();
    let mut output_microstructure = None;
    let mut output_features = None;
    let mut market_id = None;
    let mut confirmed_db = None;
    let mut market_metadata = None;
    let mut initial_balance = 1000.0;
    let mut min_profit_rate = 0.005;
    let mut min_liquidity = 0.0;
    let mut min_order_notional = 1.30;
    let mut min_fee = 0.50;
    let mut gas_cost_usd = DEFAULT_GAS_COST_USD;
    let mut confirm_delay_us = 3_000_000;
    let mut max_mem_gb = None;
    let mut sweep_min_profit_rates = None;
    let mut sweep_min_liquidities = None;
    let mut sweep_gas_costs = None;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--mode" => {
                let mode_str = args.next().context("--mode requires a value")?;
                mode = match mode_str.as_str() {
                    "backtest" | "simulate" => RunMode::Backtest,
                    "microstructure" => RunMode::Microstructure,
                    "sweep" => RunMode::Sweep,
                    "validate" | "validation" => RunMode::Validate,
                    "features" => RunMode::Features,
                    _ => return Err(anyhow!("unknown mode: {mode_str}")),
                };
            }
            "--input" => input = args.next().context("--input requires a value")?,
            "--output" => output = args.next().context("--output requires a value")?,
            "--scan-stats" => {
                scan_stats_input = args.next().context("--scan-stats requires a value")?
            }
            "--output-microstructure" => {
                output_microstructure = Some(
                    args.next()
                        .context("--output-microstructure requires a value")?,
                );
            }
            "--output-features" => {
                output_features = Some(args.next().context("--output-features requires a value")?);
            }
            "--market-id" => market_id = Some(args.next().context("--market-id requires a value")?),
            "--confirmed-db" => {
                confirmed_db = Some(args.next().context("--confirmed-db requires a value")?)
            }
            "--market-metadata" => {
                market_metadata = Some(args.next().context("--market-metadata requires a value")?)
            }
            "--initial-balance" => {
                initial_balance = args
                    .next()
                    .context("--initial-balance requires a value")?
                    .parse()
                    .context("invalid --initial-balance")?;
            }
            "--min-profit-rate" => {
                min_profit_rate = args
                    .next()
                    .context("--min-profit-rate requires a value")?
                    .parse()
                    .context("invalid --min-profit-rate")?;
            }
            "--min-liquidity" => {
                min_liquidity = args
                    .next()
                    .context("--min-liquidity requires a value")?
                    .parse()
                    .context("invalid --min-liquidity")?;
            }
            "--min-order-notional" => {
                min_order_notional = args
                    .next()
                    .context("--min-order-notional requires a value")?
                    .parse()
                    .context("invalid --min-order-notional")?;
            }
            "--min-fee" => {
                min_fee = args
                    .next()
                    .context("--min-fee requires a value")?
                    .parse()
                    .context("invalid --min-fee")?;
            }
            "--gas-cost-usd" => {
                gas_cost_usd = args
                    .next()
                    .context("--gas-cost-usd requires a value")?
                    .parse()
                    .context("invalid --gas-cost-usd")?;
            }
            "--max-mem-gb" => {
                max_mem_gb = Some(
                    args.next()
                        .context("--max-mem-gb requires a value")?
                        .parse()
                        .context("invalid --max-mem-gb")?,
                );
            }
            "--sweep-min-profit-rate" => {
                let values = parse_csv_f64(
                    &args
                        .next()
                        .context("--sweep-min-profit-rate requires a value")?,
                )
                .context("invalid --sweep-min-profit-rate")?;
                sweep_min_profit_rates = Some(values);
            }
            "--sweep-min-liquidity" => {
                let values = parse_csv_f64(
                    &args
                        .next()
                        .context("--sweep-min-liquidity requires a value")?,
                )
                .context("invalid --sweep-min-liquidity")?;
                sweep_min_liquidities = Some(values);
            }
            "--sweep-gas-costs" => {
                let values =
                    parse_csv_f64(&args.next().context("--sweep-gas-costs requires a value")?)
                        .context("invalid --sweep-gas-costs")?;
                sweep_gas_costs = Some(values);
            }
            "--confirm-delay-us" => {
                confirm_delay_us = args
                    .next()
                    .context("--confirm-delay-us requires a value")?
                    .parse()
                    .context("invalid --confirm-delay-us")?;
            }
            "--help" => {
                print_help();
                std::process::exit(0);
            }
            _ => return Err(anyhow!("unknown argument: {arg}")),
        }
    }

    Ok(CliConfig {
        mode,
        input,
        output,
        scan_stats_input,
        output_microstructure,
        output_features,
        market_id,
        confirmed_db,
        market_metadata,
        initial_balance,
        min_profit_rate,
        min_liquidity,
        min_order_notional,
        min_fee,
        gas_cost_usd,
        confirm_delay_us,
        max_mem_gb,
        sweep_min_profit_rates,
        sweep_min_liquidities,
        sweep_gas_costs,
    })
}

fn print_help() {
    println!(
        "research --mode <backtest|simulate|microstructure|sweep|validate|features> --input <glob> --output <csv>
    [--scan-stats <glob>] [--output-microstructure <csv>] [--output-features <csv>] [--market-id <id>] [--initial-balance <usdt>]
    [--min-profit-rate <rate>] [--min-liquidity <qty>] [--min-order-notional <usdt>]
    [--min-fee <usdt>] [--gas-cost-usd <usdt>] [--confirm-delay-us <micros>] [--max-mem-gb <gb>]
    [--sweep-min-profit-rate <csv>] [--sweep-min-liquidity <csv>] [--sweep-gas-costs <csv>]
    [--confirmed-db <path>] [--market-metadata <path>]\n\nDefaults:\n  --mode backtest\n  --input data/parquet/orderbook_snapshots_v1/date=*/hour=*/*.parquet\n  --output reports/potential_trades.csv\n  --scan-stats data/parquet/scan_stats_v1/date=*/hour=*/*.parquet\n  --initial-balance 1000\n  --min-profit-rate 0.005\n  --min-liquidity 0\n  --min-order-notional 1.30\n  --min-fee 0.50\n  --gas-cost-usd 0.60\n  --confirm-delay-us 3000000\n  --confirmed-db (unset)\n  --market-metadata (unset)"
    );
}

fn parse_csv_f64(raw: &str) -> Result<Vec<f64>> {
    raw.split(',')
        .map(|value| value.trim().parse().context("invalid float value"))
        .collect()
}
