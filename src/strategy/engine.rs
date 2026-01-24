use tokio::sync::mpsc;
use tracing::debug;

use crate::types::{EventEnvelope, StrategyCommand};

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
                    let _ = self
                        .command_tx
                        .send(StrategyCommand::SubmitOrder(signal.intent))
                        .await;
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
}
