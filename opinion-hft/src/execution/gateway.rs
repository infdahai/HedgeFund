//! Order Gateway module.
//!
//! Acts as the central orchestrator for the execution plane, coordinating:
//! - Order submission (REST)
//! - Signing (EIP-712)
//! - Rate limiting
//! - Confirmation tracking (WebSocket + Chain)

use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};
use alloy_primitives::Address;
use serde::Serialize;

use crate::auth::OrderSigner;
use crate::auth::Eip712Signer;
use crate::config::ExecutionConfig;
use crate::data::rest::OpinionClient;
use crate::data::rest::models::OpinionResponse;
use crate::data::websocket::{WebSocketClient, WsEvent};
use crate::error::{Result, Error};
use crate::execution::confirmation::{ConfirmationTracker, PendingOrder};
use crate::execution::rate_limiter::RateLimiter;
use crate::execution::risk_manager::{RiskManager, TradeRecord};
use crate::types::order::{Order, OrderIntent, SignatureType};

#[derive(Serialize)]
struct SignedOrderPayload {
    #[serde(flatten)]
    order: Order,
    signature: String,
}

pub struct OrderGateway {
    rest_client: OpinionClient,
    signer: Eip712Signer,
    rate_limiter: RateLimiter,
    confirmation_tracker: Arc<ConfirmationTracker>,
    risk_manager: Arc<Mutex<RiskManager>>,
    ws_client: WebSocketClient,
    maker_address: Address,
    proxy_address: Option<Address>,
    _config: ExecutionConfig,
}

impl OrderGateway {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        rest_client: OpinionClient,
        signer: Eip712Signer,
        rate_limiter: RateLimiter,
        confirmation_tracker: Arc<ConfirmationTracker>,
        risk_manager: Arc<Mutex<RiskManager>>,
        ws_client: WebSocketClient,
        maker_address: Address,
        config: ExecutionConfig,
    ) -> Self {
        Self {
            rest_client,
            signer,
            rate_limiter,
            confirmation_tracker,
            risk_manager,
            ws_client,
            maker_address,
            proxy_address: None,
            _config: config,
        }
    }

    pub fn with_proxy_address(mut self, proxy: Address) -> Self {
        self.proxy_address = Some(proxy);
        self
    }

    pub fn set_proxy_address(&mut self, proxy: Address) {
        self.proxy_address = Some(proxy);
    }

    fn effective_maker(&self) -> Address {
        self.proxy_address.unwrap_or(self.maker_address)
    }

    fn signature_type(&self) -> SignatureType {
        if self.proxy_address.is_some() {
            SignatureType::PolyGnosisSafe
        } else {
            SignatureType::Eoa
        }
    }

    pub async fn run(&mut self, mut opportunity_rx: Option<tokio::sync::mpsc::Receiver<crate::strategy::arbitrage::ArbitrageOpportunity>>) -> Result<()> {
        let mut ws_rx = self.ws_client.subscribe_events();

        info!("OrderGateway started");

        loop {
            tokio::select! {
                Ok(event) = ws_rx.recv() => {
                    self.handle_ws_event(event).await?;
                }
                
                // Handle incoming arbitrage opportunities
                Some(opp) = async { 
                    if let Some(rx) = &mut opportunity_rx {
                        rx.recv().await
                    } else {
                        std::future::pending().await
                    }
                } => {
                    if let Err(e) = self.submit_arbitrage(opp).await {
                        error!("Arbitrage execution failed: {}", e);
                    }
                }

                _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {
                    if let Err(e) = self.risk_manager.lock().await.check_health().await {
                        error!("Risk check failed: {}", e);
                    }
                }
            }
        }
    }

    async fn handle_ws_event(&self, event: WsEvent) -> Result<()> {
        match event {
            WsEvent::OrderUpdate(update) => {
                if update.status == "Finish" {
                    info!("Order finished via WS: {}", update.order_id);
                    if let Some(confirmed) = self.confirmation_tracker
                        .on_order_finish(&update.order_id, None)
                        .await? 
                    {
                        // Record trade in Risk Manager
                        // We use the limit price as the execution price (optimistic/limit assumption)
                        // since we don't have the exact match price from the fill event yet.
                        let execution_price = confirmed.expected_price; 
                        
                        // Calculate approximate P&L (Slippage only)
                        // This is technically "Slippage P&L", not "Realized P&L"
                        // Realized P&L requires matching an entry with an exit.
                        let slippage_pnl = 0.0; 

                        let trade_record = TradeRecord {
                            trade_id: confirmed.order_id.clone(),
                            market_id: confirmed.market_id.clone(),
                            expected_profit: 0.0, // We don't track expected profit per-order yet
                            actual_pnl: slippage_pnl,
                            expected_price: confirmed.expected_price,
                            actual_price: execution_price,
                            execution_time_ms: confirmed.confirmation_time.as_millis() as u64,
                            timestamp: std::time::Instant::now(),
                        };

                        if let Err(e) = self.risk_manager.lock().await.record_trade(trade_record) {
                            error!("Risk manager rejected trade record: {}", e);
                        }

                        if let Some(hedge) = confirmed.hedge_intent {
                            info!("Executing hedge for order {}", confirmed.order_id);
                            match self.submit_order(hedge, None).await {
                                Ok(id) => info!("Hedge submitted: {}", id),
                                Err(e) => error!("Hedge failed: {}", e),
                            }
                        }
                    }
                }
            }
            WsEvent::Connected => {
                info!("WebSocket connected");
                self.ws_client.subscribe("trade.order.update").await?;
            }
            _ => {}
        }
        Ok(())
    }

    pub async fn submit_order(&self, intent: OrderIntent, hedge_intent: Option<OrderIntent>) -> Result<String> {
        // 1. Pre-trade Risk Check
        // Calculate estimated order size in USDT
        let (maker_amount, taker_amount) = intent.calculate_amounts();
        let order_size_usdt = match intent.side {
             // For Buy, we pay Maker Amount (USDC)
             crate::types::order::OrderSide::Buy => {
                 // Assuming USDC is 18 decimals, converting rough float for risk check
                 // accurate conversion: maker_amount / 1e18
                 let amount_str = maker_amount.to_string();
                 amount_str.parse::<f64>().unwrap_or(0.0) / 1e18
             },
             // For Sell, we receive Taker Amount (USDC)
             crate::types::order::OrderSide::Sell => {
                 let amount_str = taker_amount.to_string();
                 amount_str.parse::<f64>().unwrap_or(0.0) / 1e18
             }
        };

        // Validate with Risk Manager
        self.risk_manager.lock().await.validate_pre_trade(
            &intent.market_id, 
            0.0, // Expected profit unknown for single order
            order_size_usdt
        )?;

        self.rate_limiter.acquire().await;
        
        let order = Order::new(
            self.effective_maker(),
            self.signer.address(),
            intent.token_id,
            maker_amount,
            taker_amount,
            intent.side,
        )
        .with_signature_type(self.signature_type());

        let signature = self.signer.sign_order(&order)?;

        let position_before = alloy_primitives::U256::ZERO; 

        self.confirmation_tracker.register_order(PendingOrder {
            order_id: "pending".into(),
            market_id: intent.market_id.clone(),
            token_id: intent.token_id,
            side: intent.side,
            expected_price: intent.price,
            position_before,
            submitted_at: std::time::Instant::now(),
            hedge_intent: hedge_intent.map(Box::new).map(|b| *b),
        });

        let payload = SignedOrderPayload {
            order,
            signature: signature.to_hex(),
        };

        let response: OpinionResponse<serde_json::Value> = self
            .rest_client
            .post("order", &payload)
            .await?;
        
        let result = response.into_result().map_err(|e| Error::ApiError { status: 500, message: e })?;
        
        let order_id = result
            .get("orderData")
            .and_then(|d| d.get("orderId"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::ApiError { 
                status: 500, 
                message: "Order ID missing in response".to_string() 
            })?
            .to_string();
        
        Ok(order_id)
    }

    /// Submit a full arbitrage opportunity (scaffolding for atomic execution).
    /// 
    /// Currently, this just logs the intent and submits legs sequentially.
    /// In the future, this should support:
    /// - "Fill or Kill" logic
    /// - Rollback if leg 2 fails
    /// - Smart batching (if API supports it)
    pub async fn submit_arbitrage(&self, opportunity: crate::strategy::arbitrage::ArbitrageOpportunity) -> Result<Vec<String>> {
        let mut order_ids = Vec::new();
        
        info!("Executing arbitrage opportunity: {}", opportunity.summary());

        // Validate Pre-trade with total cost/exposure
        self.risk_manager.lock().await.validate_pre_trade(
            &opportunity.market_id,
            opportunity.expected_profit(opportunity.max_size),
            opportunity.total_cost * opportunity.max_size
        )?;

        // Execute Legs Sequentially
        for (i, leg) in opportunity.legs.iter().enumerate() {
            // Determine side based on Leg
            let side = match leg.side {
                crate::strategy::arbitrage::TradeSide::Buy => crate::types::order::OrderSide::Buy,
                crate::strategy::arbitrage::TradeSide::Sell => crate::types::order::OrderSide::Sell,
            };

            // Parse token ID
            use std::str::FromStr;
            let token_id = alloy_primitives::U256::from_str(&leg.token_id)
                .unwrap_or(alloy_primitives::U256::ZERO);

            let intent = OrderIntent::new(
                opportunity.market_id.clone(),
                token_id,
                side,
                leg.price,
                alloy_primitives::U256::from((leg.size * 1e18) as u128), // Scale size
            );
            
            match self.submit_order(intent, None).await {
                Ok(id) => {
                    info!("Leg {} submitted: {}", i + 1, id);
                    order_ids.push(id);
                },
                Err(e) => {
                    error!("Leg {} failed: {}", i + 1, e);
                    
                    // Rollback/Unwind previously filled legs
                    // Since atomic execution failed, we must exit any positions we entered.
                    info!("Triggering rollback for {} filled legs due to failure on leg {}", i, i + 1);
                    
                    for (j, prev_leg) in opportunity.legs.iter().take(i).enumerate() {
                        // Invert side for rollback
                        let rollback_side = match prev_leg.side {
                            crate::strategy::arbitrage::TradeSide::Buy => crate::types::order::OrderSide::Sell,
                            crate::strategy::arbitrage::TradeSide::Sell => crate::types::order::OrderSide::Buy,
                        };

                        let prev_token_id = alloy_primitives::U256::from_str(&prev_leg.token_id)
                            .unwrap_or(alloy_primitives::U256::ZERO);

                        // Aggressive unwinding:
                        // If Selling (to exit Buy), price lower (0.95x)
                        // If Buying (to exit Sell), price higher (1.05x)
                        let rollback_price = match rollback_side {
                            crate::types::order::OrderSide::Buy => prev_leg.price * 1.05, 
                            crate::types::order::OrderSide::Sell => prev_leg.price * 0.95, 
                        };
                        
                        // Round price to 3 decimals to meet API reqs
                        let rollback_price = (rollback_price * 1000.0).round() / 1000.0;

                        let rollback_intent = OrderIntent::new(
                            opportunity.market_id.clone(),
                            prev_token_id,
                            rollback_side,
                            rollback_price,
                            alloy_primitives::U256::from((prev_leg.size * 1e18) as u128),
                        );

                        match self.submit_order(rollback_intent, None).await {
                            Ok(rb_id) => info!("Rollback order submitted for leg {}: {}", j + 1, rb_id),
                            Err(rb_e) => error!("Failed to submit rollback for leg {}: {}", j + 1, rb_e),
                        }
                    }

                    return Err(e);
                }
            }
        }

        Ok(order_ids)
    }
}
