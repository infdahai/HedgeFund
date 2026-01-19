use alloy_primitives::{Address, U256};
use opinion_hft::auth::Eip712Signer;
use opinion_hft::auth::OrderSigner;
use opinion_hft::config::Config;
use opinion_hft::data::rest::OpinionClient;
use opinion_hft::types::order::{Order, OrderIntent, OrderSide, SignatureType};
use std::time::Duration;
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

#[tokio::test]
async fn test_place_orders_batch() {
    let (client, config) = setup().await;

    // 1. Get active market
    let markets = client
        .get_markets_with_params(Some("activated"), Some(1))
        .await
        .expect("Failed to get markets");
    if markets.is_empty() {
        warn!("No active markets found, skipping batch test");
        return;
    }
    let market = &markets[0];
    let token_id = U256::from_str_radix(&market.yes_token_id.clone().unwrap(), 10).unwrap();

    // 2. Setup signer and addresses
    let quote_token = "0x55d398326f99059fF775485246999027B3197955";
    let exchange_addr = client.get_exchange_address(56, quote_token).await.unwrap();
    let exchange_address: Address = exchange_addr.parse().unwrap();

    let mut signer = Eip712Signer::from_secret(&config.chain.private_key).unwrap();
    signer.set_verifying_contract(exchange_address);

    let auth = client.get_user_auth().await.unwrap();
    let proxy_addr = auth.wallet_users.get("56").cloned();
    let (maker, sig_type) = if let Some(proxy) = proxy_addr {
        (proxy.parse().unwrap(), SignatureType::PolyGnosisSafe)
    } else {
        (signer.address(), SignatureType::Eoa)
    };

    // 3. Prepare 2 orders
    let mut orders_payload = Vec::new();

    for i in 0..2 {
        // Unique price or salt to differentiate
        let price = 0.2 + (i as f64 * 0.1);
        let quantity = U256::from(200u128 * 1_000_000_000_000_000_000u128); // 200 shares

        let intent = OrderIntent::new(
            market.market_id.to_string(),
            token_id,
            OrderSide::Buy,
            price,
            quantity,
        );
        let (maker_amount, taker_amount) = intent.calculate_amounts();

        let order = Order::new(
            maker,
            signer.address(),
            token_id,
            maker_amount,
            taker_amount,
            OrderSide::Buy,
        )
        .with_expiration(2000000000 + i as u64)
        .with_signature_type(sig_type);

        let signature = signer.sign_order(&order).unwrap();
        let sig_hex = signature.to_hex();

        // Construct JSON payload manually to match API requirement
        let payload = serde_json::json!({
            "salt": order.salt.to_string(),
            "maker": format!("{:?}", order.maker),
            "signer": format!("{:?}", order.signer),
            "taker": format!("{:?}", order.taker),
            "tokenId": order.token_id.to_string(),
            "makerAmount": order.maker_amount.to_string(),
            "takerAmount": order.taker_amount.to_string(),
            "expiration": order.expiration.to_string(),
            "nonce": order.nonce.to_string(),
            "feeRateBps": order.fee_rate_bps.to_string(),
            "side": "0", // Buy
            "signatureType": sig_type.as_u8().to_string(),
            "signature": sig_hex.clone(),
            "sign": sig_hex,
            "timestamp": chrono::Utc::now().timestamp(),
            "topicId": market.market_id,
            "tradingMethod": 2,
            "contractAddress": "",
            "currencyAddress": quote_token,
            "orderExpTime": "0",
            "safeRate": "0",
            "price": price.to_string(),
        });

        orders_payload.push(payload);
    }

    info!("Placing {} orders in batch...", orders_payload.len());
    let results = client.place_orders_batch(orders_payload).await.unwrap();

    assert_eq!(results.len(), 2);
    let successful_orders: Vec<String> = results
        .iter()
        .filter_map(|r| {
            if r.success {
                info!("Order {} placed successfully: {:?}", r.index, r.order_id);
                r.order_id.clone()
            } else {
                warn!("Order {} failed: {:?}", r.index, r.error);
                None
            }
        })
        .collect();

    // Clean up
    if !successful_orders.is_empty() {
        info!("Cleaning up {} orders...", successful_orders.len());
        tokio::time::sleep(Duration::from_secs(1)).await; // Wait for processing
        let cancel_results = client.cancel_orders_batch(successful_orders).await.unwrap();
        for res in cancel_results {
            info!(
                "Cancel result for index {}: success={}",
                res.index, res.success
            );
        }
    }
}

#[tokio::test]
async fn test_cancel_all_orders() {
    let (client, _) = setup().await;

    // Attempt to cancel all (even if none exist, should return success with 0 count)
    info!("Testing Cancel All Orders...");
    let result = client.cancel_all_orders(None, None).await;

    match result {
        Ok(res) => {
            info!(
                "Cancel All Result: Total={}, Cancelled={}, Failed={}",
                res.total_orders, res.cancelled, res.failed
            );
        }
        Err(e) => {
            warn!("Cancel All failed: {}", e);
        }
    }
}
