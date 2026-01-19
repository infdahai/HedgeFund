//! Opinion Trade HFT Framework
//!
//! A high-frequency trading framework for the Opinion Trade prediction market
//! on BNB Chain. Designed for minimum tick-to-trade latency with reliable
//! order confirmation via on-chain verification.
//!
//! # Architecture
//!
//! The framework is organized into three planes:
//!
//! - **Data Plane**: WebSocket client, REST poller, Chain subscriber
//! - **Control Plane**: Strategy engine, Position manager, Risk manager
//! - **Execution Plane**: Rate limiter, Priority queue, Order gateway, Confirmation tracker
//!
//! # Critical Parameters
//!
//! - EIP-712 Domain: "OPINION CTF Exchange" (note the space!)
//! - Chain ID: 56 (BNB Chain)
//! - ConditionalTokens: 0xAD1a38cEc043e70E83a3eC30443dB285ED10D774
//! - API Rate Limit: 15 TPS
//! - Price Precision: 1 decimal place (0.1 - 0.9)

pub mod auth;
pub mod chain;
pub mod config;
pub mod data;
pub mod error;
pub mod execution;
pub mod strategy;
pub mod telemetry;
pub mod types;

pub use config::Config;
pub use config::ConfigBuilder;
pub use config::ConfirmationMode;
pub use error::{Error, Result};

/// Re-export commonly used types
pub mod prelude {
    pub use crate::auth::Eip712Signer;
    pub use crate::config::Config;
    pub use crate::error::{Error, Result};
    pub use crate::types::{Market, Order, OrderIntent, OrderPriority, OrderSide, Position};
}

#[cfg(any(test, feature = "test-harness"))]
pub mod test_harness {
    use crate::auth::{Eip712Signer, OrderSigner};
    use crate::chain::client::ChainClient;
    use crate::config::Config;
    use crate::data::rest::models::{OrderbookEntry, TokenOrderbook};
    use crate::data::rest::OpinionClient;
    use crate::error::{Error, Result};
    use crate::types::market::{Orderbook, OrderbookLevel};
    use crate::types::order::{Order, OrderIntent, OrderSide, SignatureType};
    use alloy_primitives::{Address, U256};
    use serde::{Deserialize, Serialize};
    use std::collections::HashMap;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};

    pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

    pub trait TestClient: Send + Sync {
        fn get_orderbook<'a>(&'a self, token_id: &'a str) -> BoxFuture<'a, Result<TokenOrderbook>>;
        fn place_order<'a>(&'a self, intent: OrderIntent) -> BoxFuture<'a, Result<String>>;
        fn cancel_order<'a>(&'a self, order_id: &'a str) -> BoxFuture<'a, Result<()>>;
        fn get_balance<'a>(&'a self, token_id: U256) -> BoxFuture<'a, Result<U256>>;
    }

    pub fn assert_order_intent_valid(intent: &OrderIntent) -> Result<()> {
        let (maker_amount, taker_amount) = intent.calculate_amounts();
        let value_wei = match intent.side {
            OrderSide::Buy => maker_amount,
            OrderSide::Sell => taker_amount,
        };
        let value_u128 = value_wei
            .to_string()
            .parse::<u128>()
            .map_err(|_| Error::InternalError("Amount overflow".into()))?;
        let value_usdt = value_u128 as f64 / 1e18_f64;
        if value_usdt < 1.30 {
            return Err(Error::InternalError(format!(
                "Order amount {:.4} USDT below minimum 1.30",
                value_usdt
            )));
        }
        Ok(())
    }

    pub struct LiveTestClient {
        client: OpinionClient,
        signer: Eip712Signer,
        maker: Address,
    }

    impl LiveTestClient {
        pub async fn new(config: &Config) -> Result<Self> {
            let client = OpinionClient::new(&config.api, config.environment)?;
            let signer = Eip712Signer::from_secret(&config.chain.private_key)?;
            let quote_token_address = "0x55d398326f99059fF775485246999027B3197955";
            client.init(56, quote_token_address).await?;
            let proxy_address = client.get_cached_proxy_address().await;
            let maker = if let Some(addr) = proxy_address {
                addr.parse::<Address>()
                    .map_err(|e| Error::InternalError(format!("Invalid proxy address: {e}")))?
            } else {
                signer.address()
            };

            Ok(Self {
                client,
                signer,
                maker,
            })
        }

        fn build_order(&self, intent: &OrderIntent) -> Result<(Order, String)> {
            let (maker_amount, taker_amount) = intent.calculate_amounts();
            let signature_type = if self.maker != self.signer.address() {
                SignatureType::PolyGnosisSafe
            } else {
                SignatureType::Eoa
            };
            let order = Order::new(
                self.maker,
                self.signer.address(),
                intent.token_id,
                maker_amount,
                taker_amount,
                intent.side,
            )
            .with_expiration(2000000000)
            .with_signature_type(signature_type);

            let signature = self.signer.sign_order(&order)?;
            let sig_hex = signature.to_hex();
            Ok((order, sig_hex))
        }
    }

    impl TestClient for LiveTestClient {
        fn get_orderbook<'a>(&'a self, token_id: &'a str) -> BoxFuture<'a, Result<TokenOrderbook>> {
            Box::pin(async move { self.client.get_orderbook(token_id).await })
        }

        fn place_order<'a>(&'a self, intent: OrderIntent) -> BoxFuture<'a, Result<String>> {
            Box::pin(async move {
                let (order, sig_hex) = self.build_order(&intent)?;
                let payload = serde_json::json!({
                    "salt": order.salt.to_string(),
                    "maker": format!("{:?}", order.maker),
                    "signer": format!("{:?}", order.signer),
                    "taker": format!("{:?}", order.taker),
                    "tokenId": order.token_id.to_string(),
                    "makerAmount": order.maker_amount.to_string(),
                    "takerAmount": order.taker_amount.to_string(),
                    "expiration": order.expiration.to_string(),
                    "nonce": order.nonce.to_string(),
                    "feeRateBps": order.fee_rate_bps.to_string(),
                    "side": order.side.as_u8().to_string(),
                    "signatureType": order.signature_type.as_u8().to_string(),
                    "signature": sig_hex.clone(),
                    "sign": sig_hex,
                    "timestamp": chrono::Utc::now().timestamp(),
                    "topicId": intent.market_id.clone(),
                    "tradingMethod": 2,
                    "contractAddress": "",
                    "currencyAddress": "0x55d398326f99059fF775485246999027B3197955",
                    "orderExpTime": "0",
                    "safeRate": "0",
                    "price": intent.price.to_string(),
                });
                let response: crate::data::rest::models::OpinionResponse<serde_json::Value> =
                    self.client.post("order", &payload).await?;
                let result = response.into_result().map_err(|e| Error::ApiError {
                    status: 500,
                    message: e,
                })?;
                let order_id = result["orderData"]["orderId"]
                    .as_str()
                    .ok_or_else(|| Error::InternalError("Order ID missing".into()))?
                    .to_string();
                Ok(order_id)
            })
        }

        fn cancel_order<'a>(&'a self, order_id: &'a str) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move { self.client.cancel_order(order_id).await.map(|_| ()) })
        }

        fn get_balance<'a>(&'a self, token_id: U256) -> BoxFuture<'a, Result<U256>> {
            Box::pin(async move {
                let config = Config::from_env()?;
                let chain_client = ChainClient::new(&config.chain, self.maker)?;
                chain_client.balance_of(token_id).await
            })
        }
    }

    #[derive(Clone)]
    pub struct SimConfig {
        slippage_bps: u64,
        partial_fill_enabled: bool,
        min_fill_ratio: f64,
        max_fill_ratio: f64,
        topic_fee_rate: f64,
        referral_discount: f64,
        min_fee_usdt: f64,
        market_depth_usdt: f64,
        price_impact_factor: f64,
    }

    impl Default for SimConfig {
        fn default() -> Self {
            Self {
                slippage_bps: 30,
                partial_fill_enabled: true,
                min_fill_ratio: 0.5,
                max_fill_ratio: 1.0,
                topic_fee_rate: 0.02,
                referral_discount: 0.10,
                min_fee_usdt: 0.50,
                market_depth_usdt: 100.0,
                price_impact_factor: 0.05,
            }
        }
    }

    #[derive(Clone)]
    pub struct SimTestClient {
        market_id: String,
        token_id: U256,
        yes_price: f64,
        quantity: U256,
        balances: Arc<Mutex<U256>>,
        usdt_balance: Arc<Mutex<U256>>,
        orders: Arc<Mutex<Vec<String>>>,
        config: SimConfig,
    }

    struct FillResult {
        filled_quantity: U256,
        fill_price: f64,
        fee_wei: U256,
    }

    impl SimTestClient {
        pub fn new(market_id: impl Into<String>, token_id: U256) -> Self {
            let unit = U256::from(10u64).pow(U256::from(18u64));
            Self {
                market_id: market_id.into(),
                token_id,
                yes_price: 0.5,
                quantity: U256::from(2000u64) * unit,
                balances: Arc::new(Mutex::new(U256::ZERO)),
                usdt_balance: Arc::new(Mutex::new(U256::from(1000u64) * unit)),
                orders: Arc::new(Mutex::new(Vec::new())),
                config: SimConfig::default(),
            }
        }

        fn simulate_fill(&self, intent: &OrderIntent) -> Result<FillResult> {
            let quantity_u128 = intent
                .quantity
                .to_string()
                .parse::<u128>()
                .map_err(|_| Error::InternalError("Quantity overflow".into()))?;
            let order_value_usdt = intent.price * quantity_u128 as f64 / 1e18_f64;
            let mut fill_ratio = if self.config.partial_fill_enabled {
                if order_value_usdt > self.config.market_depth_usdt {
                    self.config.market_depth_usdt / order_value_usdt
                } else {
                    self.config.max_fill_ratio
                }
            } else {
                1.0
            };
            if fill_ratio < self.config.min_fill_ratio {
                fill_ratio = self.config.min_fill_ratio;
            }
            if fill_ratio > self.config.max_fill_ratio {
                fill_ratio = self.config.max_fill_ratio;
            }
            let filled_quantity_u128 = (quantity_u128 as f64 * fill_ratio) as u128;
            let filled_quantity = U256::from(filled_quantity_u128);

            let base_slippage = self.config.slippage_bps as f64 / 10000.0;
            let depth_impact = (1.0 - fill_ratio) * self.config.price_impact_factor;
            let total_slippage = base_slippage + depth_impact;
            let market_price = self.yes_price;
            let fill_price = match intent.side {
                OrderSide::Buy => (market_price * (1.0 + total_slippage)).min(intent.price),
                OrderSide::Sell => (market_price * (1.0 - total_slippage)).max(intent.price),
            };

            let fee_wei = self.calculate_fee(fill_price, filled_quantity)?;

            Ok(FillResult {
                filled_quantity,
                fill_price,
                fee_wei,
            })
        }

        fn calculate_fee(&self, price: f64, quantity: U256) -> Result<U256> {
            let quantity_u128 = quantity
                .to_string()
                .parse::<u128>()
                .map_err(|_| Error::InternalError("Quantity overflow".into()))?;
            let trade_amount = price * quantity_u128 as f64 / 1e18_f64;
            let base_fee_rate = self.config.topic_fee_rate * price * (1.0 - price);
            let effective_rate = base_fee_rate * (1.0 - self.config.referral_discount);
            let calculated_fee = trade_amount * effective_rate;
            let final_fee = calculated_fee.max(self.config.min_fee_usdt);
            let fee_wei_u128 = (final_fee * 1e18_f64) as u128;
            Ok(U256::from(fee_wei_u128))
        }

        fn apply_fill(&self, intent: &OrderIntent, fill: &FillResult) -> Result<()> {
            let filled_qty_u128 = fill
                .filled_quantity
                .to_string()
                .parse::<u128>()
                .map_err(|_| Error::InternalError("Quantity overflow".into()))?;
            let cost_wei_u128 = (fill.fill_price * filled_qty_u128 as f64) as u128;
            let cost_wei = U256::from(cost_wei_u128);
            let mut token_balance = self.balances.lock().unwrap();
            let mut usdt_balance = self.usdt_balance.lock().unwrap();
            match intent.side {
                OrderSide::Buy => {
                    *token_balance = token_balance.saturating_add(fill.filled_quantity);
                    *usdt_balance = usdt_balance
                        .saturating_sub(cost_wei)
                        .saturating_sub(fill.fee_wei);
                }
                OrderSide::Sell => {
                    *token_balance = token_balance.saturating_sub(fill.filled_quantity);
                    *usdt_balance = usdt_balance
                        .saturating_add(cost_wei)
                        .saturating_sub(fill.fee_wei);
                }
            }
            Ok(())
        }
    }

    impl TestClient for SimTestClient {
        fn get_orderbook<'a>(&'a self, pair: &'a str) -> BoxFuture<'a, Result<TokenOrderbook>> {
            Box::pin(async move {
                let bid = OrderbookLevel {
                    price: self.yes_price - 0.01,
                    quantity: U256::from(1000u64),
                };
                let ask = OrderbookLevel {
                    price: self.yes_price + 0.01,
                    quantity: U256::from(1000u64),
                };
                let orderbook = Orderbook {
                    pair: pair.to_string(),
                    bids: vec![bid],
                    asks: vec![ask],
                    timestamp: 0,
                };
                Ok(TokenOrderbook {
                    market: Some(self.market_id.clone()),
                    token_id: self.token_id.to_string(),
                    timestamp: 0,
                    bids: orderbook
                        .bids
                        .into_iter()
                        .map(|level| OrderbookEntry {
                            price: level.price.to_string(),
                            size: level.quantity.to_string(),
                        })
                        .collect(),
                    asks: orderbook
                        .asks
                        .into_iter()
                        .map(|level| OrderbookEntry {
                            price: level.price.to_string(),
                            size: level.quantity.to_string(),
                        })
                        .collect(),
                })
            })
        }

        fn place_order<'a>(&'a self, intent: OrderIntent) -> BoxFuture<'a, Result<String>> {
            Box::pin(async move {
                if intent.market_id != self.market_id {
                    return Err(Error::InternalError("Unknown market".into()));
                }
                if intent.token_id != self.token_id {
                    return Err(Error::InternalError("Unknown token".into()));
                }
                if intent.quantity > self.quantity {
                    return Err(Error::InternalError(
                        "Order size exceeds simulated liquidity".into(),
                    ));
                }
                let fill = self.simulate_fill(&intent)?;
                self.apply_fill(&intent, &fill)?;
                let order_id = format!("sim-{}", self.orders.lock().unwrap().len());
                self.orders.lock().unwrap().push(order_id.clone());
                Ok(order_id)
            })
        }

        fn cancel_order<'a>(&'a self, _order_id: &'a str) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move { Ok(()) })
        }

        fn get_balance<'a>(&'a self, token_id: U256) -> BoxFuture<'a, Result<U256>> {
            Box::pin(async move {
                if token_id != self.token_id {
                    return Err(Error::InternalError("Unknown token".into()));
                }
                Ok(*self.balances.lock().unwrap())
            })
        }
    }

    pub struct TestScenario {
        pub client: Arc<dyn TestClient>,
        pub market_id: String,
        pub token_id: U256,
        pub price: f64,
    }

    impl TestScenario {
        pub async fn live(config: &Config, market_id: String, token_id: U256) -> Result<Self> {
            let client = LiveTestClient::new(config).await?;
            Ok(Self {
                client: Arc::new(client),
                market_id,
                token_id,
                price: 0.5,
            })
        }

        pub fn simulated() -> Self {
            let market_id = "sim-market".to_string();
            let token_id = U256::from(1001u64);
            let client = SimTestClient::new(market_id.clone(), token_id);
            Self {
                client: Arc::new(client),
                market_id,
                token_id,
                price: 0.5,
            }
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(tag = "type")]
    pub enum ReplayEvent {
        #[serde(rename = "init")]
        Init {
            timestamp: u64,
            market_id: String,
            token_id: String,
            initial_price: f64,
            initial_balance_usdt: f64,
        },
        #[serde(rename = "trade")]
        Trade {
            timestamp: u64,
            trade_id: String,
            market_id: String,
            side: String,
            price: f64,
            quantity: u64,
            fee: f64,
        },
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ReplayTrade {
        pub trade_id: String,
        pub price: f64,
        pub quantity: u64,
        pub fee: f64,
        pub side: OrderSide,
    }

    pub struct ReplayTestClient {
        market_id: String,
        token_id: U256,
        current_price: f64,
        initial_balance: f64,
        trades: Vec<ReplayTrade>,
        orders: Arc<Mutex<HashMap<String, OrderIntent>>>,
        next_order_id: Arc<Mutex<u64>>,
    }

    impl ReplayTestClient {
        pub fn new_from_jsonl(market_id: String, token_id: U256, jsonl_data: &str) -> Result<Self> {
            let mut current_price = 0.5;
            let mut initial_balance = 1000.0;
            let mut trades = Vec::new();
            for line in jsonl_data.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                let event: ReplayEvent = serde_json::from_str(line)
                    .map_err(|e| Error::InternalError(format!("Replay parse error: {e}")))?;
                match event {
                    ReplayEvent::Init {
                        initial_price,
                        initial_balance_usdt,
                        ..
                    } => {
                        current_price = initial_price;
                        initial_balance = initial_balance_usdt;
                    }
                    ReplayEvent::Trade {
                        trade_id,
                        side,
                        price,
                        quantity,
                        fee,
                        ..
                    } => {
                        current_price = price;
                        let side_enum = if side == "buy" {
                            OrderSide::Buy
                        } else {
                            OrderSide::Sell
                        };
                        trades.push(ReplayTrade {
                            trade_id,
                            price,
                            quantity,
                            fee,
                            side: side_enum,
                        });
                    }
                }
            }

            Ok(Self {
                market_id,
                token_id,
                current_price,
                initial_balance,
                trades,
                orders: Arc::new(Mutex::new(HashMap::new())),
                next_order_id: Arc::new(Mutex::new(0)),
            })
        }

        pub fn get_metrics(&self) -> (f64, f64, f64, usize) {
            let mut total_pnl = 0.0;
            let mut wins = 0;
            let mut count = 0;
            let mut current_equity = self.initial_balance;
            let mut peak_equity = self.initial_balance;
            let mut max_drawdown = 0.0;
            for trade in &self.trades {
                let gross = trade.price * trade.quantity as f64 / 1e18_f64;
                let pnl = match trade.side {
                    OrderSide::Buy => -gross - trade.fee,
                    OrderSide::Sell => gross - trade.fee,
                };
                total_pnl += pnl;
                current_equity += pnl;
                if current_equity > peak_equity {
                    peak_equity = current_equity;
                }
                let drawdown = if peak_equity > 0.0 {
                    (peak_equity - current_equity) / peak_equity
                } else {
                    0.0
                };
                if drawdown > max_drawdown {
                    max_drawdown = drawdown;
                }
                if pnl > 0.0 {
                    wins += 1;
                }
                count += 1;
            }
            let win_rate = if count > 0 {
                wins as f64 / count as f64
            } else {
                0.0
            };
            (total_pnl, max_drawdown, win_rate, count)
        }
    }

    impl TestClient for ReplayTestClient {
        fn get_orderbook<'a>(&'a self, token_id: &'a str) -> BoxFuture<'a, Result<TokenOrderbook>> {
            Box::pin(async move {
                let bid = OrderbookEntry {
                    price: (self.current_price - 0.01).to_string(),
                    size: "1000".to_string(),
                };
                let ask = OrderbookEntry {
                    price: (self.current_price + 0.01).to_string(),
                    size: "1000".to_string(),
                };
                Ok(TokenOrderbook {
                    market: Some(self.market_id.clone()),
                    token_id: token_id.to_string(),
                    timestamp: 0,
                    bids: vec![bid],
                    asks: vec![ask],
                })
            })
        }

        fn place_order<'a>(&'a self, intent: OrderIntent) -> BoxFuture<'a, Result<String>> {
            Box::pin(async move {
                let mut next_id = self.next_order_id.lock().unwrap();
                let order_id = format!("replay-{}", *next_id);
                *next_id += 1;
                self.orders.lock().unwrap().insert(order_id.clone(), intent);
                Ok(order_id)
            })
        }

        fn cancel_order<'a>(&'a self, order_id: &'a str) -> BoxFuture<'a, Result<()>> {
            Box::pin(async move {
                self.orders.lock().unwrap().remove(order_id);
                Ok(())
            })
        }

        fn get_balance<'a>(&'a self, token_id: U256) -> BoxFuture<'a, Result<U256>> {
            Box::pin(async move {
                if token_id != self.token_id {
                    return Err(Error::InternalError("Unknown token".into()));
                }
                Ok(U256::ZERO)
            })
        }
    }
}
