use std::env;

use anyhow::{anyhow, Context, Result};

pub(crate) const DEFAULT_GAS_COST_USD: f64 = 0.60;

#[derive(Debug, Clone)]
pub(crate) struct CliConfig {
    pub(crate) input: String,
    pub(crate) output: String,
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
}

pub(crate) fn parse_args() -> Result<CliConfig> {
    let mut input = "data/parquet/orderbook_snapshots_v1/date=*/hour=*/*.parquet".to_string();
    let mut output = "reports/potential_trades.csv".to_string();
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

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--input" => input = args.next().context("--input requires a value")?,
            "--output" => output = args.next().context("--output requires a value")?,
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
        input,
        output,
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
    })
}

fn print_help() {
    println!(
        "research --input <glob> --output <csv> [--market-id <id>] [--initial-balance <usdt>]
    [--min-profit-rate <rate>] [--min-liquidity <qty>] [--min-order-notional <usdt>]
    [--min-fee <usdt>] [--gas-cost-usd <usdt>] [--confirm-delay-us <micros>]
    [--confirmed-db <path>] [--market-metadata <path>]\n\nDefaults:\n  --input data/parquet/orderbook_snapshots_v1/date=*/hour=*/*.parquet\n  --output reports/potential_trades.csv\n  --initial-balance 1000\n  --min-profit-rate 0.005\n  --min-liquidity 0\n  --min-order-notional 1.30\n  --min-fee 0.50\n  --gas-cost-usd 0.60\n  --confirm-delay-us 3000000\n  --confirmed-db (unset)\n  --market-metadata (unset)"
    );
}
