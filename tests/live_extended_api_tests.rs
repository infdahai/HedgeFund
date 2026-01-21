use opinion_hft::config::Config;
use opinion_hft::data::rest::OpinionClient;
use tracing::{info, warn};

mod common;
use common::init_proxy_env;

async fn setup() -> (OpinionClient, Config) {
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();
    init_proxy_env();

    let config = Config::from_env().expect("Failed to load configuration from environment");

    info!(
        "Initializing OpinionClient in {:?} environment",
        config.environment
    );
    let client = OpinionClient::new(&config.api, config.environment)
        .expect("Failed to create OpinionClient");

    (client, config)
}

#[tokio::test]
async fn test_get_latest_price() {
    let (client, _) = setup().await;

    // 获取一个活跃市场的token ID
    let markets = client
        .get_markets_with_params(Some("activated"), Some(1))
        .await
        .unwrap();
    let market = markets.first().expect("No active market found");
    let token_id = market.yes_token_id.as_ref().expect("No token ID found");

    // 测试最新价格
    let price = client.get_latest_price(token_id).await.unwrap();

    info!("Latest price: {} @ {}", price.price, price.timestamp);
    assert!(!price.price.is_empty());
    assert!(price.timestamp > 0);
}

#[tokio::test]
async fn test_get_price_history() {
    let (client, _) = setup().await;

    let markets = client
        .get_markets_with_params(Some("activated"), Some(1))
        .await
        .unwrap();
    let market = markets.first().expect("No active market found");
    let token_id = market.yes_token_id.as_ref().expect("No token ID found");

    // 测试价格历史
    let history = client
        .get_price_history(token_id, "1h", None, None)
        .await
        .unwrap();

    info!("Price history entries: {}", history.len());
    assert!(!history.is_empty());

    for entry in &history {
        assert!(!entry.price.is_empty());
        assert!(entry.timestamp > 0);
    }
}

#[tokio::test]
async fn test_get_order_by_id() {
    let (client, _) = setup().await;

    // 先获取一个订单列表
    let orders = client.get_orders(None, Some(1)).await.unwrap();

    if orders.is_empty() {
        warn!("No orders found to test get_order_by_id");
        return;
    }

    let order_id = &orders[0].order_id;
    info!("Testing get_order_by_id for: {}", order_id);

    // 测试单个订单查询
    let order = client.get_order_by_id(order_id).await.unwrap();

    assert_eq!(order.order_id, *order_id);
    assert!(!order.market_title.is_empty());
}
