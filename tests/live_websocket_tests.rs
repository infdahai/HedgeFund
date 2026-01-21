use opinion_hft::config::Config;
use opinion_hft::data::websocket::{WebSocketClient, WsEvent};
use std::time::{Duration, Instant};
use tokio::time::{sleep, timeout};
use tracing::info;

mod common;
use common::init_proxy_env;

fn load_config() -> Config {
    init_proxy_env();
    Config::from_env().expect("Failed to load config from .env")
}

#[tokio::test]
async fn test_websocket_connection() {
    let config = load_config();
    let mut ws_client = WebSocketClient::new(
        &config.websocket,
        config.api.api_key.clone(),
        config.environment,
    );
    let mut event_rx = ws_client.subscribe_events();

    ws_client.connect().await.expect("Failed to connect");

    let event = timeout(Duration::from_secs(5), event_rx.recv())
        .await
        .expect("Timeout waiting for Connected event")
        .expect("Failed to receive event");

    assert!(matches!(event, WsEvent::Connected));
    assert!(ws_client.is_connected().await);

    ws_client.close().await.ok();
}

#[tokio::test]
async fn test_websocket_market_data() {
    let config = load_config();
    let mut ws_client = WebSocketClient::new(
        &config.websocket,
        config.api.api_key.clone(),
        config.environment,
    );
    let mut event_rx = ws_client.subscribe_events();

    ws_client.connect().await.expect("Failed to connect");

    sleep(Duration::from_secs(2)).await;

    let result = timeout(Duration::from_secs(15), event_rx.recv()).await;

    if let Ok(Ok(WsEvent::MarketData { pair: _, data })) = result {
        println!("Received market data: {}", data);
    }
}

#[tokio::test]
async fn test_websocket_trade_channels() {
    let config = load_config();
    let mut ws_client = WebSocketClient::new(
        &config.websocket,
        config.api.api_key.clone(),
        config.environment,
    );
    let _event_rx = ws_client.subscribe_events();

    ws_client.connect().await.expect("Failed to connect");

    ws_client
        .subscribe("trade.order.update")
        .await
        .expect("Failed to subscribe");
    ws_client
        .subscribe("trade.record.new")
        .await
        .expect("Failed to subscribe");

    sleep(Duration::from_secs(3)).await;

    assert!(ws_client.is_connected().await);

    ws_client.close().await.ok();
}

#[tokio::test]
async fn test_websocket_message_parsing() {
    let config = load_config();
    let mut ws_client = WebSocketClient::new(
        &config.websocket,
        config.api.api_key.clone(),
        config.environment,
    );

    ws_client.connect().await.expect("Failed to connect");

    let order_json = r#"{"channel":"trade.order.update","data":{"order_id":"test_1","status":"Finish","shares":"1000","filled_shares":"1000"}}"#;
    let event = WebSocketClient::parse_message(order_json).expect("Failed to parse");

    if let WsEvent::OrderUpdate(update) = event {
        assert_eq!(update.order_id, "test_1");
        assert_eq!(update.status, "Finish");
    } else {
        panic!("Expected OrderUpdate event");
    }

    ws_client.close().await.ok();
}

#[tokio::test]
async fn test_websocket_order_lifecycle() {
    let config = load_config();
    let mut ws_client = WebSocketClient::new(
        &config.websocket,
        config.api.api_key.clone(),
        config.environment,
    );
    let mut event_rx = ws_client.subscribe_events();

    ws_client.connect().await.expect("Failed to connect");
    ws_client
        .subscribe("trade.order.update")
        .await
        .expect("Failed to subscribe");

    // 等待订单状态更新示例
    let timeout_duration = Duration::from_secs(10);
    let mut received_update = false;

    let deadline = Instant::now() + timeout_duration;
    while Instant::now() < deadline {
        match timeout(Duration::from_secs(2), event_rx.recv()).await {
            Ok(Ok(event)) => {
                info!("Received WebSocket event: {:?}", event);
                received_update = true;
                break;
            }
            _ => continue,
        }
    }

    info!(
        "Order lifecycle test completed. Received update: {}",
        received_update
    );
    ws_client.close().await.ok();
}

#[tokio::test]
async fn test_websocket_reconnection() {
    let config = load_config();
    let mut ws_client = WebSocketClient::new(
        &config.websocket,
        config.api.api_key.clone(),
        config.environment,
    );

    ws_client.connect().await.expect("Failed to connect");
    info!("✅ Initial connection successful");

    // 模拟断开
    ws_client.close().await.expect("Failed to close");
    sleep(Duration::from_millis(500)).await;
    info!("✅ Connection closed");

    // 重新连接
    ws_client.connect().await.expect("Failed to reconnect");
    info!("✅ Reconnection successful");

    ws_client.close().await.ok();
}
