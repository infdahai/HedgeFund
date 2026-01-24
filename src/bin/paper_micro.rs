//! Paper Trading for Microstructure Strategy
//!
//! Usage:
//!   cargo run --bin paper_micro -- --config config/micro.toml
//!   OPINION_MICRO_OBI_THRESHOLD=0.75 cargo run --bin paper_micro

use opinion_hft::{
    config::{Config, Environment},
    data::rest::OpinionClient,
    data::websocket::{WebSocketClient, WsEvent},
    strategy::{
        microstructure::{MicroConfig, MicroStrategy},
        orderbook_store::LocalOrderbookStore,
        MarketScanner,
    },
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();
    info!("Starting Microstructure Paper Trading");

    // Load configuration
    // We reuse the main Config for API keys etc, and MicroConfig for strategy params
    let config = Config::from_env()?;
    let micro_config = MicroConfig::from_env();

    info!(
        "Config: OBI_threshold={:.2}, spread_max={:.1}bps, holding_max={:?}",
        micro_config.enter_obi_threshold,
        micro_config.max_spread_bps,
        micro_config.max_holding_time
    );

    // Initialize components
    // Environment setup
    // Use local proxy if specified or default based on env
    let env = match std::env::var("OPINION_ENV").unwrap_or_default().as_str() {
        "local" => Environment::Local,
        _ => Environment::Aws,
    };

    let api_client = OpinionClient::new(&config.api, env)?;
    // We need chain_id for scanner
    let chain_id = config.chain.chain_id;
    let mut market_scanner = MarketScanner::new(api_client, chain_id);

    let mut ws_client = WebSocketClient::new(&config.websocket, config.api.api_key.clone(), env);
    let orderbook_store = Arc::new(LocalOrderbookStore::new());

    // Initialize strategy in paper mode
    let strategy = Arc::new(RwLock::new(MicroStrategy::new(micro_config, true)));

    // Connect WebSocket
    ws_client.connect().await?;

    // Fetch active markets to subscribe
    info!("Fetching active markets...");
    let markets = market_scanner.fetch_all_markets().await?;
    info!("Found {} markets", markets.len());

    // Populate token map for strategy (needed for live mode, good for consistency)
    let mut token_map = std::collections::HashMap::new();
    for market in &markets {
        token_map.insert(market.yes_pair.clone(), market.yes_token_id.clone());
        token_map.insert(market.no_pair.clone(), market.no_token_id.clone());
    }
    {
        let mut strat = strategy.write().await;
        strat.set_token_map(token_map);
    }

    // Subscribe to top liquid markets or all active ones
    // For paper trading, let's subscribe to a subset to avoid overwhelming if many markets
    // Or just all if count is reasonable.
    // Let's filter for "activated" and maybe some volume
    let active_markets: Vec<_> = markets.iter().filter(|m| m.is_active).collect();

    info!("Subscribing to {} active markets", active_markets.len());

    for market in active_markets {
        if let Ok(market_id) = market.id.parse::<i64>() {
            if let Err(e) = ws_client.subscribe_depth_diff(market_id).await {
                error!("Failed to subscribe to market {}: {}", market_id, e);
            }
        }
    }

    // Main event loop
    let mut ws_rx = ws_client.subscribe_events();

    info!("Entering main loop...");

    loop {
        tokio::select! {
            Ok(event) = ws_rx.recv() => {
                match event {
                    WsEvent::MarketDepthDiff {
                        market_id,
                        side,
                        price,
                        size,
                        outcome,
                        outcome_side,
                        ..
                    } => {
                        // Determine outcome string from either `outcome` or `outcome_side`
                        // outcome is Option<String> like "YES" or "NO"
                        // outcome_side is Option<i64> where 0 = YES, 1 = NO
                        let outcome_str = match (outcome, outcome_side) {
                            (Some(s), _) => s,
                            (None, Some(0)) => "YES".to_string(),
                            (None, Some(1)) => "NO".to_string(),
                            _ => {
                                // Neither outcome nor outcome_side available, skip
                                continue;
                            }
                        };

                        let pair = format!("{}-{}", market_id, outcome_str);

                        // Update local orderbook
                        // Using side string from event: "bids" or "asks"
                        if orderbook_store.apply_depth_diff(&pair, &side, price, size) {
                            // If update applied, run strategy
                            if let Some(ob) = orderbook_store.get(&pair) {
                                let mut strat = strategy.write().await;
                                // Pass None for gateway in paper mode
                                if let Err(e) = strat
                                    .on_orderbook_update(&market_id.to_string(), &pair, &ob, None)
                                    .await
                                {
                                    warn!("Strategy execution error: {}", e);
                                }
                            }
                        }
                    }
                    WsEvent::Disconnected { reason } => {
                        warn!("WebSocket disconnected: {}", reason);
                        // Reconnection logic is handled by client internal loop usually
                    }
                    _ => {} // Ignore other events
                }
            }

            _ = tokio::signal::ctrl_c() => {
                info!("Shutdown signal received");
                break;
            }
        }
    }

    // Print final stats
    let strat = strategy.read().await;
    info!("=== Paper Trading Session Ended ===");
    info!("Active Positions: {}", strat.positions.len());
    // We could calculate total PnL here if we tracked it in strategy struct
    // Current strategy struct tracks positions and logic, maybe add a total_pnl field later.

    Ok(())
}
