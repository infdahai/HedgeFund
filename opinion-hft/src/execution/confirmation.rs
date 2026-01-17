use alloy_primitives::U256;
use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use crate::chain::client::ChainClient;
use crate::config::{ConfirmationConfig, ConfirmationMode};
use crate::error::{Error, Result};
use crate::types::order::{OrderIntent, OrderSide};

#[derive(Debug, Clone)]
pub struct PendingOrder {
    pub order_id: String,
    pub market_id: String,
    pub token_id: U256,
    pub side: OrderSide,
    pub expected_price: f64,
    pub position_before: U256,
    pub submitted_at: Instant,
    pub hedge_intent: Option<OrderIntent>,
}

#[derive(Debug, Clone)]
pub struct ConfirmedOrder {
    pub order_id: String,
    pub market_id: String,
    pub token_id: U256,
    pub side: OrderSide,
    pub actual_fill: U256,
    pub expected_price: f64,
    pub position_before: U256,
    pub position_after: U256,
    pub confirmation_time: Duration,
    pub hedge_intent: Option<OrderIntent>,
}

pub struct ConfirmationTracker {
    pending: Arc<DashMap<String, PendingOrder>>,
    chain_client: ChainClient,
    config: ConfirmationConfig,
    confirmation_tx: mpsc::Sender<ConfirmedOrder>,
}

impl ConfirmationTracker {
    pub fn new(
        chain_client: ChainClient,
        config: ConfirmationConfig,
    ) -> (Self, mpsc::Receiver<ConfirmedOrder>) {
        let (confirmation_tx, confirmation_rx) = mpsc::channel(256);

        let tracker = Self {
            pending: Arc::new(DashMap::new()),
            chain_client,
            config,
            confirmation_tx,
        };

        (tracker, confirmation_rx)
    }

    pub fn register_order(&self, order: PendingOrder) {
        self.pending.insert(order.order_id.clone(), order);
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn is_pending(&self, order_id: &str) -> bool {
        self.pending.contains_key(order_id)
    }

    pub async fn on_order_finish(&self, order_id: &str, ws_fill: Option<U256>) -> Result<Option<ConfirmedOrder>> {
        let pending = match self.pending.remove(order_id) {
            Some((_, p)) => p,
            None => return Ok(None),
        };

        let (actual_fill, position_after) = match self.config.mode {
            ConfirmationMode::TrustWebSocket => {
                let fill = ws_fill.unwrap_or(U256::ZERO);
                let position_after = match pending.side {
                    OrderSide::Buy => pending.position_before.saturating_add(fill),
                    OrderSide::Sell => pending.position_before.saturating_sub(fill),
                };
                (fill, position_after)
            }
            ConfirmationMode::OnChainOnly | ConfirmationMode::Hybrid => {
                let mut position_after = U256::ZERO;
                let mut attempts = 0;
                let max_attempts = 3;

                // Retry loop for chain data consistency
                while attempts < max_attempts {
                    position_after = self
                        .chain_client
                        .balance_of(pending.token_id)
                        .await
                        .unwrap_or(U256::ZERO);

                    // If position changed, we're good
                    if position_after != pending.position_before {
                        break;
                    }

                    // If not changed, wait a bit (chain might be slightly behind WS)
                    // User report: "Chain is faster than API", but WS push might beat chain indexing slightly
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    attempts += 1;
                }

                let actual_fill = match pending.side {
                    OrderSide::Buy => position_after.saturating_sub(pending.position_before),
                    OrderSide::Sell => pending.position_before.saturating_sub(position_after),
                };

                (actual_fill, position_after)
            }
        };

        let confirmed = ConfirmedOrder {
            order_id: pending.order_id,
            market_id: pending.market_id,
            token_id: pending.token_id,
            side: pending.side,
            actual_fill,
            expected_price: pending.expected_price,
            position_before: pending.position_before,
            position_after,
            confirmation_time: pending.submitted_at.elapsed(),
            hedge_intent: pending.hedge_intent,
        };

        self.confirmation_tx
            .send(confirmed.clone())
            .await
            .map_err(|_| Error::ChannelError("confirmation channel closed".into()))?;

        Ok(Some(confirmed))
    }

    pub fn cancel_order(&self, order_id: &str) -> Option<PendingOrder> {
        self.pending.remove(order_id).map(|(_, p)| p)
    }

    pub fn timeout_expired_orders(&self, timeout: Duration) -> Vec<String> {
        let now = Instant::now();
        let expired: Vec<String> = self
            .pending
            .iter()
            .filter(|entry| now.duration_since(entry.submitted_at) > timeout)
            .map(|entry| entry.order_id.clone())
            .collect();

        for order_id in &expired {
            self.pending.remove(order_id);
        }

        expired
    }
}

impl Clone for ConfirmationTracker {
    fn clone(&self) -> Self {
        Self {
            pending: Arc::clone(&self.pending),
            chain_client: self.chain_client.clone(),
            config: self.config.clone(),
            confirmation_tx: self.confirmation_tx.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ChainConfig, CONDITIONAL_TOKENS_ADDRESS};
    use alloy_primitives::Address;

    fn mock_chain_client() -> ChainClient {
        let config = ChainConfig {
            rpc_url: "https://bsc-dataseed.binance.org".into(),
            chain_id: 56,
            private_key: crate::auth::secret::PrivateKey::default(),
            eoa_address: String::new(),
            conditional_tokens_address: CONDITIONAL_TOKENS_ADDRESS.into(),
            rpc_timeout: Duration::from_secs(5),
        };
        ChainClient::new(&config, Address::ZERO).unwrap()
    }


    fn mock_config(mode: ConfirmationMode) -> ConfirmationConfig {
        ConfirmationConfig {
            timeout: Duration::from_secs(30),
            poll_interval: Duration::from_millis(500),
            mode,
        }
    }

    #[test]
    fn test_confirmation_tracker_creation() {
        let chain_client = mock_chain_client();
        let config = mock_config(ConfirmationMode::Hybrid);
        let (tracker, _rx) = ConfirmationTracker::new(chain_client, config);
        assert_eq!(tracker.pending_count(), 0);
    }

    #[test]
    fn test_register_order() {
        let chain_client = mock_chain_client();
        let config = mock_config(ConfirmationMode::Hybrid);
        let (tracker, _rx) = ConfirmationTracker::new(chain_client, config);

        let pending = PendingOrder {
            order_id: "order-1".into(),
            market_id: "market-1".into(),
            token_id: U256::from(123),
            side: OrderSide::Buy,
            expected_price: 0.5,
            position_before: U256::from(100),
            submitted_at: Instant::now(),
            hedge_intent: None,
        };


        tracker.register_order(pending);
        assert_eq!(tracker.pending_count(), 1);
        assert!(tracker.is_pending("order-1"));
    }

    #[test]
    fn test_cancel_order() {
        let chain_client = mock_chain_client();
        let config = mock_config(ConfirmationMode::Hybrid);
        let (tracker, _rx) = ConfirmationTracker::new(chain_client, config);

        let pending = PendingOrder {
            order_id: "order-1".into(),
            market_id: "market-1".into(),
            token_id: U256::from(123),
            side: OrderSide::Buy,
            expected_price: 0.5,
            position_before: U256::from(100),
            submitted_at: Instant::now(),
            hedge_intent: None,
        };


        tracker.register_order(pending);
        assert_eq!(tracker.pending_count(), 1);

        let cancelled = tracker.cancel_order("order-1");
        assert!(cancelled.is_some());
        assert_eq!(tracker.pending_count(), 0);
    }

    #[test]
    fn test_timeout_expired_orders() {
        let chain_client = mock_chain_client();
        let config = mock_config(ConfirmationMode::Hybrid);
        let (tracker, _rx) = ConfirmationTracker::new(chain_client, config);

        let old_pending = PendingOrder {
            order_id: "old-order".into(),
            market_id: "market-1".into(),
            token_id: U256::from(123),
            side: OrderSide::Buy,
            expected_price: 0.5,
            position_before: U256::from(100),
            submitted_at: Instant::now() - Duration::from_secs(60),
            hedge_intent: None,
        };

        let new_pending = PendingOrder {
            order_id: "new-order".into(),
            market_id: "market-1".into(),
            token_id: U256::from(456),
            side: OrderSide::Sell,
            expected_price: 0.5,
            position_before: U256::from(200),
            submitted_at: Instant::now(),
            hedge_intent: None,
        };

        tracker.register_order(old_pending);
        tracker.register_order(new_pending);
        assert_eq!(tracker.pending_count(), 2);

        let expired = tracker.timeout_expired_orders(Duration::from_secs(30));
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0], "old-order");
        assert_eq!(tracker.pending_count(), 1);
        assert!(tracker.is_pending("new-order"));
    }

    #[tokio::test]
    async fn test_on_order_finish_trust_websocket() {
        let chain_client = mock_chain_client();
        let config = mock_config(ConfirmationMode::TrustWebSocket);
        let (tracker, mut rx) = ConfirmationTracker::new(chain_client, config);

        let pending = PendingOrder {
            order_id: "order-1".into(),
            market_id: "market-1".into(),
            token_id: U256::from(123),
            side: OrderSide::Buy,
            expected_price: 0.5,
            position_before: U256::from(100),
            submitted_at: Instant::now(),
            hedge_intent: None,
        };


        tracker.register_order(pending);

        let result = tracker
            .on_order_finish("order-1", Some(U256::from(50)))
            .await;
        assert!(result.is_ok());

        let confirmed = rx.try_recv().unwrap();
        assert_eq!(confirmed.order_id, "order-1");
        assert_eq!(confirmed.actual_fill, U256::from(50));
        assert_eq!(confirmed.position_after, U256::from(150));
    }

    #[test]
    fn test_clone_shares_pending() {
        let chain_client = mock_chain_client();
        let config = mock_config(ConfirmationMode::Hybrid);
        let (tracker1, _rx) = ConfirmationTracker::new(chain_client, config);

        let pending = PendingOrder {
            order_id: "order-1".into(),
            market_id: "market-1".into(),
            token_id: U256::from(123),
            side: OrderSide::Buy,
            expected_price: 0.5,
            position_before: U256::from(100),
            submitted_at: Instant::now(),
            hedge_intent: None,
        };


        tracker1.register_order(pending);

        let tracker2 = tracker1.clone();
        assert_eq!(tracker2.pending_count(), 1);
    }
}
