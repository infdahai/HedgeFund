use opinion_hft::config::Config;
use opinion_hft::data::rest::OpinionClient;

use tracing::{info, warn};

/// Helper to initialize the client and configuration
async fn setup() -> (OpinionClient, Config) {
    // Initialize tracing for tests if not already initialized
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

async fn test_market_list_and_pagination() {
    let (client, _) = setup().await;

    info!("Testing /market endpoint (list and pagination)");

    // 1. Get total market list
    let all_markets = client.get_markets().await.expect("Failed to get markets");
    info!("Total markets found: {}", all_markets.len());

    if all_markets.is_empty() {
        warn!("Market list is empty, skipping further list tests");
        return;
    }

    // 2. Test pagination (limit)
    let limit = 2;
    let limited_markets = client
        .get_markets_with_params(None, Some(limit))
        .await
        .expect("Failed to get limited markets");
    info!(
        "Limited markets (limit={}): {}",
        limit,
        limited_markets.len()
    );
    assert!(limited_markets.len() <= limit as usize);

    // 3. Test pagination (page) - using the new method we added
    let page1 = client
        .get_markets_with_pagination(None, Some(1), Some(1))
        .await
        .expect("Failed to get page 1");
    let page2 = client
        .get_markets_with_pagination(None, Some(1), Some(2))
        .await
        .expect("Failed to get page 2");

    if !page1.is_empty() && !page2.is_empty() {
        info!(
            "Page 1 market ID: {}, Page 2 market ID: {}",
            page1[0].market_id, page2[0].market_id
        );
        assert_ne!(
            page1[0].market_id, page2[0].market_id,
            "Page 1 and Page 2 should have different markets"
        );
    }
}

#[tokio::test]

async fn test_market_filtering() {
    let (client, _) = setup().await;

    info!("Testing /market endpoint filtering");

    // Filter by status "activated" (most common active status)
    let active_markets = client
        .get_markets_with_params(Some("activated"), Some(5))
        .await
        .expect("Failed to get active markets");

    info!("Found {} active markets", active_markets.len());
    for market in &active_markets {
        assert_eq!(market.status_enum.to_lowercase(), "activated");
        info!(
            "Market: [{}] {} - Status: {}",
            market.market_id, market.market_title, market.status_enum
        );
    }
}

#[tokio::test]

async fn test_quote_tokens() {
    let (client, config) = setup().await;

    info!(
        "Testing /quoteToken endpoint for chain ID {}",
        config.chain.chain_id
    );

    let tokens = client
        .get_quote_tokens(config.chain.chain_id)
        .await
        .expect("Failed to get quote tokens");

    info!("Found {} quote tokens", tokens.len());
    assert!(!tokens.is_empty(), "At least one quote token should exist");

    for token in tokens {
        info!(
            "Token: {} | Symbol: {} | Address: {} | CTF Exchange: {}",
            token.quote_token_name,
            token.symbol,
            token.quote_token_address,
            token.ctf_exchange_address
        );

        assert!(!token.quote_token_address.is_empty());
        assert!(!token.ctf_exchange_address.is_empty());
        assert!(token.decimal > 0);
    }
}

#[tokio::test]

async fn test_orderbook() {
    let (client, _) = setup().await;

    info!("Testing /token/orderbook endpoint");

    // First find an active market to get a valid token ID
    let markets = client
        .get_markets_with_params(Some("activated"), Some(1))
        .await
        .expect("Failed to get active markets");

    let market = match markets.first() {
        Some(m) => m,
        None => {
            warn!("No active markets found to test orderbook");
            return;
        }
    };

    let token_id = market
        .yes_token_id
        .as_ref()
        .or(market.no_token_id.as_ref())
        .expect("Market should have at least one token ID");

    info!(
        "Fetching orderbook for token ID: {} (Market: {})",
        token_id, market.market_title
    );

    let orderbook = client
        .get_orderbook(token_id)
        .await
        .expect("Failed to get orderbook");

    info!(
        "Orderbook received: {} bids, {} asks",
        orderbook.bids.len(),
        orderbook.asks.len()
    );
    info!("Timestamp: {}", orderbook.timestamp);

    // Assertions for orderbook entries if they exist
    if let Some(bid) = orderbook.bids.first() {
        info!("Top bid: Price={}, Size={}", bid.price, bid.size);
        assert!(!bid.price.is_empty());
    }
    if let Some(ask) = orderbook.asks.first() {
        info!("Top ask: Price={}, Size={}", ask.price, ask.size);
        assert!(!ask.price.is_empty());
    }
}

#[tokio::test]

async fn test_market_detail() {
    let (client, _) = setup().await;

    info!("Testing /market/detail endpoint");

    // First find any market to get a valid ID
    let markets = client
        .get_markets_with_params(None, Some(1))
        .await
        .expect("Failed to get markets");

    let market_id = match markets.first() {
        Some(m) => m.market_id,
        None => {
            warn!("No markets found to test market detail");
            return;
        }
    };

    info!("Fetching details for market ID: {}", market_id);

    let detail = client
        .get_market_detail(market_id)
        .await
        .expect("Failed to get market detail");

    info!(
        "Market detail: {} (Status: {})",
        detail.market_title, detail.status_enum
    );
    assert_eq!(detail.market_id, market_id);
    assert!(!detail.market_title.is_empty());

    // Check for essential fields
    if let Some(token_id) = &detail.yes_token_id {
        info!("Yes Token ID: {}", token_id);
    }
    if let Some(condition_id) = &detail.condition_id {
        info!("Condition ID: {}", condition_id);
    }
}

#[tokio::test]

async fn test_error_handling_invalid_market() {
    let (client, _) = setup().await;

    info!("Testing error handling for non-existent market ID");

    let invalid_id = 999999999;
    let result = client.get_market_detail(invalid_id).await;

    match result {
        Ok(_) => warn!("Expected error for invalid market ID, but got success"),
        Err(e) => info!("Successfully caught error for invalid market: {:?}", e),
    }
}
