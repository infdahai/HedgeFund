use std::time::Instant;

use super::OrderIntent;

#[derive(Debug, Clone)]
pub struct MarketTick {
    pub pair: String,
    pub payload: String,
    pub ts_ms: i64,
}

#[derive(Debug, Clone)]
pub struct CrossChainSignal {
    pub source: String,
    pub intent: OrderIntent,
    pub received_at: Instant,
}

#[derive(Debug, Clone)]
pub struct OrderUpdate {
    pub order_id: String,
    pub status: String,
    pub shares: Option<String>,
    pub filled_shares: Option<String>,
}

#[derive(Debug, Clone)]
pub enum EventEnvelope {
    MarketData(MarketTick),
    Signal(CrossChainSignal),
    OrderUpdate(OrderUpdate),
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum StrategyCommand {
    SubmitOrder(OrderIntent),
    CancelOrder(String),
}
