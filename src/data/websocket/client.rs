use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};
use tokio::time::interval;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

use crate::config::WebSocketConfig;
use crate::error::{Error, Result};

use super::messages::{OrderUpdate, TradeRecord};

#[derive(Debug, Clone)]
pub enum WsEvent {
    Connected,
    Disconnected { reason: String },
    OrderUpdate(OrderUpdate),
    TradeRecord(TradeRecord),
    MarketData { pair: String, data: String },
    MarketTrade { pair: String, data: String },
}

#[derive(Debug, Clone, Serialize)]
struct SubscribeMessage {
    #[serde(rename = "type")]
    msg_type: String,
    channel: String,
}

pub struct WebSocketClient {
    config: WebSocketConfig,
    api_key: String,
    event_tx: broadcast::Sender<WsEvent>,
    command_tx: Option<mpsc::Sender<WsCommand>>,
    is_connected: Arc<RwLock<bool>>,
    environment: crate::config::Environment,
}

enum WsCommand {
    Subscribe(String),
    Unsubscribe(String),
    Close,
}

use tokio_tungstenite::tungstenite::client::IntoClientRequest;

impl WebSocketClient {
    pub fn new(
        config: &WebSocketConfig,
        api_key: String,
        environment: crate::config::Environment,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(1024);

        Self {
            config: config.clone(),
            api_key,
            event_tx,
            command_tx: None,
            is_connected: Arc::new(RwLock::new(false)),
            environment,
        }
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<WsEvent> {
        self.event_tx.subscribe()
    }

    pub async fn connect(&mut self) -> Result<()> {
        let base_url = self.config.url.trim_end_matches('/');
        let url_str = format!("{}/?apikey={}", base_url, self.api_key);

        tracing::info!("Connecting to WebSocket: {}?apikey=REDACTED", base_url);

        // Build request with headers to avoid 400 Bad Request (WAF/API check)
        let mut request = url_str
            .into_client_request()
            .map_err(|e| Error::WebSocketConnectionError(format!("invalid URL: {}", e)))?;

        let headers = request.headers_mut();
        headers.insert("User-Agent", "opinion-hft/1.0".parse().unwrap());
        // Some APIs require Origin matching the host
        headers.insert("Origin", "https://opinion.trade".parse().unwrap());

        if self.environment.is_local() {
            tracing::info!(
                "Local environment detected. Ensure HTTPS_PROXY is configured if proxy is needed."
            );
        }

        let (ws_stream, _) = connect_async(request)
            .await
            .map_err(|e| Error::WebSocketConnectionError(e.to_string()))?;

        let (mut write, mut read) = ws_stream.split();

        let (command_tx, mut command_rx) = mpsc::channel::<WsCommand>(100);
        self.command_tx = Some(command_tx);

        *self.is_connected.write().await = true;
        let _ = self.event_tx.send(WsEvent::Connected);

        let event_tx = self.event_tx.clone();
        let is_connected = Arc::clone(&self.is_connected);
        let ping_interval = self.config.ping_interval;

        tokio::spawn(async move {
            let mut ping_timer = interval(ping_interval);

            loop {
                tokio::select! {
                    Some(msg) = read.next() => {
                        match msg {
                            Ok(Message::Text(text)) => {
                                if let Some(event) = Self::parse_message(&text) {
                                    let _ = event_tx.send(event);
                                }
                            }
                            Ok(Message::Ping(data)) => {
                                let _ = write.send(Message::Pong(data)).await;
                            }
                            Ok(Message::Close(_)) => {
                                *is_connected.write().await = false;
                                let _ = event_tx.send(WsEvent::Disconnected {
                                    reason: "server closed connection".into(),
                                });
                                break;
                            }
                            Err(e) => {
                                *is_connected.write().await = false;
                                let _ = event_tx.send(WsEvent::Disconnected {
                                    reason: e.to_string(),
                                });
                                break;
                            }
                            _ => {}
                        }
                    }
                    Some(cmd) = command_rx.recv() => {
                        match cmd {
                            WsCommand::Subscribe(channel) => {
                                let msg = SubscribeMessage {
                                    msg_type: "subscribe".into(),
                                    channel,
                                };
                                if let Ok(json) = serde_json::to_string(&msg) {
                                    let _ = write.send(Message::Text(json)).await;
                                }
                            }
                            WsCommand::Unsubscribe(channel) => {
                                let msg = SubscribeMessage {
                                    msg_type: "unsubscribe".into(),
                                    channel,
                                };
                                if let Ok(json) = serde_json::to_string(&msg) {
                                    let _ = write.send(Message::Text(json)).await;
                                }
                            }
                            WsCommand::Close => {
                                let _ = write.send(Message::Close(None)).await;
                                *is_connected.write().await = false;
                                break;
                            }
                        }
                    }
                    _ = ping_timer.tick() => {
                        let _ = write.send(Message::Ping(vec![])).await;
                    }
                }
            }
        });

        Ok(())
    }

    pub fn parse_message(text: &str) -> Option<WsEvent> {
        #[derive(Deserialize)]
        struct GenericMessage {
            channel: Option<String>,
            data: Option<serde_json::Value>,
        }

        let mut buffer = text.as_bytes().to_vec();
        let msg: GenericMessage = simd_json::from_slice(&mut buffer).ok()?;

        match msg.channel.as_deref() {
            Some("trade.order.update") => {
                let update: OrderUpdate = serde_json::from_value(msg.data?).ok()?;
                Some(WsEvent::OrderUpdate(update))
            }
            Some("trade.record.new") => {
                let record: TradeRecord = serde_json::from_value(msg.data?).ok()?;
                Some(WsEvent::TradeRecord(record))
            }
            Some(channel) if channel.starts_with("market.data.") => {
                let pair = channel.strip_prefix("market.data.")?.to_string();
                Some(WsEvent::MarketData {
                    pair,
                    data: msg.data?.to_string(),
                })
            }
            Some(channel) if channel.starts_with("market.trade.") => {
                let pair = channel.strip_prefix("market.trade.")?.to_string();
                Some(WsEvent::MarketTrade {
                    pair,
                    data: msg.data?.to_string(),
                })
            }
            _ => None,
        }
    }

    pub async fn subscribe(&self, channel: &str) -> Result<()> {
        if let Some(tx) = &self.command_tx {
            tx.send(WsCommand::Subscribe(channel.to_string()))
                .await
                .map_err(|_| Error::ChannelError("command channel closed".into()))?;
        }
        Ok(())
    }

    pub async fn unsubscribe(&self, channel: &str) -> Result<()> {
        if let Some(tx) = &self.command_tx {
            tx.send(WsCommand::Unsubscribe(channel.to_string()))
                .await
                .map_err(|_| Error::ChannelError("command channel closed".into()))?;
        }
        Ok(())
    }

    pub async fn close(&self) -> Result<()> {
        if let Some(tx) = &self.command_tx {
            let _ = tx.send(WsCommand::Close).await;
        }
        Ok(())
    }

    pub async fn is_connected(&self) -> bool {
        *self.is_connected.read().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn mock_config() -> WebSocketConfig {
        WebSocketConfig {
            url: "wss://ws.opinion.trade".into(),
            reconnect: crate::config::ReconnectConfig::default(),
            ping_interval: Duration::from_secs(30),
            pong_timeout: Duration::from_secs(10),
        }
    }

    #[test]
    fn test_websocket_client_creation() {
        let config = mock_config();
        let client = WebSocketClient::new(
            &config,
            "test-key".into(),
            crate::config::Environment::default(),
        );
        assert!(client.command_tx.is_none());
    }

    #[test]
    fn test_event_subscription() {
        let config = mock_config();
        let client = WebSocketClient::new(
            &config,
            "test-key".into(),
            crate::config::Environment::default(),
        );
        let _rx = client.subscribe_events();
    }

    #[test]
    fn test_parse_order_update() {
        let json = r#"{"channel":"trade.order.update","data":{"order_id":"123","status":"Finish","shares":"100","filled_shares":"100"}}"#;
        let event = WebSocketClient::parse_message(json);
        assert!(matches!(event, Some(WsEvent::OrderUpdate(_))));
    }

    #[test]
    fn test_parse_trade_record() {
        let json = r#"{"channel":"trade.record.new","data":{"order_id":"123","tx_hash":"0xabc","amount":"100"}}"#;
        let event = WebSocketClient::parse_message(json);
        assert!(matches!(event, Some(WsEvent::TradeRecord(_))));
    }

    #[test]
    fn test_parse_market_data() {
        let json = r#"{"channel":"market.data.BTC-YES","data":{"bids":[],"asks":[]}}"#;
        let event = WebSocketClient::parse_message(json);
        assert!(matches!(event, Some(WsEvent::MarketData { pair, .. }) if pair == "BTC-YES"));
    }

    #[test]
    fn test_parse_market_trade() {
        let json = r#"{"channel":"market.trade.BTC-YES","data":{"price":"0.5","amount":"100"}}"#;
        let event = WebSocketClient::parse_message(json);
        assert!(matches!(event, Some(WsEvent::MarketTrade { pair, .. }) if pair == "BTC-YES"));
    }

    #[test]
    fn test_parse_unknown_message() {
        let json = r#"{"channel":"unknown","data":{}}"#;
        let event = WebSocketClient::parse_message(json);
        assert!(event.is_none());
    }
}
