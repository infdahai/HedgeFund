#[cfg(not(feature = "polars"))]
fn main() -> anyhow::Result<()> {
    Err(anyhow::anyhow!(
        "research binary requires --features polars"
    ))
}

#[cfg(feature = "polars")]
mod research_app {
    use std::collections::{BTreeMap, HashMap};
    use std::env;
    use std::fs;
    use std::path::Path;
    use std::time::Duration;

    use anyhow::{anyhow, Context, Result};
    use glob::glob;
    use polars::prelude::*;
    use rayon::prelude::*;
    use rusqlite::Connection;
    use tracing::{debug, info, warn};

    use opinion_hft::strategy::arbitrage::{
        ArbitrageConfig, ArbitrageDetector, ArbitrageOpportunity, ArbitrageType,
    };
    use opinion_hft::strategy::market_scanner::{MarketInfo, MarketOrderbook, OrderbookSide};

    pub fn main() -> Result<()> {
        let _guards = opinion_hft::telemetry::init_telemetry();
        let config = parse_args()?;
        let df = load_snapshots(&config)?;
        let rows = rows_from_df(&df)?;
        let markets = build_market_ticks(rows);
        let micro_reports = build_microstructure_reports(&markets, &config);

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
        let confirmed = load_confirmed_orders(config.confirmed_db.as_deref()).unwrap_or_default();
        print_summary(&reports, &trades, &micro_reports, &confirmed, &config);

        Ok(())
    }

    #[derive(Debug, Clone)]
    struct CliConfig {
        input: String,
        output: String,
        market_id: Option<String>,
        confirmed_db: Option<String>,
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
    struct ConfirmedOrderRecord {
        market_id: String,
        side: String,
        expected_price: f64,
        actual_fill: String,
        confirmation_time_ms: u64,
        timestamp_ms: i64,
    }

    #[derive(Debug, Clone)]
    struct MicrostructureReport {
        market_id: String,
        liquidity_availability_pct: f64,
        spread_p50: f64,
        spread_p95: f64,
        slippage_100_shares: f64,
        opportunity_frequency_per_hour: f64,
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

    fn parse_args() -> Result<CliConfig> {
        let mut input = "data/parquet/orderbook_snapshots_v1/date=*/hour=*/*.parquet".to_string();
        let mut output = "reports/potential_trades.csv".to_string();
        let mut market_id = None;
        let mut confirmed_db = None;
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
                "--market-id" => {
                    market_id = Some(args.next().context("--market-id requires a value")?)
                }
                "--confirmed-db" => {
                    confirmed_db = Some(args.next().context("--confirmed-db requires a value")?)
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
    [--min-fee <usdt>] [--confirm-delay-us <micros>] [--confirmed-db <path>]\n\nDefaults:\n  --input data/parquet/orderbook_snapshots_v1/date=*/hour=*/*.parquet\n  --output reports/potential_trades.csv\n  --initial-balance 1000\n  --min-profit-rate 0.005\n  --min-liquidity 0\n  --min-order-notional 1.30\n  --min-fee 0.50\n  --confirm-delay-us 3000000\n  --confirmed-db (unset)"
    );
    }

    fn load_snapshots(config: &CliConfig) -> Result<DataFrame> {
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
                    warn!("skip corrupt parquet file", path = ?path, error = %err);
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
            warn!("parquet files skipped", skipped = skipped);
        }
        if not_ready > 0 {
            warn!("parquet partitions not ready", skipped = not_ready);
        }

        let df = concat(frames, UnionArgs::default())?
            .with_streaming(true)
            .collect()?;
        Ok(df)
    }

    fn rows_from_df(df: &DataFrame) -> Result<Vec<SnapshotRow>> {
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

    fn partition_ready(path: &Path) -> bool {
        let Some(parent) = path.parent() else {
            return false;
        };
        let success = parent.join("_SUCCESS");
        success.exists()
    }

    fn build_market_ticks(rows: Vec<SnapshotRow>) -> HashMap<String, Vec<MarketTick>> {
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
                if let Some(opportunity) =
                    detect_best_opportunity(&mut detector, &market_info, tick)
                {
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
        let detected_at = if tick.ts_us <= 0 {
            0
        } else {
            (tick.ts_us as u64) / 1_000_000
        };

        let mut opportunities = Vec::new();
        if let Some(opportunity) = detector.detect_yesno_arbitrage(
            market,
            &yes_ob.bids,
            &yes_ob.asks,
            &no_ob.bids,
            &no_ob.asks,
            detected_at,
        ) {
            opportunities.push(opportunity);
        }
        if let Some(opportunity) = detector.detect_mint_sell_arbitrage(
            market,
            &yes_ob.bids,
            &yes_ob.asks,
            &no_ob.bids,
            &no_ob.asks,
            detected_at,
        ) {
            opportunities.push(opportunity);
        }

        opportunities.into_iter().max_by(|a, b| {
            a.net_profit_abs
                .partial_cmp(&b.net_profit_abs)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
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

    fn print_summary(
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
                .collect::<std::collections::HashSet<_>>()
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

    fn write_trade_report(path: &str, trades: &[TradeRecord]) -> Result<()> {
        let output_path = Path::new(path);
        if let Some(parent) = output_path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("create output dir {parent:?}"))?;
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

    fn arb_type_label(arb_type: ArbitrageType) -> String {
        match arb_type {
            ArbitrageType::YesNo => "YESNO",
            ArbitrageType::NegRisk => "NEG_RISK",
            ArbitrageType::SpreadAnomaly => "SPREAD",
            ArbitrageType::MintSell => "MINT_SELL",
        }
        .to_string()
    }

    fn build_microstructure_reports(
        markets: &HashMap<String, Vec<MarketTick>>,
        config: &CliConfig,
    ) -> Vec<MicrostructureReport> {
        markets
            .iter()
            .map(|(market_id, ticks)| MicroTotals::from_ticks(market_id, ticks, config))
            .collect()
    }

    #[derive(Default)]
    struct MicroTotals {
        total_ticks: u64,
        liquid_ticks: u64,
        spreads: Vec<f64>,
        slippage_rates: Vec<f64>,
        opportunity_ticks: u64,
        first_ts_us: Option<i64>,
        last_ts_us: Option<i64>,
    }

    impl MicroTotals {
        fn from_ticks(
            market_id: &str,
            ticks: &[MarketTick],
            config: &CliConfig,
        ) -> MicrostructureReport {
            let mut totals = MicroTotals::default();
            for tick in ticks {
                totals.total_ticks += 1;
                totals.first_ts_us = Some(
                    totals
                        .first_ts_us
                        .map_or(tick.ts_us, |value| value.min(tick.ts_us)),
                );
                totals.last_ts_us = Some(
                    totals
                        .last_ts_us
                        .map_or(tick.ts_us, |value| value.max(tick.ts_us)),
                );

                let yes = &tick.yes;
                let no = &tick.no;
                if yes.best_bid > 0.0
                    && yes.best_ask > 0.0
                    && no.best_bid > 0.0
                    && no.best_ask > 0.0
                {
                    totals.liquid_ticks += 1;
                    let spread_yes = (yes.best_ask - yes.best_bid).max(0.0);
                    let spread_no = (no.best_ask - no.best_bid).max(0.0);
                    totals.spreads.push((spread_yes + spread_no) / 2.0);

                    let slippage_yes = slippage_rate_for_size(
                        100.0,
                        yes.best_bid,
                        yes.best_ask,
                        yes.bid_size,
                        yes.ask_size,
                    );
                    let slippage_no = slippage_rate_for_size(
                        100.0,
                        no.best_bid,
                        no.best_ask,
                        no.bid_size,
                        no.ask_size,
                    );
                    totals
                        .slippage_rates
                        .push((slippage_yes + slippage_no) / 2.0);
                }

                if yes.best_ask > 0.0 && no.best_ask > 0.0 {
                    if yes.best_ask + no.best_ask < 1.0 - config.min_profit_rate {
                        totals.opportunity_ticks += 1;
                    }
                }
            }

            totals.finish(market_id.to_string())
        }

        fn finish(mut self, market_id: String) -> MicrostructureReport {
            let liquidity_pct = if self.total_ticks > 0 {
                self.liquid_ticks as f64 / self.total_ticks as f64 * 100.0
            } else {
                0.0
            };

            let spread_p50 = percentile(&mut self.spreads, 0.50);
            let spread_p95 = percentile(&mut self.spreads, 0.95);
            let slippage_100 = percentile(&mut self.slippage_rates, 0.50);

            let hours = match (self.first_ts_us, self.last_ts_us) {
                (Some(start), Some(end)) if end > start => {
                    Duration::from_micros((end - start) as u64).as_secs_f64() / 3600.0
                }
                _ => 0.0,
            };
            let opp_frequency = if hours > 0.0 {
                self.opportunity_ticks as f64 / hours
            } else {
                0.0
            };

            MicrostructureReport {
                market_id,
                liquidity_availability_pct: liquidity_pct,
                spread_p50,
                spread_p95,
                slippage_100_shares: slippage_100,
                opportunity_frequency_per_hour: opp_frequency,
            }
        }
    }

    fn slippage_rate_for_size(
        size: f64,
        best_bid: f64,
        best_ask: f64,
        bid_size: f64,
        ask_size: f64,
    ) -> f64 {
        if size <= 0.0 || best_bid <= 0.0 || best_ask <= 0.0 {
            return 0.0;
        }
        let spread = (best_ask - best_bid).max(0.0);
        let base = spread / best_ask;
        let depth = ask_size.min(bid_size);
        if depth <= 0.0 {
            return base;
        }
        let impact = (size / depth).max(0.0) * 0.01;
        base + impact
    }

    fn percentile(values: &mut Vec<f64>, quantile: f64) -> f64 {
        if values.is_empty() {
            return 0.0;
        }
        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let idx = ((values.len() - 1) as f64 * quantile).round() as usize;
        values.get(idx).copied().unwrap_or(0.0)
    }

    fn load_confirmed_orders(db_path: Option<&str>) -> Result<Vec<ConfirmedOrderRecord>> {
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

    pub fn run() -> Result<()> {
        main()
    }
}

#[cfg(feature = "polars")]
fn main() -> anyhow::Result<()> {
    research_app::run()
}
