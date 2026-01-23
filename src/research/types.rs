use std::collections::HashMap;

#[derive(Debug, Clone)]
pub(crate) struct SnapshotRow {
    pub(crate) market_id: String,
    pub(crate) outcome: String,
    pub(crate) ts_us: i64,
    pub(crate) best_bid: f64,
    pub(crate) best_ask: f64,
    pub(crate) bid_size: f64,
    pub(crate) ask_size: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct TickBook {
    pub(crate) best_bid: f64,
    pub(crate) best_ask: f64,
    pub(crate) bid_size: f64,
    pub(crate) ask_size: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct MarketTick {
    pub(crate) ts_us: i64,
    pub(crate) yes: TickBook,
    pub(crate) no: TickBook,
}

#[derive(Debug, Clone)]
pub(crate) struct TradeRecord {
    pub(crate) market_id: String,
    pub(crate) ts_us: i64,
    pub(crate) arb_type: String,
    pub(crate) total_spend: f64,
    pub(crate) expected_profit: f64,
    pub(crate) balance_after: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct ConfirmedOrderRecord {
    pub(crate) market_id: String,
    pub(crate) side: String,
    pub(crate) expected_price: f64,
    pub(crate) actual_fill: String,
    pub(crate) confirmation_time_ms: u64,
    pub(crate) timestamp_ms: i64,
}

#[derive(Debug, Clone)]
pub(crate) struct MicrostructureReport {
    pub(crate) market_id: String,
    pub(crate) liquidity_availability_pct: f64,
    pub(crate) scan_skip_rate: f64,
    pub(crate) spread_p50: f64,
    pub(crate) spread_p95: f64,
    pub(crate) slippage_100_shares: f64,
    pub(crate) opportunity_frequency_per_hour: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct PendingTx {
    pub(crate) market_id: String,
    pub(crate) submit_ts_us: i64,
    pub(crate) confirm_ts_us: i64,
    pub(crate) cost: f64,
    pub(crate) expected_profit: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct SimState {
    pub(crate) balance: f64,
    pub(crate) locked_balance: f64,
    pub(crate) inventory: HashMap<String, f64>,
    pub(crate) unlock_ts_us: i64,
    pub(crate) pending_txs: Vec<PendingTx>,
    pub(crate) total_txs: u64,
    pub(crate) total_fees: f64,
    pub(crate) winning_txs: u64,
    pub(crate) max_drawdown: f64,
    pub(crate) peak_balance: f64,
    pub(crate) confirmed_latency_us_total: i64,
    pub(crate) confirmed_count: u64,
    pub(crate) confirmed_by_market: HashMap<String, u64>,
    pub(crate) total_traded_notional: f64,
    pub(crate) locked_time_weighted_sum: f64,
    pub(crate) last_update_ts_us: i64,
}

impl SimState {
    pub(crate) fn new(initial_balance: f64) -> Self {
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
            total_traded_notional: 0.0,
            locked_time_weighted_sum: 0.0,
            last_update_ts_us: 0,
        }
    }

    pub(crate) fn update_drawdown(&mut self) {
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

#[derive(Debug, Clone)]
pub(crate) struct MarketReport {
    pub(crate) market_id: String,
    pub(crate) trades: Vec<TradeRecord>,
    pub(crate) pnl: f64,
    pub(crate) total_fees: f64,
    pub(crate) max_drawdown: f64,
    pub(crate) total_trades: u64,
    pub(crate) win_rate: f64,
    pub(crate) avg_confirm_latency_us: f64,
    pub(crate) capital_turnover: f64,
    pub(crate) return_on_capital_employed: f64,
}
