use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpinionResponse<T> {
    #[serde(alias = "errno")]
    pub code: i32,
    #[serde(alias = "errmsg")]
    #[serde(default)]
    pub msg: String,
    pub result: Option<T>,
}

impl<T> OpinionResponse<T> {
    pub fn into_result(self) -> Result<T, String> {
        if self.code == 0 {
            match self.result {
                Some(data) => Ok(data),
                None => Err("Operation successful but returned no data".to_string()),
            }
        } else {
            Err(format!("[{}] {}", self.code, self.msg))
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderResponse {
    pub order_id: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserOrder {
    #[serde(rename = "orderId")]
    pub order_id: String,
    #[serde(rename = "marketId")]
    pub market_id: i64,
    #[serde(rename = "marketTitle")]
    pub market_title: String,
    #[serde(rename = "outcomeSide")]
    pub outcome_side: i32,
    #[serde(rename = "outcomeSideEnum")]
    pub outcome_side_enum: String,
    pub side: i32,
    #[serde(rename = "sideEnum")]
    pub side_enum: String,
    pub price: String,
    pub status: i32,
    #[serde(rename = "statusEnum")]
    pub status_enum: String,
    #[serde(rename = "orderAmount")]
    pub order_amount: String,
    #[serde(rename = "orderShares", default)]
    pub order_shares: String,
    #[serde(rename = "filledAmount")]
    pub filled_amount: String,
    #[serde(rename = "filledShares")]
    pub filled_shares: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "expiresAt")]
    pub expires_at: i64,
    #[serde(rename = "tradingMethod")]
    pub trading_method: i32,
    #[serde(rename = "tradingMethodEnum")]
    pub trading_method_enum: String,
    #[serde(rename = "quoteToken")]
    pub quote_token: String,
    // Optional fields - not always present in API responses
    #[serde(rename = "tokenId", default)]
    pub token_id: Option<String>,
    #[serde(default)]
    pub maker: Option<String>,
    #[serde(default)]
    pub signer: Option<String>,
    #[serde(default)]
    pub taker: Option<String>,
    #[serde(rename = "signatureType", default)]
    pub signature_type: Option<i32>,
    #[serde(default)]
    pub signature: Option<String>,
    #[serde(default)]
    pub nonce: Option<String>,
    #[serde(default)]
    pub salt: Option<String>,
    #[serde(default)]
    pub expiration: Option<String>,
    #[serde(default)]
    pub outcome: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderListResult {
    pub total: i64,
    pub list: Vec<UserOrder>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketListResult {
    pub total: i64,
    pub list: Vec<ApiMarket>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiMarket {
    pub market_id: i64,
    pub market_title: String,
    pub status: i32,
    pub status_enum: String,
    #[serde(default)]
    pub market_type: Option<i32>,
    #[serde(default)]
    pub yes_label: Option<String>,
    #[serde(default)]
    pub no_label: Option<String>,
    #[serde(default)]
    pub rules: Option<String>,
    #[serde(default)]
    pub yes_token_id: Option<String>,
    #[serde(default)]
    pub no_token_id: Option<String>,
    #[serde(default)]
    pub condition_id: Option<String>,
    #[serde(default)]
    pub result_token_id: Option<String>,
    #[serde(default)]
    pub volume: Option<String>,
    #[serde(default)]
    pub volume24h: Option<String>,
    #[serde(default)]
    pub volume7d: Option<String>,
    #[serde(default)]
    pub quote_token: Option<String>,
    #[serde(default)]
    pub chain_id: Option<String>,
    #[serde(default)]
    pub question_id: Option<String>,
    #[serde(default)]
    pub created_at: Option<i64>,
    #[serde(default)]
    pub cutoff_at: Option<i64>,
    #[serde(default)]
    pub resolved_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenOrderbook {
    pub market: Option<String>,
    pub token_id: String,
    pub timestamp: i64,
    pub bids: Vec<OrderbookEntry>,
    pub asks: Vec<OrderbookEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderbookEntry {
    pub price: String,
    pub size: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LatestPrice {
    pub token_id: String,
    pub price: String,
    pub side: String,
    pub size: String,
    pub timestamp: i64,
}

/// Response wrapper for quote token data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteTokenInfo {
    pub id: u64,
    #[serde(rename = "quoteTokenName")]
    pub quote_token_name: String,
    #[serde(rename = "quoteTokenAddress")]
    pub quote_token_address: String,
    #[serde(rename = "ctfExchangeAddress")]
    pub ctf_exchange_address: String,
    pub decimal: u64,
    pub symbol: String,
    #[serde(rename = "chainId")]
    pub chain_id: String,
}

/// Response wrapper for quote tokens list
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteTokenList {
    pub total: u64,
    pub list: Vec<QuoteTokenInfo>,
}

/// Response wrapper for user auth data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAuthInfo {
    #[serde(rename = "walletAddress")]
    pub wallet_address: String,
    #[serde(rename = "walletUsers")]
    pub wallet_users: std::collections::HashMap<String, String>,
}
