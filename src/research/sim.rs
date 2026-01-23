use tracing::{debug, info};

use crate::strategy::arbitrage::{
    ArbitrageConfig, ArbitrageDetector, ArbitrageOpportunity, ArbitrageType,
};
use crate::strategy::market_scanner::{MarketInfo, MarketOrderbook, OrderbookSide};

use super::config::CliConfig;
use super::types::{MarketReport, MarketTick, PendingTx, SimState, TradeRecord};

const LIQUIDITY_FILL_BUFFER: f64 = 0.8;
const MIN_ANNUAL_ROI: f64 = 0.20;
const MICROS_PER_DAY: f64 = 86_400.0 * 1_000_000.0;

pub(crate) fn simulate_market(
    market: &MarketInfo,
    ticks: &[MarketTick],
    config: &CliConfig,
    arb_config: &ArbitrageConfig,
) -> MarketReport {
    let mut detector = ArbitrageDetector::new(arb_config.clone());
    let mut state = SimState::new(config.initial_balance);
    let mut trades = Vec::new();
    let mut pending_signal = false;
    let market_id = market.id.as_str();

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
            if let Some(opportunity) = detect_best_opportunity(&mut detector, market, tick) {
                execute_trade(
                    &mut state,
                    market,
                    &opportunity,
                    current_ts,
                    config,
                    &mut trades,
                );
            }
            pending_signal = false;
        }

        if let Some(opportunity) = detect_best_opportunity(&mut detector, market, tick) {
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

    let duration_us = ticks
        .last()
        .map(|tick| tick.ts_us)
        .unwrap_or(0)
        .saturating_sub(ticks.first().map(|tick| tick.ts_us).unwrap_or(0));
    let avg_locked = if duration_us > 0 {
        state.locked_time_weighted_sum / duration_us as f64
    } else {
        0.0
    };
    let capital_turnover = if avg_locked > 0.0 {
        state.total_traded_notional / avg_locked
    } else {
        0.0
    };
    let return_on_capital_employed = if avg_locked > 0.0 {
        (state.balance - config.initial_balance) / avg_locked
    } else {
        0.0
    };

    MarketReport {
        market_id: market.id.clone(),
        trades,
        pnl: state.balance - config.initial_balance,
        total_fees: state.total_fees,
        max_drawdown: state.max_drawdown,
        total_trades: state.total_txs,
        win_rate,
        avg_confirm_latency_us,
        capital_turnover,
        return_on_capital_employed,
    }
}

pub(crate) fn build_market_info(market_id: &str, resolution_ts: Option<i64>) -> MarketInfo {
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
        resolution_ts,
    }
}

fn build_orderbook(pair: &str, book: &super::types::TickBook) -> MarketOrderbook {
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
    market: &MarketInfo,
    opportunity: &ArbitrageOpportunity,
    current_ts: i64,
    config: &CliConfig,
    trades: &mut Vec<TradeRecord>,
) {
    let min_fill_rate = min_fill_rate(opportunity);
    if min_fill_rate < 1.0 {
        debug!(
            market_id = %opportunity.market_id,
            fill_rate = min_fill_rate,
            "skipped opportunity due to fill rate"
        );
        return;
    }

    if let Some(lock_days) = resolution_lock_days(market, current_ts) {
        let annual_roi = annualized_roi(opportunity.net_profit_rate, lock_days);
        if annual_roi < MIN_ANNUAL_ROI {
            debug!(
                market_id = %opportunity.market_id,
                lock_days = lock_days,
                annual_roi = annual_roi,
                "skipped opportunity due to resolution lock"
            );
            return;
        }
    }

    let net_profit = opportunity.net_profit_abs - config.gas_cost_usd;
    if net_profit <= 0.0 {
        debug!(
            market_id = %opportunity.market_id,
            gross_profit = opportunity.net_profit_abs,
            gas_cost_usd = config.gas_cost_usd,
            "skipped opportunity due to gas cost"
        );
        return;
    }

    let total_spend = opportunity.total_spend;
    if state.balance < total_spend {
        return;
    }

    state.balance -= total_spend;
    state.locked_balance += total_spend;
    state.total_txs += 1;
    if net_profit > 0.0 {
        state.winning_txs += 1;
    }

    let notional = opportunity.total_cost * opportunity.max_size;
    let fee = (total_spend - notional).max(0.0);
    state.total_fees += fee;
    state.total_traded_notional += total_spend;

    let confirm_ts_us = current_ts + config.confirm_delay_us;
    info!(
        market_id = %opportunity.market_id,
        ts_us = current_ts,
        arb_type = ?opportunity.arb_type,
        total_spend = total_spend,
        expected_profit = net_profit,
        lock_until_us = confirm_ts_us,
        "submit simulated trade"
    );

    let pending = PendingTx {
        market_id: opportunity.market_id.clone(),
        submit_ts_us: current_ts,
        confirm_ts_us,
        cost: total_spend,
        expected_profit: net_profit,
    };
    state.unlock_ts_us = pending.confirm_ts_us;
    state.pending_txs.push(pending);

    trades.push(TradeRecord {
        market_id: opportunity.market_id.clone(),
        ts_us: current_ts,
        arb_type: arb_type_label(opportunity.arb_type),
        total_spend,
        expected_profit: net_profit,
        balance_after: state.balance,
    });

    for leg in &opportunity.legs {
        let entry = state.inventory.entry(leg.token_id.clone()).or_insert(0.0);
        match leg.side {
            crate::strategy::arbitrage::TradeSide::Buy => {
                *entry += leg.size;
            }
            crate::strategy::arbitrage::TradeSide::Sell => {
                *entry -= leg.size;
            }
        }
    }
}

fn min_fill_rate(opportunity: &ArbitrageOpportunity) -> f64 {
    let mut min_rate = f64::INFINITY;
    for leg in &opportunity.legs {
        if leg.size <= 0.0 {
            return 0.0;
        }
        let available = leg.available_liquidity * LIQUIDITY_FILL_BUFFER;
        let rate = if available <= 0.0 {
            0.0
        } else {
            available / leg.size
        };
        if rate < min_rate {
            min_rate = rate;
        }
    }
    if min_rate.is_finite() {
        min_rate
    } else {
        0.0
    }
}

fn resolution_lock_days(market: &MarketInfo, current_ts: i64) -> Option<f64> {
    let resolution_ts_us = market.resolution_ts_us()?;
    if resolution_ts_us <= current_ts {
        return None;
    }
    let delta_us = resolution_ts_us - current_ts;
    Some(delta_us as f64 / MICROS_PER_DAY)
}

fn annualized_roi(profit_rate: f64, lock_days: f64) -> f64 {
    if lock_days <= 0.0 {
        return 0.0;
    }
    profit_rate * (365.0 / lock_days)
}

fn apply_confirmations(state: &mut SimState, current_ts: i64) {
    if state.last_update_ts_us > 0 && current_ts > state.last_update_ts_us {
        let elapsed_us = current_ts - state.last_update_ts_us;
        state.locked_time_weighted_sum += state.locked_balance * elapsed_us as f64;
    }
    state.last_update_ts_us = current_ts;
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

fn arb_type_label(arb_type: ArbitrageType) -> String {
    match arb_type {
        ArbitrageType::YesNo => "YESNO",
        ArbitrageType::NegRisk => "NEG_RISK",
        ArbitrageType::SpreadAnomaly => "SPREAD",
        ArbitrageType::MintSell => "MINT_SELL",
    }
    .to_string()
}
