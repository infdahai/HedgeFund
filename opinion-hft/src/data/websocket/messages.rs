use serde::{Deserialize, Serialize};

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
}
