use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use simd_json;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WsMessage {
    #[serde(rename = "subscribe")]
    Subscribe { channel: String },
    #[serde(rename = "unsubscribe")]
    Unsubscribe { channel: String },
    #[serde(rename = "ping")]
    Ping,
    #[serde(rename = "pong")]
    Pong,
    #[serde(rename = "trade.order.update")]
    OrderUpdate(OrderUpdate),
    #[serde(rename = "trade.record.new")]
    TradeRecord(TradeRecord),
}

impl WsMessage {
    pub fn parse(data: &mut [u8]) -> Result<Self> {
        simd_json::from_slice(data).map_err(|e| Error::ParseError(e.to_string()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderUpdate {
    pub order_id: String,
    pub status: String,
    pub shares: Option<String>,
    pub filled_shares: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeRecord {
    pub order_id: String,
    pub tx_hash: Option<String>,
    pub amount: Option<String>,
    pub price: Option<String>,
    pub avg_price: Option<String>,
}
