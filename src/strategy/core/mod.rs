use serde::{Deserialize, Serialize};

pub mod detector;
pub mod fees;
pub mod microstructure;
// risk module removed - functionality merged into execution::risk_manager

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketInfo {
    pub id: String,
    pub question: String,
    pub yes_token_id: String,
    pub no_token_id: String,
    pub yes_pair: String,
    pub no_pair: String,
    pub volume_24h: f64,
    pub liquidity: f64,
    pub is_active: bool,
    pub market_type: Option<i32>,
    pub root_market_id: Option<String>,
    pub parent_title: Option<String>,
    #[serde(default)]
    pub resolution_ts: Option<i64>,
}

impl MarketInfo {
    pub fn is_categorical(&self) -> bool {
        matches!(self.market_type, Some(1))
    }

    /// Returns normalized resolution timestamp in microseconds, if available.
    pub fn resolution_ts_us(&self) -> Option<i64> {
        self.resolution_ts
            .map(normalize_epoch_us)
            .filter(|ts| *ts > 0)
    }
}

fn normalize_epoch_us(ts: i64) -> i64 {
    if ts <= 0 {
        return 0;
    }
    if ts > 1_000_000_000_000_000 {
        ts
    } else if ts > 1_000_000_000_000 {
        ts.saturating_mul(1_000)
    } else {
        ts.saturating_mul(1_000_000)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderbookSide {
    pub price: f64,
    pub size: f64,
}

#[derive(Debug, Clone)]
pub struct MarketOrderbook {
    pub pair: String,
    pub bids: Vec<OrderbookSide>,
    pub asks: Vec<OrderbookSide>,
}

impl MarketOrderbook {
    pub fn best_bid(&self) -> Option<f64> {
        self.bids.first().map(|b| b.price)
    }

    pub fn best_ask(&self) -> Option<f64> {
        self.asks.first().map(|a| a.price)
    }

    pub fn spread(&self) -> Option<f64> {
        match (self.best_ask(), self.best_bid()) {
            (Some(ask), Some(bid)) => Some(ask - bid),
            _ => None,
        }
    }

    pub fn mid_price(&self) -> Option<f64> {
        match (self.best_ask(), self.best_bid()) {
            (Some(ask), Some(bid)) => Some((ask + bid) / 2.0),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TradeSide {
    Buy,
    Sell,
}

pub use detector::{
    ArbScanStats, ArbitrageConfig, ArbitrageDetector, ArbitrageLeg, ArbitrageOpportunity,
    ArbitrageType, OutcomeOrderbook,
};
pub use fees::{fee_amount, fee_rate, quote_fee, FeeConfig, FeeQuote};
pub use microstructure::MicrostructureFeature;
