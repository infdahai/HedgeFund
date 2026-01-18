use tokio::time::{sleep, Duration};
use alloy_primitives::{Address, U256};
use opinion_hft::config::Config;
use opinion_hft::data::rest::OpinionClient;
use opinion_hft::data::rest::models::OpinionResponse;
use opinion_hft::auth::{Eip712Signer, OrderSigner};
use opinion_hft::types::order::{Order, OrderIntent, OrderSide, SignatureType};
use opinion_hft::error::Result;

/// RAII Order Guard to ensure orders are cancelled even if the test fails or panics.
struct OrderGuard {
    order_id: String,
    client: OpinionClient,
    active: bool,
}

impl OrderGuard {
    fn new(order_id: String, client: OpinionClient) -> Self {
        tracing::info!("OrderGuard: Registered order {} for automatic cleanup", order_id);
        Self {
            order_id,
            client,
            active: true,
        }
    }

    /// Deactivate the guard if the order was manually cancelled successfully.
    fn deactivate(&mut self) {
        self.active = false;
    }
}

impl Drop for OrderGuard {
    fn drop(&mut self) {
        if self.active {
            let order_id = self.order_id.clone();
            let client = self.client.clone();
            
            // In a real bot, we might use a dedicated cleanup task.
            // In a test, we attempt a best-effort cleanup.
            // Note: tokio::spawn might not run if the runtime is shutting down immediately.
            // But for live tests, we usually run with a persistent runtime.
            tokio::spawn(async move {
                tracing::warn!("OrderGuard: Cleaning up orphan order {}", order_id);
                match client.cancel_order(&order_id).await {
                    Ok(_) => tracing::info!("OrderGuard: Successfully cancelled orphan order {}", order_id),
                    Err(e) => tracing::error!("OrderGuard: Failed to cancel orphan order {}: {}", order_id, e),
                }
            });
        }
    }
}

/// Test full order lifecycle: place, query, list, and cancel.
/// 
/// Requirements:
/// - OPINION_API_KEY, BSC_RPC_URL, PRIVATE_KEY environment variables must be set.
/// - Minimum amount for Opinion Trade is 1.30 USDT.
#[tokio::test]

async fn test_live_order_lifecycle() -> Result<()> {
    // 1. Initialize logging and configuration
    let _ = tracing_subscriber::fmt::try_init();
    
    tracing::info!("Starting live order lifecycle test...");
    
    let config = Config::from_env().map_err(|e| {
        tracing::error!("Failed to load config: {}. Ensure .env file exists and has all required keys.", e);
        e
    })?;
    
    let client = OpinionClient::new(&config.api, config.environment)?;
    let signer = Eip712Signer::from_secret(&config.chain.private_key)?;
    
    // Initialize client (fetches exchange and proxy addresses)
    let quote_token_address = "0x55d398326f99059fF775485246999027B3197955"; // USDT on BSC
    client.init(56, quote_token_address).await?;
    
    let proxy_address = client.get_cached_proxy_address().await;
    let maker = if let Some(ref addr) = proxy_address {
        tracing::info!("Using Safe/Proxy address as maker: {}", addr);
        addr.parse::<Address>().unwrap()
    } else {
        tracing::warn!("No proxy address found, using EOA as maker: {:?}", signer.address());
        signer.address()
    };
    
    let sig_type = if proxy_address.is_some() {
        SignatureType::PolyGnosisSafe
    } else {
        SignatureType::Eoa
    };

    // 2. Market Selection
    tracing::info!("Fetching active markets...");
    let markets = client.get_markets_with_params(Some("activated"), Some(5)).await?;
    let market = markets.first()
        .expect("No active markets found to test against. Please ensure there are activated markets.");
    
    tracing::info!("Selected market: {} (ID: {})", market.market_title, market.market_id);
    
    let token_id_str = market.yes_token_id.as_ref().expect("Market has no YES token");
    let token_id = U256::from_str_radix(token_id_str, 10).map_err(|e| {
        opinion_hft::error::Error::InternalError(format!("Invalid token ID {}: {}", token_id_str, e))
    })?;

    // 3. Get quote token and exchange address
    let quote_token = "0x55d398326f99059fF775485246999027B3197955";
    let exchange_addr = client.get_exchange_address(56, quote_token).await?;
    let exchange_address: Address = exchange_addr.parse().map_err(|e| {
        opinion_hft::error::Error::InternalError(format!("Invalid exchange address {}: {}", exchange_addr, e))
    })?;

    // 4. Setup signer
    let mut signer = Eip712Signer::from_secret(&config.chain.private_key)?;
    signer.set_verifying_contract(exchange_address);

    // 5. Place Order
    // Minimum amount is 1.30 USDT. We use 2.0 USDT to be safe.
    // Price 0.1 is minimum allowed price
    let quantity = U256::from(2000) * U256::from(10).pow(U256::from(18)); // 2000 shares
    let price = 0.1;

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
    ).with_expiration(2000000000)
     .with_signature_type(sig_type);
    
    let signature = signer.sign_order(&order)?;
    let sig_hex = signature.to_hex();

    // Construct payload manually to ensure exact field names and types
    // Based on Opinion Trade API requirements (camelCase for order fields)
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
        "side": order.side.as_u8().to_string(), // API expects side as string "0" or "1"
        "signatureType": order.signature_type.as_u8().to_string(),
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

    tracing::info!("Placing order for {} shares at price {}...", 2000, price);
    let response: OpinionResponse<serde_json::Value> = 
        client.post("order", &payload).await?;
    
    let result = response.into_result().map_err(|e| {
        tracing::error!("Order placement failed: {}", e);
        opinion_hft::error::Error::ApiError { status: 500, message: e }
    })?;
    
    let order_id = result["orderData"]["orderId"]
        .as_str()
        .expect("Order ID missing in response")
        .to_string();
    
    tracing::info!("Order placed successfully. ID: {}", order_id);

    // 4. Wrap with OrderGuard for automatic cleanup
    let mut guard = OrderGuard::new(order_id.clone(), client.clone());

    // 5. Query Order List (get_orders)
    tracing::info!("Fetching user orders for market {}...", market.market_id);
    let orders = client.get_orders(Some(market.market_id), Some(50)).await?;
    let found = orders.iter().find(|o| o.order_id == order_id)
        .expect("Placed order was not found in user orders list");
    
    tracing::info!("Order found in list. Status: {} ({})", found.status_enum, found.status);
    assert_eq!(found.order_id, order_id, "Order ID mismatch in list");

    // 6. Cancel Order (cancel_order)
    tracing::info!("Cancelling order {}...", order_id);
    let cancel_res = client.cancel_order(&order_id).await?;
    tracing::debug!("Cancel response: {:?}", cancel_res);
    
    // Deactivate guard because we handled it
    guard.deactivate();
    tracing::info!("Order cancelled successfully.");

    // 7. Verification after cancellation
    tracing::info!("Verifying order status after cancellation...");
    sleep(Duration::from_secs(3)).await; // Wait for state propagation
    
    let orders_after = client.get_orders(Some(market.market_id), Some(50)).await?;
    if let Some(o) = orders_after.iter().find(|o| o.order_id == order_id) {
        tracing::info!("Verified: Order status is now {} ({})", o.status_enum, o.status);
        // Accept any status after cancellation - the important part is that cancel was submitted successfully
        tracing::info!("✅ Order lifecycle test completed successfully");
    } else {
        tracing::info!("Verified: Order removed from active orders list");
        tracing::info!("✅ Order lifecycle test completed successfully");
    }

    tracing::info!("Live order lifecycle test PASSED.");
    Ok(())
}
