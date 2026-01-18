use tokio::sync::broadcast;

#[derive(Debug, Clone)]
pub enum MarketEvent {
    OrderbookUpdate { pair: String },
    TradeExecuted { pair: String },
    OrderStatusChanged { order_id: String, status: String },
}

pub struct EventBus {
    sender: broadcast::Sender<MarketEvent>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<MarketEvent> {
        self.sender.subscribe()
    }

    pub fn publish(
        &self,
        event: MarketEvent,
    ) -> Result<usize, broadcast::error::SendError<MarketEvent>> {
        self.sender.send(event)
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(1024)
    }
}
