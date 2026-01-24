use alloy_primitives::U256;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::config::{MAX_PRICE, MIN_PRICE};
use crate::types::{EventEnvelope, OrderIntent, StrategyCommand};

pub struct StrategyEngine {
    event_rx: mpsc::Receiver<EventEnvelope>,
    command_tx: mpsc::Sender<StrategyCommand>,
}

impl StrategyEngine {
    pub fn new(
        event_rx: mpsc::Receiver<EventEnvelope>,
        command_tx: mpsc::Sender<StrategyCommand>,
    ) -> Self {
        Self {
            event_rx,
            command_tx,
        }
    }

    pub async fn run(mut self) {
        while let Some(event) = self.event_rx.recv().await {
            match event {
                EventEnvelope::Signal(signal) => {
                    if let Err(reason) = Self::validate_intent(&signal.intent) {
                        warn!(
                            market_id = %signal.intent.market_id,
                            reason,
                            "dropping invalid signal"
                        );
                        continue;
                    }
                    if let Err(e) = self
                        .command_tx
                        .send(StrategyCommand::SubmitOrder(signal.intent))
                        .await
                    {
                        warn!(error = %e, "failed to forward signal to execution queue");
                    }
                }
                EventEnvelope::OrderUpdate(update) => {
                    debug!(
                        order_id = %update.order_id,
                        status = %update.status,
                        "order update event"
                    );
                }
                EventEnvelope::MarketData(_) => {}
                EventEnvelope::Shutdown => break,
            }
        }
    }

    fn validate_intent(intent: &OrderIntent) -> Result<(), &'static str> {
        if !intent.price.is_finite() {
            return Err("price not finite");
        }
        if intent.price < MIN_PRICE || intent.price > MAX_PRICE {
            return Err("price out of bounds");
        }
        if intent.quantity == U256::ZERO {
            return Err("quantity is zero");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::U256;
    use tokio::time::{timeout, Duration};

    use crate::types::{CrossChainSignal, EventEnvelope, OrderIntent, OrderSide, StrategyCommand};

    #[tokio::test]
    async fn test_strategy_engine_forwards_valid_signal() {
        let (event_tx, event_rx) = mpsc::channel(4);
        let (command_tx, mut command_rx) = mpsc::channel(4);

        tokio::spawn(StrategyEngine::new(event_rx, command_tx).run());

        let intent = OrderIntent::new(
            "m1",
            U256::from(1u64),
            OrderSide::Buy,
            0.5,
            U256::from(2u64),
        );
        let signal = CrossChainSignal {
            source: "test".to_string(),
            intent,
            received_at: std::time::Instant::now(),
        };

        event_tx
            .send(EventEnvelope::Signal(signal))
            .await
            .expect("send signal");

        let cmd = timeout(Duration::from_millis(50), command_rx.recv())
            .await
            .expect("command receive timeout")
            .expect("command");

        match cmd {
            StrategyCommand::SubmitOrder(intent) => {
                assert!((intent.price - 0.5).abs() < f64::EPSILON);
                assert_eq!(intent.market_id, "m1");
            }
            _ => panic!("unexpected command"),
        }
    }

    #[tokio::test]
    async fn test_strategy_engine_drops_invalid_signal() {
        let (event_tx, event_rx) = mpsc::channel(4);
        let (command_tx, mut command_rx) = mpsc::channel(4);

        tokio::spawn(StrategyEngine::new(event_rx, command_tx).run());

        let intent = OrderIntent::new("m2", U256::from(1u64), OrderSide::Buy, 0.0, U256::ZERO);
        let signal = CrossChainSignal {
            source: "test".to_string(),
            intent,
            received_at: std::time::Instant::now(),
        };

        event_tx
            .send(EventEnvelope::Signal(signal))
            .await
            .expect("send signal");

        let result = timeout(Duration::from_millis(50), command_rx.recv()).await;
        assert!(
            result.is_err(),
            "invalid signal should not produce a command"
        );
    }
}
