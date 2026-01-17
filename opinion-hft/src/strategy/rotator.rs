//! Market Rotator for random sampling of the market universe.
//!
//! Helps discover opportunities in long-tail markets by periodically
//! rotating WebSocket subscriptions.

use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;
use tracing::{info, warn};

use crate::data::websocket::WebSocketClient;
use crate::error::Result;
use crate::strategy::market_scanner::MarketScanner;

pub struct MarketRotator {
    scanner: Arc<MarketScanner>,
    ws_client: WebSocketClient,
    batch_size: usize,
    interval: Duration,
}

impl MarketRotator {
    pub fn new(
        scanner: Arc<MarketScanner>,
        ws_client: WebSocketClient,
        batch_size: usize,
        interval: Duration,
    ) -> Self {
        Self {
            scanner,
            ws_client,
            batch_size,
            interval,
        }
    }

    pub async fn start(&self) -> Result<()> {
        let mut timer = interval(self.interval);
        let mut current_subscriptions: Vec<String> = Vec::new();

        info!("Starting MarketRotator with batch size {}", self.batch_size);

        loop {
            timer.tick().await;

            // 1. Select random markets
            let random_markets = self.scanner.get_random_markets(self.batch_size);
            if random_markets.is_empty() {
                warn!("No markets found to rotate");
                continue;
            }

            // 2. Unsubscribe from old
            for channel in &current_subscriptions {
                if let Err(e) = self.ws_client.unsubscribe(channel).await {
                    warn!("Failed to unsubscribe from {}: {}", channel, e);
                }
            }
            current_subscriptions.clear();

            // 3. Subscribe to new
            for market in random_markets {
                // Subscribe to both Yes and No pair data
                // Format: market.data.{pair}
                
                let yes_channel = format!("market.data.{}", market.yes_pair);
                let no_channel = format!("market.data.{}", market.no_pair);

                if let Err(e) = self.ws_client.subscribe(&yes_channel).await {
                    warn!("Failed to subscribe to {}: {}", yes_channel, e);
                } else {
                    current_subscriptions.push(yes_channel);
                }

                if let Err(e) = self.ws_client.subscribe(&no_channel).await {
                    warn!("Failed to subscribe to {}: {}", no_channel, e);
                } else {
                    current_subscriptions.push(no_channel);
                }
            }

            info!("Rotated to {} new markets", self.batch_size);
        }
    }
}
