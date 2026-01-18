use serde::{de::DeserializeOwned, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use rquest::{Client, Proxy};
use rquest_util::Emulation;

use crate::config::{ApiConfig, Environment};
use crate::error::{Error, Result};
use super::models::{
    OpinionResponse, MarketListResult, MarketDetailResult, ApiMarket, TokenOrderbook, LatestPrice,
    OrderListResult, UserOrder, QuoteTokenInfo, QuoteTokenList, UserAuthInfo, PriceHistoryResult,
    OrderDataWrapper
};

#[derive(Clone)]
pub struct OpinionClient {
    client: Client,
    base_url: String,
    api_key: String,
    cached_exchange_address: Arc<Mutex<Option<String>>>,
    cached_proxy_address: Arc<Mutex<Option<String>>>,
}

impl OpinionClient {
    pub fn new(config: &ApiConfig, environment: Environment) -> Result<Self> {
        let mut builder = Client::builder()
            .emulation(Emulation::Chrome120)
            .timeout(std::time::Duration::from_secs(30));

        // Configure proxy based on environment
        let mode_desc = match environment {
            Environment::Local => {
                // Local: Use proxy to bypass Kong Gateway TLS fingerprinting
                if let Ok(proxy_url) = std::env::var("OPINION_PROXY_URL") {
                    tracing::info!("Local environment: using proxy {}", proxy_url);
                    if let Ok(proxy) = Proxy::all(&proxy_url) {
                        builder = builder.proxy(proxy);
                        "proxy"
                    } else {
                        tracing::warn!("Failed to parse proxy URL, using direct connection");
                        "direct"
                    }
                } else {
                    tracing::warn!("OPINION_PROXY_URL not set, using direct connection");
                    "direct"
                }
            }
            Environment::Aws => {
                // AWS: Direct connection, no proxy needed
                tracing::info!("AWS environment: using direct connection");
                "direct"
            }
        };

        let client = builder
            .build()
            .map_err(|e| Error::InternalError(format!("failed to build rquest client: {}", e)))?;

        tracing::info!("OpinionClient initialized: {} mode (Chrome120 TLS emulation)", mode_desc);

        Ok(Self {
            client,
            base_url: config.rest_base_url.trim_end_matches('/').to_string(),
            api_key: config.api_key.clone(),
            cached_exchange_address: Arc::new(Mutex::new(None)),
            cached_proxy_address: Arc::new(Mutex::new(None)),
        })
    }

    fn build_url(&self, endpoint: &str) -> String {
        format!("{}/{}", self.base_url, endpoint)
    }

    pub async fn init(&self, chain_id: u64, quote_token: &str) -> Result<()> {
        let exchange_address = self.get_exchange_address(chain_id, quote_token).await?;
        *self.cached_exchange_address.lock().await = Some(exchange_address);

        let proxy_address = self.get_proxy_address(chain_id).await?;
        *self.cached_proxy_address.lock().await = proxy_address;

        Ok(())
    }

    pub async fn get_cached_exchange_address(&self) -> Option<String> {
        self.cached_exchange_address.lock().await.clone()
    }

    pub async fn get_cached_proxy_address(&self) -> Option<String> {
        self.cached_proxy_address.lock().await.clone()
    }

    async fn parse_response<T: DeserializeOwned>(&self, response: rquest::Response) -> Result<T> {
        let status = response.status();
        if !status.is_success() {
            return Err(Error::ApiError {
                status: status.as_u16(),
                message: format!("HTTP error {}", status),
            });
        }

        let text = response.text().await
            .map_err(|e| Error::ApiError { status: 500, message: format!("failed to read response: {}", e) })?;
        let trimmed = text.trim();

        serde_json::from_str(trimmed).map_err(|e| Error::ApiError {
            status: 500,
            message: format!("failed to parse response: {} - body: {}", e, trimmed),
        })
    }

    pub async fn get<T: DeserializeOwned>(&self, endpoint: &str) -> Result<T> {
        let url = self.build_url(endpoint);
        tracing::debug!("GET request to: {}", url);

        let response = self.client
            .get(&url)
            .header("apikey", &self.api_key)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| Error::HttpError(e.into()))?;

        self.parse_response(response).await
    }

    pub async fn post<T: DeserializeOwned, B: Serialize>(
        &self,
        endpoint: &str,
        body: &B,
    ) -> Result<T> {
        let url = self.build_url(endpoint);
        tracing::debug!("POST request to: {}", url);

        let response = self.client
            .post(&url)
            .header("apikey", &self.api_key)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| Error::HttpError(e.into()))?;

        self.parse_response(response).await
    }

    pub async fn delete<T: DeserializeOwned>(&self, endpoint: &str) -> Result<T> {
        let url = self.build_url(endpoint);
        tracing::debug!("DELETE request to: {}", url);

        let response = self.client
            .delete(&url)
            .header("apikey", &self.api_key)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| Error::HttpError(e.into()))?;

        self.parse_response(response).await
    }

    pub async fn get_markets(&self) -> Result<Vec<ApiMarket>> {
        self.get_markets_with_params(None, None).await
    }

    pub async fn get_markets_with_params(&self, status: Option<&str>, limit: Option<u32>) -> Result<Vec<ApiMarket>> {
        self.get_markets_with_pagination(status, limit, None).await
    }

    pub async fn get_markets_with_pagination(&self, status: Option<&str>, limit: Option<u32>, page: Option<u32>) -> Result<Vec<ApiMarket>> {
        let mut endpoint = "market".to_string();
        let mut params = vec![];
        if let Some(s) = status {
            params.push(format!("status={}", s));
        }
        if let Some(l) = limit {
            params.push(format!("limit={}", l));
        }
        if let Some(p) = page {
            params.push(format!("page={}", p));
        }
        if !params.is_empty() {
            endpoint = format!("{}?{}", endpoint, params.join("&"));
        }
        let response: OpinionResponse<MarketListResult> = self.get(&endpoint).await?;
        response.into_result().map(|r| r.list)
            .map_err(|e| Error::ApiError { status: 500, message: e })
    }

    pub async fn get_market_detail(&self, market_id: i64) -> Result<ApiMarket> {
        // NOTE: Market detail endpoint uses path parameter, not query parameter
        // URL: /openapi/market/{marketId}
        let endpoint = format!("market/{}", market_id);
        let response: OpinionResponse<MarketDetailResult> = self.get(&endpoint).await?;
        response.into_result()
            .map(|r| r.data)
            .map_err(|e| Error::ApiError { status: 500, message: e })
    }

    pub async fn get_orderbook(&self, token_id: &str) -> Result<TokenOrderbook> {
        let endpoint = format!("token/orderbook?token_id={}", token_id);
        let response: OpinionResponse<TokenOrderbook> = self.get(&endpoint).await?;
        response.into_result()
            .map_err(|e| Error::ApiError { status: 500, message: e })
    }

    pub async fn get_latest_price(&self, token_id: &str) -> Result<LatestPrice> {
        let endpoint = format!("token/latest-price?token_id={}", token_id);
        let response: OpinionResponse<LatestPrice> = self.get(&endpoint).await?;
        response.into_result()
            .map_err(|e| Error::ApiError { status: 500, message: e })
    }

    pub async fn get_orders(&self, market_id: Option<i64>, limit: Option<u32>) -> Result<Vec<UserOrder>> {
        let mut endpoint = "order".to_string();
        let mut params = vec![];
        if let Some(id) = market_id {
            params.push(format!("marketId={}", id));
        }
        if let Some(l) = limit {
            params.push(format!("limit={}", l));
        }
        if !params.is_empty() {
            endpoint = format!("{}?{}", endpoint, params.join("&"));
        }

        let response: OpinionResponse<OrderListResult> = self.get(&endpoint).await?;
        response.into_result().map(|r| r.list)
            .map_err(|e| Error::ApiError { status: 500, message: e })
    }

    pub async fn cancel_order(&self, order_id: &str) -> Result<serde_json::Value> {
        let body = serde_json::json!({"orderId": order_id});
        let response: OpinionResponse<serde_json::Value> = self.post("order/cancel", &body).await?;
        response.into_result()
            .map_err(|e| Error::ApiError { status: 500, message: e })
    }

    pub async fn get_quote_tokens(&self, chain_id: u64) -> Result<Vec<QuoteTokenInfo>> {
        let endpoint = format!("quoteToken?chainId={}", chain_id);
        let response: OpinionResponse<QuoteTokenList> = self.get(&endpoint).await?;
        response.into_result().map(|r| r.list)
            .map_err(|e| Error::ApiError { status: 500, message: e })
    }

    pub async fn get_exchange_address(&self, chain_id: u64, quote_token: &str) -> Result<String> {
        if let Some(cached) = self.cached_exchange_address.lock().await.as_ref() {
            return Ok(cached.clone());
        }

        let tokens = self.get_quote_tokens(chain_id).await?;

        tokens
            .iter()
            .find(|t| t.quote_token_address.eq_ignore_ascii_case(quote_token))
            .map(|t| t.ctf_exchange_address.clone())
            .ok_or_else(|| Error::ApiError {
                status: 404,
                message: format!("Exchange address not found for quote token: {}", quote_token),
            })
    }

    pub async fn get_user_auth(&self) -> Result<UserAuthInfo> {
        let response: OpinionResponse<UserAuthInfo> = self.get("user/auth").await?;
        response.into_result()
            .map_err(|e| Error::ApiError { status: 500, message: e })
    }

    pub async fn get_proxy_address(&self, chain_id: u64) -> Result<Option<String>> {
        if let Some(cached) = self.cached_proxy_address.lock().await.as_ref() {
            return Ok(Some(cached.clone()));
        }

        let auth = self.get_user_auth().await?;
        Ok(auth.wallet_users.get(&chain_id.to_string()).cloned())
    }

    // --- User Data API ---

    pub async fn get_my_positions(
        &self,
        chain_id: u64,
        market_id: Option<i64>,
        page: Option<u32>,
        limit: Option<u32>,
    ) -> Result<Vec<crate::data::rest::models::UserPosition>> {
        let mut endpoint = "positions".to_string();
        let mut params = vec![format!("chain_id={}", chain_id)];
        
        if let Some(id) = market_id {
            params.push(format!("marketId={}", id));
        }
        if let Some(p) = page {
            params.push(format!("page={}", p));
        }
        if let Some(l) = limit {
            params.push(format!("limit={}", l));
        }
        
        endpoint = format!("{}?{}", endpoint, params.join("&"));
        
        let response: OpinionResponse<crate::data::rest::models::PositionListResult> = self.get(&endpoint).await?;
        response.into_result()
            .map(|r| r.list)
            .map_err(|e| Error::ApiError { status: 500, message: e })
    }

    pub async fn get_my_balances(&self, chain_id: u64) -> Result<crate::data::rest::models::UserBalance> {
        // Try snake_case 'chain_id' instead of camelCase 'chainId'
        let endpoint = format!("user/balance?chain_id={}", chain_id);
        let response: OpinionResponse<crate::data::rest::models::UserBalance> = self.get(&endpoint).await?;
        response.into_result().map_err(|e| Error::ApiError { status: 500, message: e })
    }

    pub async fn get_my_trades(
        &self,
        chain_id: u64,
        market_id: Option<i64>,
        page: Option<u32>,
        limit: Option<u32>,
    ) -> Result<Vec<crate::data::rest::models::UserTrade>> {
        let mut endpoint = "trade".to_string();
        let mut params = vec![format!("chain_id={}", chain_id)];
        
        if let Some(id) = market_id {
            params.push(format!("marketId={}", id));
        }
        if let Some(p) = page {
            params.push(format!("page={}", p));
        }
        if let Some(l) = limit {
            params.push(format!("limit={}", l));
        }
        
        endpoint = format!("{}?{}", endpoint, params.join("&"));
        
        let response: OpinionResponse<crate::data::rest::models::TradeListResult> = self.get(&endpoint).await?;
        response.into_result()
            .map(|r| r.list.unwrap_or_default())
            .map_err(|e| Error::ApiError { status: 500, message: e })
    }

    // --- Batch Operations (Client-side implementation) ---

    pub async fn place_orders_batch(
        &self, 
        orders: Vec<serde_json::Value>
    ) -> Result<Vec<crate::data::rest::models::BatchOrderResult>> {
        use futures::future::join_all;
        
        let mut tasks = Vec::new();
        for (index, order_payload) in orders.into_iter().enumerate() {
            let client = self.clone();
            tasks.push(tokio::spawn(async move {
                let result = client.post::<OpinionResponse<serde_json::Value>, _>("order", &order_payload).await;
                
                match result {
                    Ok(resp) => {
                        match resp.into_result() {
                            Ok(data) => {
                                let order_id = data.get("orderData")
                                    .and_then(|d| d.get("orderId"))
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string());
                                crate::data::rest::models::BatchOrderResult {
                                    index,
                                    success: true,
                                    order_id,
                                    error: None
                                }
                            },
                            Err(e) => crate::data::rest::models::BatchOrderResult {
                                index,
                                success: false,
                                order_id: None,
                                error: Some(e)
                            }
                        }
                    },
                    Err(e) => crate::data::rest::models::BatchOrderResult {
                        index,
                        success: false,
                        order_id: None,
                        error: Some(e.to_string())
                    }
                }
            }));
        }

        let results = join_all(tasks).await;
        Ok(results.into_iter().filter_map(|r| r.ok()).collect())
    }

    pub async fn cancel_orders_batch(
        &self,
        order_ids: Vec<String>
    ) -> Result<Vec<crate::data::rest::models::BatchOrderResult>> {
        use futures::future::join_all;
        
        let mut tasks = Vec::new();
        for (index, order_id) in order_ids.into_iter().enumerate() {
            let client = self.clone();
            tasks.push(tokio::spawn(async move {
                let result = client.cancel_order(&order_id).await;
                
                match result {
                    Ok(_) => crate::data::rest::models::BatchOrderResult {
                        index,
                        success: true,
                        order_id: Some(order_id),
                        error: None
                    },
                    Err(e) => crate::data::rest::models::BatchOrderResult {
                        index,
                        success: false,
                        order_id: Some(order_id),
                        error: Some(e.to_string())
                    }
                }
            }));
        }

        let results = join_all(tasks).await;
        Ok(results.into_iter().filter_map(|r| r.ok()).collect())
    }

    pub async fn cancel_all_orders(
        &self,
        market_id: Option<i64>,
        _side: Option<i32> // Filter by side not strictly supported by list API but can be done client side
    ) -> Result<crate::data::rest::models::CancelAllResult> {
        // 1. Fetch open orders
        let orders = self.get_orders(market_id, Some(50)).await?;
        // Filter active orders (status 1 = pending/open)
        let active_orders: Vec<String> = orders.into_iter()
            .filter(|o| o.status == 1) 
            .map(|o| o.order_id)
            .collect();
            
        let total = active_orders.len();
        if total == 0 {
            return Ok(crate::data::rest::models::CancelAllResult { total_orders: 0, cancelled: 0, failed: 0 });
        }

        // 2. Batch cancel
        let results = self.cancel_orders_batch(active_orders).await?;
        
        let cancelled = results.iter().filter(|r| r.success).count();
        let failed = results.len() - cancelled;

        Ok(crate::data::rest::models::CancelAllResult {
            total_orders: total,
            cancelled,
            failed
        })
    }

    // --- Missing REST API ---

    pub async fn get_price_history(
        &self,
        token_id: &str,
        interval: &str,
        _start: Option<i64>,
        _end: Option<i64>
    ) -> Result<Vec<crate::data::rest::models::PriceHistoryEntry>> {
        let endpoint = format!("token/price-history?token_id={}&interval={}", token_id, interval);
        let response: OpinionResponse<PriceHistoryResult> = self.get(&endpoint).await?;
        response.into_result()
            .map(|r| r.history)
            .map_err(|e| Error::ApiError { status: 500, message: e })
    }

    pub async fn get_order_by_id(&self, order_id: &str) -> Result<UserOrder> {
        let endpoint = format!("order/{}", order_id);
        let response: OpinionResponse<OrderDataWrapper> = self.get(&endpoint).await?;
        response.into_result()
            .map(|r| r.order_data)
            .map_err(|e| Error::ApiError { status: 500, message: e })
    }
}
