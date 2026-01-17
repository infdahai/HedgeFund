//! Integration tests for real trading scenarios
//!
//! These tests interact with the live Opinion Trade API.
//! Run with: cargo test --test trading_scenarios -- --ignored --nocapture

use alloy_primitives::{Address, U256};
use opinion_hft::{
    config::Config,
    data::rest::OpinionClient,
    types::order::{Order, OrderIntent, OrderSide, SignatureType},
    auth::{Eip712Signer, OrderSigner},
};

fn load_test_config() -> Config {
    Config::from_env().expect("Failed to load config from .env")
}

/// Test: Fetch quote tokens and exchange address
#[tokio::test]
#[ignore]
async fn test_fetch_quote_tokens() {
    let config = load_test_config();
    let client = OpinionClient::new(&config.api, config.environment).expect("Failed to create OpinionClient");

    let tokens = client.get_quote_tokens(56).await.expect("Failed to get quote tokens");
    
    assert!(!tokens.is_empty(), "Should have at least one quote token");
    
    let usdt = tokens.iter().find(|t| t.symbol == "USDT");
    assert!(usdt.is_some(), "Should have USDT token");
    
    let usdt = usdt.unwrap();
    println!("USDT Token Info:");
    println!("  Address: {}", usdt.quote_token_address);
    println!("  Exchange: {}", usdt.ctf_exchange_address);
    println!("  Decimals: {}", usdt.decimal);
}

/// Test: Get user authentication info and proxy address
#[tokio::test]
#[ignore]
async fn test_fetch_user_auth() {
    let config = load_test_config();
    let client = OpinionClient::new(&config.api).expect("Failed to create OpinionClient");

    let auth = client.get_user_auth().await.expect("Failed to get user auth");
    
    println!("User Auth Info:");
    println!("  Wallet Address: {}", auth.wallet_address);
    println!("  Chain Proxies: {:?}", auth.wallet_users);
    
    assert!(!auth.wallet_address.is_empty(), "Wallet address should not be empty");
    
    // Check for BNB Chain (56) proxy
    if let Some(proxy) = auth.wallet_users.get("56") {
        println!("  BSC Proxy: {}", proxy);
        assert!(!proxy.is_empty(), "Proxy address should not be empty");
    }
}

/// Test: Fetch active markets
#[tokio::test]
#[ignore]
async fn test_fetch_active_markets() {
    let config = load_test_config();
    let client = OpinionClient::new(&config.api).expect("Failed to create OpinionClient");

    let markets = client.get_markets_with_params(Some("activated"), Some(5))
        .await
        .expect("Failed to get markets");
    
    println!("Found {} active markets", markets.len());
    
    for market in &markets {
        println!("Market {}:", market.market_id);
        println!("  Title: {}", market.market_title);
        println!("  YES Token: {:?}", market.yes_token_id);
        println!("  NO Token: {:?}", market.no_token_id);
    }
    
    assert!(!markets.is_empty(), "Should have at least one active market");
}

/// Test: Fetch orderbook for a token
#[tokio::test]
#[ignore]
async fn test_fetch_orderbook() {
    let config = load_test_config();
    let client = OpinionClient::new(&config.api).expect("Failed to create OpinionClient");

    // First get an active market
    let markets = client.get_markets_with_params(Some("activated"), Some(1))
        .await
        .expect("Failed to get markets");
    
    if markets.is_empty() {
        println!("No active markets to test orderbook");
        return;
    }

    let market = &markets[0];
    let token_id = market.yes_token_id.clone().unwrap_or_default();
    
    if token_id.is_empty() {
        println!("No token ID for market {}", market.market_id);
        return;
    }

    let orderbook = client.get_orderbook(&token_id)
        .await
        .expect("Failed to get orderbook");
    
    println!("Orderbook for token {}:", token_id);
    println!("  Bids: {} levels", orderbook.bids.len());
    println!("  Asks: {} levels", orderbook.asks.len());
    
    if let Some(best_bid) = orderbook.bids.first() {
        println!("  Best Bid: {} @ {}", best_bid.size, best_bid.price);
    }
    if let Some(best_ask) = orderbook.asks.first() {
        println!("  Best Ask: {} @ {}", best_ask.size, best_ask.price);
    }
}

/// Test: EIP-712 signature generation
#[tokio::test]
#[ignore]
async fn test_eip712_signing() {
    let config = load_test_config();
    let impersonate = OpinionClient::new(&config.api).expect("Failed to create client");
    
    // Get exchange address for verifying contract
    let exchange_addr = impersonate.get_exchange_address(56, "0x55d398326f99059fF775485246999027B3197955")
        .await
        .expect("Failed to get exchange address");
    
    let exchange_address: Address = exchange_addr.parse().expect("Invalid exchange address");
    
    // Create signer
    let mut signer = Eip712Signer::from_secret(&config.chain.private_key)
        .expect("Failed to create signer");
    signer.set_verifying_contract(exchange_address);
    
    // Create test order
    let order = Order::new(
        signer.address(),
        signer.address(),
        U256::from(12345u64),
        U256::from(1000u128 * 1_000_000_000_000_000_000u128), // 1000 USDT
        U256::from(2000u128 * 1_000_000_000_000_000_000u128), // 2000 shares
        OrderSide::Buy,
    )
    .with_expiration(2000000000)
    .with_nonce(0);
    
    // Sign
    let signature = signer.sign_order(&order).expect("Failed to sign order");
    let sig_hex = signature.to_hex();
    
    println!("Order signed successfully:");
    println!("  Signature: {}", sig_hex);
    println!("  Signer: {}", signer.address());
    
    assert!(sig_hex.starts_with("0x"), "Signature should start with 0x");
    assert_eq!(sig_hex.len(), 132, "Signature should be 65 bytes (132 hex chars + 0x)");
}

/// Test: Full order placement flow (uses deep price to avoid fills)
#[tokio::test]
#[ignore]
async fn test_place_limit_order() {
    let config = load_test_config();
    let rest_client = OpinionClient::new(&config.api).expect("Failed to create OpinionClient");
    let impersonate = OpinionClient::new(&config.api).expect("Failed to create OpinionClient");

    // 1. Get active market
    let markets = rest_client.get_markets_with_params(Some("activated"), Some(1))
        .await
        .expect("Failed to get markets");
    
    if markets.is_empty() {
        println!("No active markets available for testing");
        return;
    }

    let market = &markets[0];
    let token_id_str = market.yes_token_id.clone().unwrap_or_default();
    if token_id_str.is_empty() {
        println!("No token ID available");
        return;
    }
    let token_id = U256::from_str_radix(&token_id_str, 10).expect("Invalid token ID");

    // 2. Get exchange address
    let quote_token = "0x55d398326f99059fF775485246999027B3197955";
    let exchange_addr = impersonate.get_exchange_address(56, quote_token)
        .await
        .expect("Failed to get exchange address");
    let exchange_address: Address = exchange_addr.parse().expect("Invalid address");

    // 3. Get proxy address
    let auth = impersonate.get_user_auth().await.expect("Failed to get user auth");
    let proxy_addr = auth.wallet_users.get("56").cloned();
    
    // 4. Setup signer
    let mut signer = Eip712Signer::from_secret(&config.chain.private_key)
        .expect("Failed to create signer");
    signer.set_verifying_contract(exchange_address);
    
    // 5. Determine maker
    let (maker, sig_type) = if let Some(proxy) = proxy_addr {
        let addr: Address = proxy.parse().expect("Invalid proxy address");
        (addr, SignatureType::PolyGnosisSafe)
    } else {
        (signer.address(), SignatureType::Eoa)
    };

    // 6. Create order with VERY low price (0.01) to avoid fills
    // Minimum order value is 1.30 USDT, so use quantity that meets this
    let quantity = U256::from(200u128 * 1_000_000_000_000_000_000u128); // 200 shares
    let price = 0.01; // Very low price -> 200 * 0.01 = 2 USDT (above 1.30 minimum)
    
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
    .with_expiration(2000000000)
    .with_signature_type(sig_type);

    // 7. Sign
    let signature = signer.sign_order(&order).expect("Failed to sign");
    let sig_hex = signature.to_hex();

    // 8. Build request
    let request = serde_json::json!({
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
        "side": "0",
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

    // 9. Submit order
    let response: opinion_hft::data::rest::OpinionResponse<serde_json::Value> = 
        rest_client.post("order", &request).await.expect("Failed to submit order");
    
    println!("Order Response:");
    println!("  code: {}", response.code);
    println!("  msg: {}", response.msg);
    
    if response.code == 0 {
        println!("✅ Order placed successfully!");
        if let Some(result) = &response.result {
            if let Some(order_id) = result.get("orderData").and_then(|d| d.get("orderId")) {
                println!("  Order ID: {}", order_id);
            }
        }
    } else {
        println!("❌ Order failed: {}", response.msg);
    }
    
    assert_eq!(response.code, 0, "Order should succeed");
}

/// Test: Cancel order (requires an existing order)
#[tokio::test]
#[ignore]
async fn test_cancel_order() {
    // This test would require placing an order first, then cancelling it
    // Skipped for now as it requires more complex state management
    println!("Cancel order test - requires order placement first");
}

/// Test: Get user positions
#[tokio::test]
#[ignore]
async fn test_get_positions() {
    let config = load_test_config();
    let rest_client = OpinionClient::new(&config.api).expect("Failed to create OpinionClient");
    
    // Get positions endpoint (if available)
    // Note: This depends on the actual API endpoint structure
    let response: Result<serde_json::Value, _> = rest_client.get("position").await;
    
    match response {
        Ok(positions) => {
            println!("Positions: {:#?}", positions);
        }
        Err(e) => {
            println!("Failed to get positions (may require different endpoint): {}", e);
        }
    }
}
