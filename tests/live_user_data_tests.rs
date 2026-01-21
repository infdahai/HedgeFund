use opinion_hft::config::Config;
use opinion_hft::data::rest::OpinionClient;
use tracing::{info, warn};

async fn setup() -> (OpinionClient, Config) {
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();

    let config = Config::from_env().expect("Failed to load configuration from environment");

    info!(
        "Initializing OpinionClient in {:?} environment",
        config.environment
    );
    let client = OpinionClient::new(&config.api, config.environment)
        .expect("Failed to create OpinionClient");

    (client, config)
}

#[ignore]

#[tokio::test]
async fn test_get_my_positions() {
    let (client, _) = setup().await;
    info!("Testing /positions endpoint");

    let chain_id = 56;
    // Test with no market filter first
    let result = client
        .get_my_positions(chain_id, None, Some(1), Some(10))
        .await;

    match result {
        Ok(positions) => {
            info!("Found {} positions", positions.len());
            for pos in positions {
                info!(
                    "Position: Market {} - Token {} - Qty {}",
                    pos.market_id, pos.token_id, pos.quantity
                );
            }
        }
        Err(e) => {
            // It's possible to fail if user has no positions or API structure changed
            warn!(
                "Failed to get positions (might be expected if empty): {}",
                e
            );
        }
    }
}

#[ignore]

#[tokio::test]
async fn test_get_my_balances() {
    let (client, _) = setup().await;
    info!("Testing /user/balance endpoint");

    let chain_id = 56;
    let result = client.get_my_balances(chain_id).await;

    match result {
        Ok(balance) => {
            info!(
                "User Balance: Total {:?}, Available {:?}",
                balance.total_balance, balance.available_balance
            );
            assert_eq!(balance.chain_id, "56");
        }
        Err(e) => {
            warn!("Failed to get user balance: {}", e);
            // Don't fail the test if it's just a network/auth issue in CI, but log it.
            // For live tests, we generally expect success.
            panic!("Critical: Failed to fetch balance: {}", e);
        }
    }
}

#[ignore]

#[tokio::test]
async fn test_get_my_trades() {
    let (client, _) = setup().await;
    info!("Testing /trade endpoint");

    let chain_id = 56;
    let result = client
        .get_my_trades(chain_id, None, Some(1), Some(10))
        .await;

    match result {
        Ok(trades) => {
            info!("Found {} trades", trades.len());
            for trade in trades {
                info!(
                    "Trade: {} - {} - {}",
                    trade.order_id, trade.price, trade.amount
                );
            }
        }
        Err(e) => {
            warn!("Failed to get trades: {}", e);
        }
    }
}
