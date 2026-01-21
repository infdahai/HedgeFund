use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Deserializer, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
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
    Disconnected {
        reason: String,
    },
    OrderUpdate(OrderUpdate),
    TradeRecord(TradeRecord),
    MarketData {
        pair: String,
        data: String,
    },
    MarketTrade {
        pair: String,
        data: String,
    },
    MarketDepthDiff {
        market_id: i64,
        outcome: Option<String>,
        outcome_side: Option<i64>,
        side: String,
        price: f64,
        size: f64,
    },
}

#[derive(Debug, Clone, Serialize)]
struct SubscribeMessage {
    action: String,
    channel: String,
}

#[derive(Debug, Clone, Serialize)]
struct SubscribeMarketMessage {
    action: String,
    channel: String,
    #[serde(rename = "marketId")]
    market_id: i64,
}

#[derive(Clone)]
pub struct WebSocketClient {
    config: WebSocketConfig,
    api_key: String,
    event_tx: broadcast::Sender<WsEvent>,
    command_tx: Option<mpsc::Sender<WsCommand>>,
    is_connected: Arc<RwLock<bool>>,
    diff_counter: Arc<AtomicUsize>,
    environment: crate::config::Environment,
}

enum WsCommand {
    Subscribe(String),
    Unsubscribe(String),
    SubscribeMarket { channel: String, market_id: i64 },
    UnsubscribeMarket { channel: String, market_id: i64 },
    Raw(String),
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
            diff_counter: Arc::new(AtomicUsize::new(0)),
            environment,
        }
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<WsEvent> {
        self.event_tx.subscribe()
    }

    pub async fn connect(&mut self) -> Result<()> {
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            let provider = rustls::crypto::ring::default_provider();
            let _ = provider.install_default();
        }
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
        let diff_counter = Arc::clone(&self.diff_counter);
        let diff_counter_log = Arc::clone(&self.diff_counter);
        let ping_interval = self.config.ping_interval;

        tokio::spawn(async move {
            let mut diff_timer = interval(Duration::from_secs(5));
            loop {
                diff_timer.tick().await;
                let count = diff_counter_log.load(Ordering::Relaxed);
                tracing::info!("WS Health: received_diffs_total={}", count);
            }
        });

        tokio::spawn(async move {
            let mut ping_timer = interval(ping_interval);

            loop {
                tokio::select! {
                    Some(msg) = read.next() => {
                        match msg {
                            Ok(Message::Text(text)) => {
                                let sample = truncate_ws_message(&text, 500);
                                tracing::debug!("Received WS message: {}", sample);
                                if text.contains("market.depth.diff") {
                                    diff_counter.fetch_add(1, Ordering::Relaxed);
                                }
                                if let Some(event) = Self::parse_message(&text) {
                                    let _ = event_tx.send(event);
                                } else {
                                    tracing::debug!("Unparsed WS message: {}", sample);
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
                                    action: "subscribe".into(),
                                    channel,
                                };
                                if let Ok(json) = serde_json::to_string(&msg) {
                                    let _ = write.send(Message::Text(json)).await;
                                }
                            }
                            WsCommand::Unsubscribe(channel) => {
                                let msg = SubscribeMessage {
                                    action: "unsubscribe".into(),
                                    channel,
                                };
                                if let Ok(json) = serde_json::to_string(&msg) {
                                    let _ = write.send(Message::Text(json)).await;
                                }
                            }
                            WsCommand::SubscribeMarket { channel, market_id } => {
                                let msg = SubscribeMarketMessage {
                                    action: "SUBSCRIBE".into(),
                                    channel,
                                    market_id,
                                };
                                if let Ok(json) = serde_json::to_string(&msg) {
                                    let _ = write.send(Message::Text(json)).await;
                                }
                            }
                            WsCommand::UnsubscribeMarket { channel, market_id } => {
                                let msg = SubscribeMarketMessage {
                                    action: "UNSUBSCRIBE".into(),
                                    channel,
                                    market_id,
                                };
                                if let Ok(json) = serde_json::to_string(&msg) {
                                    let _ = write.send(Message::Text(json)).await;
                                }
                            }
                            WsCommand::Raw(payload) => {
                                let _ = write.send(Message::Text(payload)).await;
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
        .or_else(|| parse_depth_diff(text))
    }

    pub async fn subscribe(&self, channel: &str) -> Result<()> {
        if let Some(tx) = &self.command_tx {
            tracing::debug!("WS subscribe: {}", channel);
            tx.send(WsCommand::Subscribe(channel.to_string()))
                .await
                .map_err(|_| Error::ChannelError("command channel closed".into()))?;
        }
        Ok(())
    }

    pub async fn unsubscribe(&self, channel: &str) -> Result<()> {
        if let Some(tx) = &self.command_tx {
            tracing::debug!("WS unsubscribe: {}", channel);
            tx.send(WsCommand::Unsubscribe(channel.to_string()))
                .await
                .map_err(|_| Error::ChannelError("command channel closed".into()))?;
        }
        Ok(())
    }

    pub async fn subscribe_depth_diff(&self, market_id: i64) -> Result<()> {
        if let Some(tx) = &self.command_tx {
            tracing::debug!("WS subscribe: market.depth.diff market_id={}", market_id);
            tx.send(WsCommand::SubscribeMarket {
                channel: "market.depth.diff".to_string(),
                market_id,
            })
            .await
            .map_err(|_| Error::ChannelError("command channel closed".into()))?;
        }
        Ok(())
    }

    pub async fn unsubscribe_depth_diff(&self, market_id: i64) -> Result<()> {
        if let Some(tx) = &self.command_tx {
            tracing::debug!("WS unsubscribe: market.depth.diff market_id={}", market_id);
            tx.send(WsCommand::UnsubscribeMarket {
                channel: "market.depth.diff".to_string(),
                market_id,
            })
            .await
            .map_err(|_| Error::ChannelError("command channel closed".into()))?;
        }
        Ok(())
    }

    pub async fn send_raw(&self, payload: &str) -> Result<()> {
        if let Some(tx) = &self.command_tx {
            tracing::debug!("WS send raw payload");
            tx.send(WsCommand::Raw(payload.to_string()))
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

fn truncate_ws_message(message: &str, max_chars: usize) -> String {
    if message.chars().count() <= max_chars {
        return message.to_string();
    }

    let mut truncated = message.chars().take(max_chars).collect::<String>();
    truncated.push_str("...");
    truncated
}

fn deserialize_option_i64<'de, D>(deserializer: D) -> std::result::Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    let parsed = match value {
        Some(serde_json::Value::Number(number)) => number.as_i64(),
        Some(serde_json::Value::String(raw)) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                None
            } else {
                trimmed.parse::<i64>().ok()
            }
        }
        _ => None,
    };

    Ok(parsed)
}

fn parse_depth_diff(text: &str) -> Option<WsEvent> {
    #[derive(Deserialize)]
    struct DepthDiffMessage {
        #[serde(rename = "msgType")]
        msg_type: String,
        #[serde(rename = "marketId")]
        market_id: i64,
        #[serde(default)]
        outcome: Option<String>,
        #[serde(
            rename = "outcomeSide",
            default,
            deserialize_with = "deserialize_option_i64"
        )]
        outcome_side: Option<i64>,
        side: String,
        price: String,
        size: String,
    }

    let diff: DepthDiffMessage = serde_json::from_str(text).ok()?;
    if !diff.msg_type.eq_ignore_ascii_case("market.depth.diff") {
        return None;
    }

    tracing::debug!("WS depth diff raw: {}", text);

    let price = diff.price.parse::<f64>().ok()?;
    let size = diff.size.parse::<f64>().ok()?;

    Some(WsEvent::MarketDepthDiff {
        market_id: diff.market_id,
        outcome: diff.outcome,
        outcome_side: diff.outcome_side,
        side: diff.side,
        price,
        size,
    })
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
