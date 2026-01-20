//! Small-scale arbitrage detection tests
//!
//! Tests the arbitrage detector with real market data to verify:
//! - YesNo arbitrage detection (buy both sides when sum < 1.0)
//! - MintSell arbitrage detection (mint and sell both when sum > 1.0)
//! - Configuration and fee calculation

use opinion_hft::config::Config;
use opinion_hft::data::rest::OpinionClient;
use opinion_hft::strategy::arbitrage::{ArbitrageDetector, ArbitrageConfig, ArbitrageType};
use opinion_hft::strategy::market_scanner::{MarketInfo, MarketOrderbook, OrderbookSide};
use tracing::info;

/// Helper to initialize logging
fn init_logs() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    static mut GUARDS: Option<opinion_hft::telemetry::TelemetryGuards> = None;

    unsafe {
        INIT.call_once(|| {
            let guards = opinion_hft::telemetry::init_telemetry();
            GUARDS = Some(guards);
        });
    }
}

fn now_unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Helper to initialize the client and configuration
async fn setup() -> (OpinionClient, Config) {
    init_logs();

    let config = Config::from_env().expect("Failed to load configuration from environment");
    
    info!("Initializing OpinionClient in {:?} environment", config.environment);
    let client = OpinionClient::new(&config.api, config.environment)
        .expect("Failed to create OpinionClient");
    
    (client, config)
}

/// Fetch a market with its orderbook data
async fn fetch_market_with_orderbook(
    client: &OpinionClient,
) -> Option<(MarketInfo, MarketOrderbook, MarketOrderbook)> {
    // Get active markets
    let markets = client.get_markets().await.ok()?;
    
    // Find first active market with valid token IDs
    for market in markets {
        if market.status != 2 {
            continue; // Skip non-active markets
        }
        
        let yes_token_id = match &market.yes_token_id {
            Some(id) if id != "0" => id,
            _ => continue,
        };
        
        let no_token_id = match &market.no_token_id {
            Some(id) if id != "0" => id,
            _ => continue,
        };
        
        // Fetch orderbooks
        let yes_ob = client.get_orderbook(yes_token_id).await.ok()?;
        let no_ob = client.get_orderbook(no_token_id).await.ok()?;
        
        // Convert to internal format
        let convert_orderbook = |pair: &str, ob: opinion_hft::data::rest::models::TokenOrderbook| -> MarketOrderbook {
            let convert_side = |entries: Vec<opinion_hft::data::rest::models::OrderbookEntry>| -> Vec<OrderbookSide> {
                entries.into_iter()
                    .filter_map(|e| {
                        Some(OrderbookSide {
                            price: e.price.parse().ok()?,
                            size: e.size.parse().ok()?,
                        })
                    })
                    .collect()
            };

            MarketOrderbook {
                pair: pair.to_string(),
                bids: convert_side(ob.bids),
                asks: convert_side(ob.asks),
            }
        };
        
        let market_id = market.market_id.to_string();
        let yes_pair = format!("{}-YES", market_id);
        let no_pair = format!("{}-NO", market_id);
        
        let market_info = MarketInfo {
            id: market_id.clone(),
            question: market.market_title.clone(),
            yes_token_id: yes_token_id.clone(),
            no_token_id: no_token_id.clone(),
            yes_pair: yes_pair.clone(),
            no_pair: no_pair.clone(),
            volume_24h: market.volume24h.as_ref().and_then(|v| v.parse().ok()).unwrap_or(0.0),
            liquidity: 0.0,
            is_active: true,
            market_type: market.market_type,
            root_market_id: None,
            parent_title: None,
        };
        
        let yes_orderbook = convert_orderbook(&yes_pair, yes_ob);
        let no_orderbook = convert_orderbook(&no_pair, no_ob);
        
        // Check if orderbooks have data
        if yes_orderbook.best_ask().is_some() && no_orderbook.best_ask().is_some() {
            info!(
                "Selected market: {} - {}",
                market_info.id,
                market_info.question
            );
            return Some((market_info, yes_orderbook, no_orderbook));
        }
    }
    
    None
}

#[tokio::test]
async fn test_arbitrage_detector_yesno() {
    let (client, _) = setup().await;
    
    info!("Testing YesNo arbitrage detection with real market data");
    
    // Fetch a real market
    let (market, yes_ob, no_ob) = match fetch_market_with_orderbook(&client).await {
        Some(data) => data,
        None => {
            info!("No suitable market found for testing, skipping");
            return;
        }
    };
    
    // Log orderbook state
    info!(
        "YES orderbook - Best Ask: {:?}, Best Bid: {:?}",
        yes_ob.best_ask(),
        yes_ob.best_bid()
    );
    info!(
        "NO orderbook - Best Ask: {:?}, Best Bid: {:?}",
        no_ob.best_ask(),
        no_ob.best_bid()
    );
    
    // Create detector with default config
    let mut detector = ArbitrageDetector::with_defaults();
    
    // Detect arbitrage
    let detected_at = now_unix_seconds();
    let opportunity = detector.detect_yesno_arbitrage(
        &market,
        &yes_ob.bids,
        &yes_ob.asks,
        &no_ob.bids,
        &no_ob.asks,
        detected_at,
    );
    
    match opportunity {
        Some(opp) => {
            info!("✓ YesNo arbitrage opportunity detected!");
            info!("  Type: {:?}", opp.arb_type);
            info!("  Market: {}", opp.market_id);
            info!("  Total Cost: {:.4}", opp.total_cost);
            info!("  Gross Profit Rate: {:.2}%", opp.gross_profit_rate * 100.0);
            info!("  Net Profit Rate: {:.2}%", opp.net_profit_rate * 100.0);
            info!("  Max Size: {:.2}", opp.max_size);
            info!("  Legs: {}", opp.legs.len());
            
            // Verify basic properties
            assert_eq!(opp.arb_type, ArbitrageType::YesNo);
            assert!(opp.total_cost < 1.0, "Total cost should be < 1.0 for YesNo arb");
            assert!(opp.net_profit_rate > 0.0, "Net profit should be positive");
            assert_eq!(opp.legs.len(), 2, "YesNo arb should have 2 legs");
        }
        None => {
            info!("✗ No YesNo arbitrage opportunity found (expected for most markets)");
            let yes_ask = yes_ob.best_ask().unwrap_or(0.0);
            let no_ask = no_ob.best_ask().unwrap_or(0.0);
            info!("  Sum of asks: {:.4} (needs to be < 1.0)", yes_ask + no_ask);
        }
    }
}

#[tokio::test]
async fn test_arbitrage_detector_mint_sell() {
    let (client, _) = setup().await;
    
    info!("Testing MintSell arbitrage detection with real market data");
    
    // Fetch a real market
    let (market, yes_ob, no_ob) = match fetch_market_with_orderbook(&client).await {
        Some(data) => data,
        None => {
            info!("No suitable market found for testing, skipping");
            return;
        }
    };
    
    // Log orderbook state
    info!(
        "YES orderbook - Best Bid: {:?}",
        yes_ob.best_bid()
    );
    info!(
        "NO orderbook - Best Bid: {:?}",
        no_ob.best_bid()
    );
    
    // Create detector with default config
    let mut detector = ArbitrageDetector::with_defaults();
    
    // Detect arbitrage
    let detected_at = now_unix_seconds();
    let opportunity = detector.detect_mint_sell_arbitrage(
        &market,
        &yes_ob.bids,
        &yes_ob.asks,
        &no_ob.bids,
        &no_ob.asks,
        detected_at,
    );
    
    match opportunity {
        Some(opp) => {
            info!("✓ MintSell arbitrage opportunity detected!");
            info!("  Type: {:?}", opp.arb_type);
            info!("  Market: {}", opp.market_id);
            info!("  Total Cost: {:.4} (mint cost)", opp.total_cost);
            info!("  Gross Profit Rate: {:.2}%", opp.gross_profit_rate * 100.0);
            info!("  Net Profit Rate: {:.2}%", opp.net_profit_rate * 100.0);
            info!("  Max Size: {:.2}", opp.max_size);
            info!("  Legs: {}", opp.legs.len());
            
            // Verify basic properties
            assert_eq!(opp.arb_type, ArbitrageType::MintSell);
            assert_eq!(opp.total_cost, 1.0, "Mint cost is always 1.0");
            assert!(opp.gross_profit_rate > 0.0, "Gross profit should be positive");
            assert_eq!(opp.legs.len(), 2, "MintSell arb should have 2 legs (sell YES + sell NO)");
            
            // Verify legs are SELL orders
            for leg in &opp.legs {
                assert_eq!(
                    leg.side,
                    opinion_hft::strategy::arbitrage::TradeSide::Sell,
                    "MintSell legs should be Sell orders"
                );
            }
        }
        None => {
            info!("✗ No MintSell arbitrage opportunity found (expected for most markets)");
            let yes_bid = yes_ob.best_bid().unwrap_or(0.0);
            let no_bid = no_ob.best_bid().unwrap_or(0.0);
            info!("  Sum of bids: {:.4} (needs to be > 1.0)", yes_bid + no_bid);
        }
    }
}

#[tokio::test]
async fn test_arbitrage_configuration() {
    init_logs();
    
    info!("Testing ArbitrageConfig fee calculation");
    
    // Create custom config
    let config = ArbitrageConfig {
        min_profit_rate: 0.01,      // 1% minimum
        min_liquidity: 50.0,         // Lower threshold for testing
        min_order_notional: 1.30,    // Keep consistent with defaults
        min_fee: 0.50,               // Keep consistent with defaults
        topic_fee_rate: 0.02,        // 2% base fee
        referral_discount: 0.10,     // 10% rebate
        max_slippage: 0.02,          // 2% slippage
    };
    
    // Test fee calculation at different price points
    let test_prices = vec![0.1, 0.3, 0.5, 0.7, 0.9];
    
    info!("Fee rates at different prices (with 10% referral discount):");
    for price in test_prices {
        let fee_rate = config.calculate_fee_rate(price);
        info!(
            "  Price: {:.1} → Fee Rate: {:.4} ({:.2}%)",
            price,
            fee_rate,
            fee_rate * 100.0
        );
        
        // Verify fee curve properties
        assert!(fee_rate >= 0.0, "Fee rate should be non-negative");
        
        // Fee peaks at p=0.5
        if (price - 0.5).abs() < 0.01 {
            // At p=0.5: base_fee = 0.02 * 0.5 * 0.5 = 0.005
            // After 10% discount: 0.005 * 0.9 = 0.0045
            let expected = 0.0045;
            assert!(
                (fee_rate - expected).abs() < 0.0001,
                "Fee at p=0.5 should be ~0.45%"
            );
        }
    }
    
    // Verify symmetric property: fee(p) == fee(1-p)
    let p = 0.3;
    let fee_at_p = config.calculate_fee_rate(p);
    let fee_at_1_minus_p = config.calculate_fee_rate(1.0 - p);
    assert!(
        (fee_at_p - fee_at_1_minus_p).abs() < 0.0001,
        "Fee curve should be symmetric"
    );
    info!(
        "✓ Fee symmetry verified: fee({:.1}) = fee({:.1}) = {:.4}",
        p,
        1.0 - p,
        fee_at_p
    );
}
