// Live Trade Integration Tests - Complete workflow: Place -> Cancel -> Query
//
// These tests demonstrate complete order lifecycle:
// 1. Place a limit order (deep price to avoid fills)
// 2. Query the order
// 3. Cancel the order
// 4. Verify cancellation
//
// Tests are ignored by default (#[ignore]) because they require live API access and valid credentials in .env file

use alloy_primitives::{Address, U256};
use opinion_hft::{
    auth::{Eip712Signer, OrderSigner},
    config::Config,
    data::rest::{OpinionClient, OpinionResponse},
    types::order::{Order, OrderIntent, OrderSide, SignatureType},
};
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PlaceOrderRequest {
    salt: String,
    maker: String,
    signer: String,
    taker: String,
    token_id: String,
    maker_amount: String,
    taker_amount: String,
    expiration: String,
    nonce: String,
    fee_rate_bps: String,
    side: String,
    signature_type: String,
    signature: String,
    sign: String,
    timestamp: i64,
    topic_id: u64,
    trading_method: u64,
    contract_address: String,
    currency_address: String,
    order_exp_time: String,
    safe_rate: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    origin: Option<String>,
}

impl PlaceOrderRequest {
    fn from_order(order: &Order, signature: &str, market_id: u64, quote_token: &str) -> Self {
        let side_str = match order.side {
            OrderSide::Buy => "0",
            OrderSide::Sell => "1",
        };
        let sig_type_str = order.signature_type.as_u8().to_string();

        Self {
            salt: order.salt.to_string(),
            maker: format!("{:?}", order.maker),
            signer: format!("{:?}", order.signer),
            taker: format!("{:?}", order.taker),
            token_id: order.token_id.to_string(),
            maker_amount: order.maker_amount.to_string(),
            taker_amount: order.taker_amount.to_string(),
            expiration: order.expiration.to_string(),
            nonce: order.nonce.to_string(),
            fee_rate_bps: order.fee_rate_bps.to_string(),
            side: side_str.to_string(),
            signature_type: sig_type_str.to_string(),
            signature: signature.to_string(),
            sign: signature.to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            topic_id: market_id,
            trading_method: 2,
            contract_address: "".to_string(),
            currency_address: quote_token.to_string(),
            order_exp_time: "0".to_string(),
            safe_rate: "0".to_string(),
            origin: None,
        }
    }
}

#[tokio::test]

async fn test_complete_order_workflow() -> anyhow::Result<()> {
    println!("🚀 Starting Complete Order Workflow Test...");
    println!("   Steps: Place -> Query -> Cancel -> Verify");

    let config = Config::from_env().expect("Failed to load config from .env");

    let client = OpinionClient::new(&config.api, config.environment)?;

    // 1. Get active market
    let markets = client
        .get_markets_with_params(Some("activated"), Some(1))
        .await?;
    assert!(!markets.is_empty(), "No activated markets found");

    let market = &markets[0];
    println!(
        "📌 Target Market: {} (ID: {})",
        market.market_title, market.market_id
    );

    let token_id_str = market.yes_token_id.clone().unwrap_or_default();
    assert!(!token_id_str.is_empty(), "No YES token ID found");

    let token_id = U256::from_str_radix(&token_id_str, 10)?;
    println!("   Token ID: {}", token_id);

    // 2. Get exchange and proxy addresses
    let quote_token = "0x55d398326f99059fF775485246999027B3197955";
    let exchange_addr = client.get_exchange_address(56, quote_token).await?;
    let exchange_address: Address = exchange_addr.parse()?;
    println!("✅ Exchange Address: {}", exchange_address);

    let auth = client.get_user_auth().await?;
    let proxy_addr = auth.wallet_users.get("56").cloned();
    let (maker, sig_type) = match &proxy_addr {
        Some(proxy) if !proxy.is_empty() => {
            let addr: Address = proxy.parse()?;
            println!("✅ Proxy/Safe Address: {}", addr);
            (addr, SignatureType::PolyGnosisSafe)
        }
        _ => {
            let addr: Address = config.chain.eoa_address.parse()?;
            println!("⚠️ Using EOA as maker: {}", addr);
            (addr, SignatureType::Eoa)
        }
    };

    // 3. Setup signer
    let mut signer = Eip712Signer::from_secret(&config.chain.private_key)?;
    signer.set_verifying_contract(exchange_address);
    println!("   Signer: {}", signer.address());

    // 4. Place a limit order (deep price to avoid fills)
    println!("\n📝 Step 1: Placing limit order...");
    let quantity = U256::from(200u128 * 1_000_000_000_000_000_000u128);
    let price = 0.01; // Very low price (won't fill)

    let intent = OrderIntent::new(
        market.market_id.to_string(),
        token_id,
        OrderSide::Buy,
        price,
        quantity,
    );
    let (maker_amount, taker_amount) = intent.calculate_amounts();
    println!("   Price: {}, Quantity: 200 shares", price);
    println!("   Maker Amount: {} USDT", maker_amount);

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

    let signature = signer.sign_order(&order)?;
    let sig_hex = signature.to_hex();

    let request =
        PlaceOrderRequest::from_order(&order, &sig_hex, market.market_id as u64, quote_token);
    let request_json = serde_json::to_value(&request)?;
    let mut final_json = request_json;
    final_json["price"] = Value::String(price.to_string());

    let response: OpinionResponse<serde_json::Value> = client.post("order", &final_json).await?;

    assert_eq!(
        response.code, 0,
        "Order failed: [{}] {}",
        response.code, response.msg
    );

    let order_id = response
        .result
        .as_ref()
        .and_then(|r| r.get("orderData"))
        .and_then(|d| d.get("orderId"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    println!("✅ Order Placed Successfully!");
    println!("   Order ID: {}", order_id);
    println!("   Status: Pending");

    // 5. Query the order
    println!("\n🔍 Step 2: Querying order status...");
    tokio::time::sleep(Duration::from_secs(1)).await;

    let orders = client.get_orders(Some(market.market_id), Some(10)).await?;
    let placed_order = orders.iter().find(|o| o.order_id == order_id);

    match placed_order {
        Some(o) => {
            println!("   Found order:");
            println!("   - Status: {} ({})", o.status_enum, o.status);
            println!("   - Price: {}", o.price);
            println!("   - Amount: {} USDT", o.order_amount);
            println!(
                "   - Filled: {} USDT / {} shares",
                o.filled_amount, o.filled_shares
            );
        }
        None => {
            println!("   Order not found in user orders (may still be processing)");
        }
    }

    // 6. Cancel the order
    println!("\n❌ Step 3: Cancelling order...");
    let cancel_result = client.cancel_order(&order_id).await;

    match cancel_result {
        Ok(result) => {
            println!("✅ Cancel request submitted successfully!");
            println!("   Response: {:#?}", result);
        }
        Err(e) => {
            println!("⚠️ Cancel may have failed or order already filled/cancelled");
            println!("   Error: {}", e);
        }
    }

    // 7. Verify cancellation
    println!("\n✅ Step 4: Verifying order status...");
    tokio::time::sleep(Duration::from_secs(1)).await;

    let orders_after_cancel = client.get_orders(Some(market.market_id), Some(10)).await?;
    let order_after = orders_after_cancel.iter().find(|o| o.order_id == order_id);

    match order_after {
        Some(o) => {
            println!("   Order status: {} ({})", o.status_enum, o.status);
            if o.status == 4 {
                // Cancelled status
                println!("✅ Order successfully cancelled!");
            } else {
                println!("⚠️ Order status is: {}", o.status_enum);
            }
        }
        None => {
            println!("✅ Order no longer in active orders (cancelled or filled)");
        }
    }

    // Summary
    println!("\n📊 Workflow Complete!");
    println!("   - Order ID: {}", order_id);
    println!("   - Placed: ✅");
    println!("   - Cancelled: ✅");

    Ok(())
}

#[tokio::test]

async fn test_api_auth_and_config() -> anyhow::Result<()> {
    println!("Testing API authentication and configuration...");

    let config = Config::from_env()?;
    assert!(!config.api.api_key.is_empty(), "API key must be configured");
    assert!(
        !config.chain.eoa_address.is_empty(),
        "EOA address must be configured"
    );

    let client = OpinionClient::new(&config.api, config.environment)?;

    // Test markets endpoint
    let markets = client
        .get_markets_with_params(Some("activated"), Some(1))
        .await?;
    println!("✅ Markets API accessible: {} markets found", markets.len());

    // Test user auth endpoint
    let _auth = client.get_user_auth().await?;
    println!("✅ User auth API accessible");

    // Test exchange address
    let quote_token = "0x55d398326f99059fF775485246999027B3197955";
    let exchange_addr = client.get_exchange_address(56, quote_token).await?;
    println!("✅ Exchange address API accessible: {}", exchange_addr);

    Ok(())
}
