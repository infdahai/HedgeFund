use std::collections::{BTreeMap, HashMap};
use std::env;
use std::fs;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use polars::prelude::*;
use rayon::prelude::*;
use tracing::{debug, info};

use opinion_hft::strategy::arbitrage::{
    ArbitrageConfig, ArbitrageDetector, ArbitrageOpportunity, ArbitrageType,
};
use opinion_hft::strategy::market_scanner::{MarketInfo, MarketOrderbook, OrderbookSide};

#[derive(Debug, Clone)]
struct CliConfig {
    input: String,
    output: String,
    market_id: Option<String>,
    initial_balance: f64,
    min_profit_rate: f64,
    min_liquidity: f64,
    min_order_notional: f64,
    min_fee: f64,
    confirm_delay_us: i64,
}

#[derive(Debug, Clone)]
struct SnapshotRow {
    market_id: String,
    outcome: String,
    ts_us: i64,
    best_bid: f64,
    best_ask: f64,
    bid_size: f64,
    ask_size: f64,
}

#[derive(Debug, Clone)]
struct TickBook {
    best_bid: f64,
    best_ask: f64,
    bid_size: f64,
    ask_size: f64,
}

#[derive(Debug, Clone)]
struct MarketTick {
    ts_us: i64,
    yes: TickBook,
    no: TickBook,
}

#[derive(Debug, Clone)]
struct TradeRecord {
    market_id: String,
    ts_us: i64,
    arb_type: String,
    total_spend: f64,
    expected_profit: f64,
    balance_after: f64,
}

#[derive(Debug, Clone)]
struct PendingTx {
    market_id: String,
    submit_ts_us: i64,
    confirm_ts_us: i64,
    cost: f64,
    expected_profit: f64,
}

#[derive(Debug, Clone)]
struct SimState {
    balance: f64,
    locked_balance: f64,
    inventory: HashMap<String, f64>,
    unlock_ts_us: i64,
    pending_txs: Vec<PendingTx>,
    total_txs: u64,
    total_fees: f64,
    winning_txs: u64,
    max_drawdown: f64,
    peak_balance: f64,
    confirmed_latency_us_total: i64,
    confirmed_count: u64,
    confirmed_by_market: HashMap<String, u64>,
}

impl SimState {
    fn new(initial_balance: f64) -> Self {
        Self {
            balance: initial_balance,
            locked_balance: 0.0,
            inventory: HashMap::new(),
            unlock_ts_us: 0,
            pending_txs: Vec::new(),
            total_txs: 0,
            total_fees: 0.0,
            winning_txs: 0,
            max_drawdown: 0.0,
            peak_balance: initial_balance,
            confirmed_latency_us_total: 0,
            confirmed_count: 0,
            confirmed_by_market: HashMap::new(),
        }
    }

    fn update_drawdown(&mut self) {
        if self.balance > self.peak_balance {
            self.peak_balance = self.balance;
        }
        if self.peak_balance > 0.0 {
            let drawdown = (self.peak_balance - self.balance) / self.peak_balance;
            if drawdown > self.max_drawdown {
                self.max_drawdown = drawdown;
            }
        }
    }
}

fn main() -> Result<()> {
    let _guards = opinion_hft::telemetry::init_telemetry();
    let config = parse_args()?;
    let df = load_snapshots(&config)?;
    let rows = rows_from_df(&df)?;
    let markets = build_market_ticks(rows);

    let arb_config = ArbitrageConfig {
        min_profit_rate: config.min_profit_rate,
        min_liquidity: config.min_liquidity,
        min_order_notional: config.min_order_notional,
        min_fee: config.min_fee,
        ..ArbitrageConfig::default()
    };

    let reports: Vec<MarketReport> = markets
        .par_iter()
        .map(|(market_id, ticks)| simulate_market(market_id, ticks, &config, &arb_config))
        .collect();

    let mut trades = Vec::new();
    for report in &reports {
        trades.extend(report.trades.clone());
    }

    write_trade_report(&config.output, &trades)?;
    print_summary(&reports, &trades, &config);

    Ok(())
}

fn parse_args() -> Result<CliConfig> {
    let mut input = "data/parquet/orderbook_snapshots_v1/date=*/hour=*/*.parquet".to_string();
    let mut output = "reports/potential_trades.csv".to_string();
    let mut market_id = None;
    let mut initial_balance = 1000.0;
    let mut min_profit_rate = 0.005;
    let mut min_liquidity = 0.0;
    let mut min_order_notional = 1.30;
    let mut min_fee = 0.50;
    let mut confirm_delay_us = 3_000_000;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--input" => input = args.next().context("--input requires a value")?,
            "--output" => output = args.next().context("--output requires a value")?,
            "--market-id" => market_id = Some(args.next().context("--market-id requires a value")?),
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
        initial_balance,
        min_profit_rate,
        min_liquidity,
        min_order_notional,
        min_fee,
        confirm_delay_us,
    })
}

fn print_help() {
    println!(
        "research --input <glob> --output <csv> [--market-id <id>] [--initial-balance <usdt>]
    [--min-profit-rate <rate>] [--min-liquidity <qty>] [--min-order-notional <usdt>]
    [--min-fee <usdt>] [--confirm-delay-us <micros>]\n\nDefaults:\n  --input data/parquet/orderbook_snapshots_v1/date=*/hour=*/*.parquet\n  --output reports/potential_trades.csv\n  --initial-balance 1000\n  --min-profit-rate 0.005\n  --min-liquidity 0\n  --min-order-notional 1.30\n  --min-fee 0.50\n  --confirm-delay-us 3000000"
    );
}

fn load_snapshots(config: &CliConfig) -> Result<DataFrame> {
    let mut lf = LazyFrame::scan_parquet(&config.input, Default::default())?;
    if let Some(ref market_id) = config.market_id {
        lf = lf.filter(col("market_id").eq(lit(market_id.as_str())));
    }
    let df = lf.collect()?;
    Ok(df)
}

fn rows_from_df(df: &DataFrame) -> Result<Vec<SnapshotRow>> {
    let names = df.get_column_names();
    let idx_market = find_index(&names, "market_id")?;
    let idx_local_recv = find_index_optional(&names, "local_recv_ts_us");
    let idx_snapshot_ts = find_index_optional(&names, "snapshot_ts");
    if idx_local_recv.is_none() && idx_snapshot_ts.is_none() {
        return Err(anyhow!("missing timestamp column (expected local_recv_ts_us or snapshot_ts)"));
    }
    let idx_best_bid = find_index_optional(&names, "best_bid");
    let idx_best_ask = find_index_optional(&names, "best_ask");
    let idx_bid_size = find_index_optional(&names, "bid_size");
    let idx_ask_size = find_index_optional(&names, "ask_size");
    let idx_bids = find_index_optional(&names, "bids");
    let idx_asks = find_index_optional(&names, "asks");
    let idx_outcome = find_index_optional(&names, "outcome");
    let idx_pair = find_index_optional(&names, "pair");

    let mut rows = Vec::with_capacity(df.height());
    for idx in 0..df.height() {
        let row = df.get_row(idx)?;
        let values = row.0;

        let market_id = any_to_string(&values[idx_market])
            .ok_or_else(|| anyhow!("invalid market_id"))?;
        let ts_us = if let Some(idx) = idx_local_recv {
            any_to_i64(&values[idx]).ok_or_else(|| anyhow!("invalid local_recv_ts_us"))?
        } else if let Some(idx) = idx_snapshot_ts {
            any_to_i64(&values[idx]).ok_or_else(|| anyhow!("invalid snapshot_ts"))? * 1000
        } else {
            continue;
        };

        let (best_bid, bid_size) = if let Some(idx) = idx_best_bid {
            let price = any_to_f64(&values[idx]).ok_or_else(|| anyhow!("invalid best_bid"))?;
            let size = idx_bid_size
                .and_then(|size_idx| any_to_f64(&values[size_idx]))
                .unwrap_or(0.0);
            (price, size)
        } else if let Some(idx) = idx_bids {
            extract_top_from_list(&values[idx]).ok_or_else(|| anyhow!("missing bids list"))?
        } else {
            continue;
        };

        let (best_ask, ask_size) = if let Some(idx) = idx_best_ask {
            let price = any_to_f64(&values[idx]).ok_or_else(|| anyhow!("invalid best_ask"))?;
            let size = idx_ask_size
                .and_then(|size_idx| any_to_f64(&values[size_idx]))
                .unwrap_or(0.0);
            (price, size)
        } else if let Some(idx) = idx_asks {
            extract_top_from_list(&values[idx]).ok_or_else(|| anyhow!("missing asks list"))?
        } else {
            continue;
        };

        let outcome = idx_outcome
            .and_then(|idx| any_to_string(&values[idx]))
            .or_else(|| idx_pair.and_then(|idx| any_to_string(&values[idx]).and_then(derive_outcome)))
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

fn build_market_ticks(rows: Vec<SnapshotRow>) -> HashMap<String, Vec<MarketTick>> {
    let mut markets: HashMap<String, BTreeMap<i64, TickAccumulator>> = HashMap::new();

    for row in rows {
        let tick = markets
            .entry(row.market_id.clone())
            .or_insert_with(BTreeMap::new)
            .entry(row.ts_us)
            .or_insert_with(TickAccumulator::default);

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

#[derive(Default)]
struct TickAccumulator {
    yes: Option<TickBook>,
    no: Option<TickBook>,
}

#[derive(Debug, Clone)]
struct MarketReport {
    market_id: String,
    trades: Vec<TradeRecord>,
    pnl: f64,
    total_fees: f64,
    max_drawdown: f64,
    total_trades: u64,
    win_rate: f64,
    avg_confirm_latency_us: f64,
}

fn simulate_market(
    market_id: &str,
    ticks: &[MarketTick],
    config: &CliConfig,
    arb_config: &ArbitrageConfig,
) -> MarketReport {
    let mut detector = ArbitrageDetector::new(arb_config.clone());
    let mut state = SimState::new(config.initial_balance);
    let mut trades = Vec::new();
    let mut pending_signal = false;

    let market_info = build_market_info(market_id);

    for tick in ticks.iter() {
        let current_ts = tick.ts_us;

        apply_confirmations(&mut state, current_ts);

        if !state.pending_txs.is_empty() || current_ts < state.unlock_ts_us {
            if pending_signal {
                debug!(
                    market_id = %market_id,
                    ts_us = current_ts,
                    unlock_ts_us = state.unlock_ts_us,
                    "skipped opportunity due to lock"
                );
            }
            pending_signal = false;
            continue;
        }

        if pending_signal {
            if let Some(opportunity) = detect_best_opportunity(&mut detector, &market_info, tick) {
                execute_trade(&mut state, &opportunity, current_ts, config, &mut trades);
            }
            pending_signal = false;
        }

        if let Some(opportunity) = detect_best_opportunity(&mut detector, &market_info, tick) {
            debug!(
                market_id = %market_id,
                ts_us = current_ts,
                arb_type = ?opportunity.arb_type,
                expected_profit = opportunity.net_profit_abs,
                "signal detected; scheduling next-tick execution"
            );
            pending_signal = true;
        }
    }

    let win_rate = if state.total_txs > 0 {
        state.winning_txs as f64 / state.total_txs as f64
    } else {
        0.0
    };

    let avg_confirm_latency_us = if state.confirmed_count > 0 {
        state.confirmed_latency_us_total as f64 / state.confirmed_count as f64
    } else {
        0.0
    };

    MarketReport {
        market_id: market_id.to_string(),
        trades,
        pnl: state.balance - config.initial_balance,
        total_fees: state.total_fees,
        max_drawdown: state.max_drawdown,
        total_trades: state.total_txs,
        win_rate,
        avg_confirm_latency_us,
    }
}

fn build_market_info(market_id: &str) -> MarketInfo {
    MarketInfo {
        id: market_id.to_string(),
        question: "".to_string(),
        yes_token_id: "0".to_string(),
        no_token_id: "0".to_string(),
        yes_pair: format!("{market_id}-YES"),
        no_pair: format!("{market_id}-NO"),
        volume_24h: 0.0,
        liquidity: 0.0,
        is_active: true,
        market_type: None,
        root_market_id: None,
        parent_title: None,
    }
}

fn build_orderbook(pair: &str, book: &TickBook) -> MarketOrderbook {
    MarketOrderbook {
        pair: pair.to_string(),
        bids: vec![OrderbookSide {
            price: book.best_bid,
            size: book.bid_size,
        }],
        asks: vec![OrderbookSide {
            price: book.best_ask,
            size: book.ask_size,
        }],
    }
}

fn detect_best_opportunity(
    detector: &mut ArbitrageDetector,
    market: &MarketInfo,
    tick: &MarketTick,
) -> Option<ArbitrageOpportunity> {
    let yes_ob = build_orderbook(&market.yes_pair, &tick.yes);
    let no_ob = build_orderbook(&market.no_pair, &tick.no);

    let mut opportunities = Vec::new();
    if let Some(opportunity) = detector.detect_yesno_arbitrage(market, &yes_ob, &no_ob) {
        opportunities.push(opportunity);
    }
    if let Some(opportunity) = detector.detect_mint_sell_arbitrage(market, &yes_ob, &no_ob) {
        opportunities.push(opportunity);
    }

    opportunities
        .into_iter()
        .max_by(|a, b| a.net_profit_abs.partial_cmp(&b.net_profit_abs).unwrap_or(std::cmp::Ordering::Equal))
}

fn execute_trade(
    state: &mut SimState,
    opportunity: &ArbitrageOpportunity,
    current_ts: i64,
    config: &CliConfig,
    trades: &mut Vec<TradeRecord>,
) {
    let total_spend = opportunity.total_spend;
    if state.balance < total_spend {
        return;
    }

    state.balance -= total_spend;
    state.locked_balance += total_spend;
    state.total_txs += 1;
    if opportunity.net_profit_abs > 0.0 {
        state.winning_txs += 1;
    }

    let notional = opportunity.total_cost * opportunity.max_size;
    let fee = (total_spend - notional).max(0.0);
    state.total_fees += fee;

    let confirm_ts_us = current_ts + config.confirm_delay_us;
    info!(
        market_id = %opportunity.market_id,
        ts_us = current_ts,
        arb_type = ?opportunity.arb_type,
        total_spend = total_spend,
        expected_profit = opportunity.net_profit_abs,
        lock_until_us = confirm_ts_us,
        "submit simulated trade"
    );

    let pending = PendingTx {
        market_id: opportunity.market_id.clone(),
        submit_ts_us: current_ts,
        confirm_ts_us,
        cost: total_spend,
        expected_profit: opportunity.net_profit_abs,
    };
    state.unlock_ts_us = pending.confirm_ts_us;
    state.pending_txs.push(pending);

    trades.push(TradeRecord {
        market_id: opportunity.market_id.clone(),
        ts_us: current_ts,
        arb_type: arb_type_label(opportunity.arb_type),
        total_spend,
        expected_profit: opportunity.net_profit_abs,
        balance_after: state.balance,
    });

    for leg in &opportunity.legs {
        let entry = state.inventory.entry(leg.token_id.clone()).or_insert(0.0);
        match leg.side {
            opinion_hft::strategy::arbitrage::TradeSide::Buy => {
                *entry += leg.size;
            }
            opinion_hft::strategy::arbitrage::TradeSide::Sell => {
                *entry -= leg.size;
            }
        }
    }
}

fn apply_confirmations(state: &mut SimState, current_ts: i64) {
    let mut confirmed_profit = 0.0;
    let mut confirmed_cost = 0.0;
    state.pending_txs.retain(|tx| {
        if current_ts >= tx.confirm_ts_us {
            let latency = current_ts - tx.submit_ts_us;
            state.confirmed_latency_us_total += latency;
            state.confirmed_count += 1;
            *state
                .confirmed_by_market
                .entry(tx.market_id.clone())
                .or_insert(0) += 1;
            info!(
                market_id = %tx.market_id,
                confirm_ts_us = current_ts,
                submit_ts_us = tx.submit_ts_us,
                latency_us = latency,
                expected_profit = tx.expected_profit,
                "confirmed simulated trade"
            );
            confirmed_profit += tx.expected_profit;
            confirmed_cost += tx.cost;
            false
        } else {
            true
        }
    });

    if confirmed_cost > 0.0 {
        state.locked_balance -= confirmed_cost;
        state.balance += confirmed_cost + confirmed_profit;
        state.update_drawdown();
    }

    if state.pending_txs.is_empty() {
        state.unlock_ts_us = current_ts;
    }
}

fn print_summary(reports: &[MarketReport], trades: &[TradeRecord], config: &CliConfig) {
    let total_trades: u64 = reports.iter().map(|r| r.total_trades).sum();
    let total_pnl: f64 = reports.iter().map(|r| r.pnl).sum();
    let total_fees: f64 = reports.iter().map(|r| r.total_fees).sum();
    let max_drawdown = reports
        .iter()
        .map(|r| r.max_drawdown)
        .fold(0.0, f64::max);
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

    for report in reports.iter().filter(|r| r.total_trades > 0) {
        println!(
            "  market {} trades {} win_rate {:.2} pnl {:.6}",
            report.market_id,
            report.total_trades,
            report.win_rate,
            report.pnl
        );
    }
}

fn write_trade_report(path: &str, trades: &[TradeRecord]) -> Result<()> {
    let output_path = Path::new(path);
    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).with_context(|| format!("create output dir {parent:?}"))?;
        }
    }

    let mut contents = String::from("market_id,ts_us,arb_type,total_spend,expected_profit,balance_after\n");
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

fn find_index(names: &[&PlSmallStr], target: &str) -> Result<usize> {
    names
        .iter()
        .position(|name| name.as_str() == target)
        .ok_or_else(|| anyhow!("missing column {target}"))
}

fn find_index_optional(names: &[&PlSmallStr], target: &str) -> Option<usize> {
    names.iter().position(|name| name.as_str() == target)
}

fn any_to_string(value: &AnyValue) -> Option<String> {
    match value {
        AnyValue::String(value) => Some(value.to_string()),
        AnyValue::StringOwned(value) => Some(value.to_string()),
        _ => None,
    }
}

fn any_to_i64(value: &AnyValue) -> Option<i64> {
    match value {
        AnyValue::Int64(value) => Some(*value),
        AnyValue::Int32(value) => Some(*value as i64),
        AnyValue::UInt64(value) => Some(*value as i64),
        AnyValue::UInt32(value) => Some(*value as i64),
        AnyValue::Float64(value) => Some(*value as i64),
        AnyValue::Float32(value) => Some(*value as i64),
        _ => None,
    }
}

fn any_to_f64(value: &AnyValue) -> Option<f64> {
    match value {
        AnyValue::Float64(value) => Some(*value),
        AnyValue::Float32(value) => Some(*value as f64),
        AnyValue::Int64(value) => Some(*value as f64),
        AnyValue::Int32(value) => Some(*value as f64),
        AnyValue::UInt64(value) => Some(*value as f64),
        AnyValue::UInt32(value) => Some(*value as f64),
        _ => None,
    }
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

fn extract_top_from_list(value: &AnyValue) -> Option<(f64, f64)> {
    match value {
        AnyValue::List(series) => {
            if series.len() == 0 {
                return None;
            }
            let struct_series = series.struct_().ok()?;
            let price_series = struct_series.field_by_name("p").ok()?;
            let size_series = struct_series.field_by_name("s").ok()?;
            let price = price_series.get(0).ok()?;
            let size = size_series.get(0).ok()?;
            Some((any_to_f64(&price)?, any_to_f64(&size)?))
        }
        _ => None,
    }
}

fn round_price(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

fn arb_type_label(arb_type: ArbitrageType) -> String {
    match arb_type {
        ArbitrageType::YesNo => "YESNO",
        ArbitrageType::NegRisk => "NEG_RISK",
        ArbitrageType::SpreadAnomaly => "SPREAD",
        ArbitrageType::MintSell => "MINT_SELL",
    }
    .to_string()
}
