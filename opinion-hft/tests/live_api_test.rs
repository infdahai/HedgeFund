use alloy_primitives::{Address, U256};
use opinion_hft::chain::ChainClient;
use opinion_hft::data::rest::OpinionClient;
use opinion_hft::data::websocket::{WebSocketClient, WsEvent};
use std::time::Duration;
use tokio::time::timeout;

fn load_test_config() -> opinion_hft::config::Config {
    opinion_hft::config::Config::from_env().expect("Failed to load config from .env")
}

#[tokio::test]
#[ignore]
async fn test_rest_get_markets() {
    let config = load_test_config();
    let client = OpinionClient::new(&config.api, config.environment)
        .expect("Failed to create REST client");

    let result: serde_json::Value = client.get("markets").await.expect("Failed to get markets");

    println!("Markets response: {:#?}", result);
    assert!(
        result.is_array() || result.is_object(),
        "Expected array or object response"
    );
}

#[tokio::test]
#[ignore]
async fn test_rest_get_orderbook() {
    let config = load_test_config();
    let client = OpinionClient::new(&config.api, config.environment)
        .expect("Failed to create REST client");

    let result: serde_json::Value = client
        .get("orderbook/BTC-USD")
        .await
        .expect("Failed to get orderbook");

    println!("Orderbook response: {:#?}", result);
}

#[tokio::test]
#[ignore]
async fn test_websocket_connect_and_receive() {
    let config = load_test_config();
    let mut ws_client = WebSocketClient::new(&config.websocket, config.api.api_key.clone());

    let mut event_rx = ws_client.subscribe_events();

    ws_client
        .connect()
        .await
        .expect("Failed to connect to WebSocket");

    let connect_event = timeout(Duration::from_secs(5), event_rx.recv())
        .await
        .expect("Timeout waiting for connect event")
        .expect("Failed to receive event");

    match connect_event {
        WsEvent::Connected => println!("WebSocket connected successfully!"),
        other => panic!("Expected Connected event, got: {:?}", other),
    }

    ws_client
        .subscribe("trade.order.update")
        .await
        .expect("Failed to subscribe");

    println!("Subscribed to trade.order.update channel");
    println!("Listening for events for 10 seconds...");

    let listen_result = timeout(Duration::from_secs(10), async {
        loop {
            match event_rx.recv().await {
                Ok(event) => {
                    println!("Received event: {:?}", event);
                    match &event {
                        WsEvent::OrderUpdate(update) => {
                            println!("Order update: {:?}", update);
                            return Some(event);
                        }
                        WsEvent::TradeRecord(record) => {
                            println!("Trade record: {:?}", record);
                        }
                        _ => {}
                    }
                }
                Err(e) => {
                    println!("Event receive error: {:?}", e);
                    break;
                }
            }
        }
        None
    })
    .await;

    println!("Listen result: {:?}", listen_result);

    ws_client.close().await.expect("Failed to close WebSocket");
}

#[tokio::test]
#[ignore]
async fn test_websocket_market_data() {
    let config = load_test_config();
    let mut ws_client = WebSocketClient::new(&config.websocket, config.api.api_key.clone());

    let mut event_rx = ws_client.subscribe_events();

    ws_client.connect().await.expect("Failed to connect");

    let _ = timeout(Duration::from_secs(2), event_rx.recv()).await;

    ws_client
        .subscribe("market.data.BTC-USD")
        .await
        .expect("Failed to subscribe to market data");

    println!("Subscribed to market.data.BTC-USD, listening for 15 seconds...");

    let mut received_count = 0;
    let result = timeout(Duration::from_secs(15), async {
        loop {
            if let Ok(WsEvent::MarketData { pair, data }) = event_rx.recv().await {
                println!("Market data for {}: {}", pair, data);
                received_count += 1;
                if received_count >= 3 {
                    return received_count;
                }
            }
        }
    })
    .await;

    println!(
        "Received {} market data events",
        result.unwrap_or(received_count)
    );

    ws_client.close().await.ok();
}

#[tokio::test]
#[ignore]
async fn test_chain_balance_query() {
    let config = load_test_config();

    let eoa_address: Address = config
        .chain
        .eoa_address
        .parse()
        .expect("Invalid EOA address in config");

    let chain_client =
        ChainClient::new(&config.chain, eoa_address).expect("Failed to create chain client");

    println!("Querying balance for EOA: {:?}", eoa_address);
    println!("Contract: {:?}", chain_client.contract_address());

    let token_id = U256::from(1u64);

    let balance = chain_client
        .balance_of(token_id)
        .await
        .expect("Failed to query balance");

    println!("Token ID {} balance: {}", token_id, balance);
}

#[tokio::test]
#[ignore]
async fn test_chain_batch_balance_query() {
    let config = load_test_config();

    let eoa_address: Address = config
        .chain
        .eoa_address
        .parse()
        .expect("Invalid EOA address in config");

    let chain_client =
        ChainClient::new(&config.chain, eoa_address).expect("Failed to create chain client");

    let token_ids = vec![U256::from(1u64), U256::from(2u64), U256::from(3u64)];

    println!("Querying batch balances for {} tokens", token_ids.len());

    let positions = chain_client
        .balance_of_batch(&token_ids)
        .await
        .expect("Failed to query batch balances");

    for pos in &positions {
        println!("Token {:?}: balance = {:?}", pos.token_id, pos.balance);
    }

    assert_eq!(positions.len(), token_ids.len());
}
