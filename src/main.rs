use opinion_hft::prelude::*;
// use opinion_hft::config::ConfirmationMode;
use opinion_hft::telemetry::init_telemetry;
use opinion_hft::execution::gateway::OrderGateway;
use opinion_hft::data::rest::OpinionClient;
use opinion_hft::data::websocket::{WebSocketClient, WsEvent};
use opinion_hft::chain::client::ChainClient;
use opinion_hft::auth::Eip712Signer;
use opinion_hft::execution::rate_limiter::RateLimiter;
use opinion_hft::execution::confirmation::ConfirmationTracker;
use opinion_hft::execution::risk_manager::RiskManager;
use opinion_hft::strategy::arbitrage::{ArbitrageDetector, OutcomeOrderbook};
use opinion_hft::strategy::market_scanner::MarketScanner;
use opinion_hft::strategy::orderbook_store::LocalOrderbookStore;
use tracing::{info, error, warn};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use alloy_primitives::Address;

#[derive(Clone)]
struct MarketTokenPairs {
    yes_pair: String,
    no_pair: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Initialize Telemetry (Async Logging)
    let _guards = init_telemetry();
    info!("Starting Opinion HFT Bot...");

    // 2. Load Configuration
    let config = Config::from_env()?;

    info!("Configuration loaded for chain ID {}", config.chain.chain_id);
    if config.execution.scan_only {
        info!("Scan-only mode enabled: order submission is disabled");
    }

    // 3. Initialize Clients
    let maker_address: Address = config.chain.eoa_address.parse().map_err(|e| Error::ConfigError(format!("Invalid EOA: {}", e)))?;

    let chain_client = ChainClient::new(&config.chain, maker_address)?;
    let rest_client = OpinionClient::new(&config.api, config.environment)?;
    let mut ws_client = WebSocketClient::new(&config.websocket, config.api.api_key.clone(), config.environment);
    let mut signer = Eip712Signer::from_secret(&config.chain.private_key)?;

    // Initialize OpinionClient (fetch and cache protected endpoint data once at startup)
    // Uses rrequest with Chrome120 TLS emulation (local=proxy, aws=direct)
    // Caches results for high-frequency trading
    info!("Initializing OpinionClient (fetching protected endpoints)...");
    const QUOTE_TOKEN: &str = "0x55d398326f99059fF775485246999027B3197955";
    rest_client.init(config.chain.chain_id, QUOTE_TOKEN).await?;
    info!("OpinionClient initialized successfully");

    // Set the CTF Exchange address as verifyingContract for EIP-712 signatures
    // This is CRITICAL: the CTF Exchange address is different from ConditionalTokens address
    if let Some(exchange_address_str) = rest_client.get_cached_exchange_address().await {
        let exchange_address: Address = exchange_address_str.parse()
            .map_err(|e| Error::ConfigError(format!("Invalid exchange address: {}", e)))?;
        signer.set_verifying_contract(exchange_address);
        info!("EIP-712 verifyingContract set to CTF Exchange: {}", exchange_address);
    } else {
        return Err(Error::ConfigError("CTF Exchange address not found after initialization".to_string()));
    }

    // Get cached proxy address for Gnosis Safe support
    let proxy_address_opt = rest_client.get_cached_proxy_address().await;
    let proxy_address = if let Some(addr) = proxy_address_opt {
        addr.parse::<Address>().map_err(|e| Error::ConfigError(format!("Invalid proxy address: {}", e)))?
    } else {
        maker_address
    };


    // 4. Initialize WebSocket + Local Orderbook Store
    let orderbook_store = LocalOrderbookStore::new();
    let market_pairs: Arc<RwLock<HashMap<i64, MarketTokenPairs>>> =
        Arc::new(RwLock::new(HashMap::new()));

    ws_client.connect().await?;
    ws_client.subscribe("trade.order.update").await?;
    ws_client.subscribe("trade.record.new").await?;

    let mut ws_market_rx = ws_client.subscribe_events();
    let orderbook_store_rx = orderbook_store.clone();
    let market_pairs_rx = market_pairs.clone();
    tokio::spawn(async move {
        while let Ok(event) = ws_market_rx.recv().await {
            match event {
                WsEvent::MarketData { pair, data } => {
                    if orderbook_store_rx.update_from_ws(&pair, &data).is_none() {
                        tracing::debug!("Ignored market data update for {}", pair);
                    }
                }
                WsEvent::MarketDepthDiff { market_id, outcome, outcome_side, side, price, size } => {
                    let pairs = {
                        let guard = market_pairs_rx.read().await;
                        guard.get(&market_id).cloned()
                    };
                    if let Some(pairs) = pairs {
                        let mut target_pair = match outcome.as_deref() {
                            Some(outcome) if outcome.eq_ignore_ascii_case("yes") => Some((&pairs.yes_pair, "YES")),
                            Some(outcome) if outcome.eq_ignore_ascii_case("no") => Some((&pairs.no_pair, "NO")),
                            Some(outcome) => {
                                tracing::debug!(
                                    "Unknown depth diff outcome for market_id={}: {}",
                                    market_id,
                                    outcome
                                );
                                None
                            }
                            None => None,
                        };

                        if target_pair.is_none() {
                            if let Some(outcome_side) = outcome_side {
                                if outcome_side == 1 {
                                    target_pair = Some((&pairs.yes_pair, "YES"));
                                } else {
                                    target_pair = Some((&pairs.no_pair, "NO"));
                                }
                            } else {
                                tracing::debug!(
                                    "Depth diff missing outcome/outcomeSide for market_id={}",
                                    market_id
                                );
                            }
                        }

                        if let Some((pair, outcome_label)) = target_pair {
                            if orderbook_store_rx.apply_depth_diff(pair, side.as_str(), price, size) {
                                tracing::info!(
                                    "Applied depth diff to {} book for market_id={}",
                                    outcome_label,
                                    market_id
                                );
                            } else {
                                tracing::debug!("Ignored depth diff update for {}", pair);
                            }
                        }
                    } else {
                        tracing::debug!("No market pair mapping for market_id={}", market_id);
                    }
                }
                WsEvent::Disconnected { reason } => {
                    warn!("WebSocket disconnected: {}", reason);
                }
                _ => {}
            }
        }
    });

    let ws_client_for_gateway = ws_client.clone();
    let ws_client_for_scanner = ws_client.clone();

    // 5. Initialize Execution Components
    let rate_limiter = RateLimiter::new(15, 3); // 15 TPS, burst 3
    let (confirmation_tracker, mut confirmation_rx) = ConfirmationTracker::new(
        chain_client.clone(),
        config.confirmation.clone()
    );
    let confirmation_tracker = Arc::new(confirmation_tracker);
    let risk_manager = Arc::new(Mutex::new(RiskManager::new(config.risk.clone(), config.environment, rest_client.clone())));

    // 6. Initialize Gateway
    let mut gateway = OrderGateway::new(
        rest_client.clone(),
        signer,
        rate_limiter.clone(),
        confirmation_tracker.clone(),
        risk_manager.clone(),
        ws_client_for_gateway,
        proxy_address,
        config.execution,
    );

    // Create channel for strategy -> execution
    let (strategy_tx, strategy_rx) = tokio::sync::mpsc::channel(100);

    // 7. Spawn Strategy Loop (Arbitrage Scanner)
    let scanner_client = rest_client.clone();
    let scanner_rate_limiter = rate_limiter.clone();
    let orderbook_store_scan = orderbook_store.clone();
    let market_pairs_scan = market_pairs.clone();
    
    tokio::spawn(async move {
        info!("Arbitrage Scanner started...");
        let mut scanner = MarketScanner::new(scanner_client, config.chain.chain_id);
        let mut detector = ArbitrageDetector::with_defaults();
        struct OutcomeMeta {
            outcome_id: String,
            token_id: String,
            pair: String,
        }

        struct CategoricalScanTarget {
            market_id: String,
            outcomes: Vec<OutcomeMeta>,
        }

        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60)); // Scan every minute
        let mut subscribed_market_ids: HashSet<i64> = HashSet::new();
        let max_orderbook_age = Duration::from_secs(300);
        let subscribe_delay = Duration::from_millis(20);

        loop {
            interval.tick().await;
            let scan_start = Instant::now();
            let mut markets_scanned = 0usize;
            let mut orderbook_errors = 0usize;
            let mut orderbook_missing = 0usize;
            let mut orderbook_stale = 0usize;
            let mut orderbook_rest_refresh = 0usize;
            let mut skipped_missing_tokens = 0usize;
            let mut yesno_found = 0usize;
            let mut mint_sell_found = 0usize;
            let mut neg_risk_found = 0usize;
            let mut desired_market_ids: HashSet<i64> = HashSet::new();

            // Fetch all markets
            scanner_rate_limiter.acquire().await;
            let total_markets = match scanner.refresh_markets_if_stale().await {
                Ok(markets) => markets.len(),
                Err(e) => {
                    error!("Failed to fetch markets: {}", e);
                    continue;
                }
            };

            // Filter for active markets with decent volume (e.g. > 1000 USDT)
            let mut liquid_markets: Vec<_> = scanner
                .filter_liquid_markets(1000.0)
                .into_iter()
                .cloned()
                .collect();
            if liquid_markets.is_empty() {
                warn!("No markets meet volume filter, falling back to active markets with missing volume");
                liquid_markets = scanner
                    .filter_liquid_markets(0.0)
                    .into_iter()
                    .cloned()
                    .collect();
            }
            let liquid_count = liquid_markets.len();
            info!(
                "Scanning {} markets (total cached: {})",
                liquid_count,
                scanner.cached_markets().len()
            );

            let mut categorical_targets = Vec::new();
            let mut binary_targets = Vec::new();

            for market in liquid_markets {
                if market.is_categorical() {
                    categorical_targets.push(market);
                    continue;
                }

                if market.yes_token_id == "0" || market.no_token_id == "0" {
                    skipped_missing_tokens += 1;
                    continue;
                }

                if let Ok(market_id) = market.id.parse::<i64>() {
                    let mut guard = market_pairs_scan.write().await;
                    guard.insert(
                        market_id,
                        MarketTokenPairs {
                            yes_pair: market.yes_pair.clone(),
                            no_pair: market.no_pair.clone(),
                        },
                    );
                    desired_market_ids.insert(market_id);
                } else {
                    warn!("Invalid market id for WS subscribe: {}", market.id);
                }
                binary_targets.push(market);
            }

            let categorical_markets = categorical_targets.len();
            let binary_markets = binary_targets.len();

            // Categorical markets: fetch details and gather child pairs
            let mut categorical_scans = Vec::new();
            for market in categorical_targets {
                let market_id = match market.id.parse::<i64>() {
                    Ok(id) => id,
                    Err(e) => {
                        warn!("Invalid categorical market id {}: {}", market.id, e);
                        continue;
                    }
                };

                scanner_rate_limiter.acquire().await;
                let detail = match scanner.fetch_categorical_market(market_id).await {
                    Ok(data) => data,
                    Err(e) => {
                        warn!("Failed to fetch categorical market {}: {}", market.id, e);
                        orderbook_errors += 1;
                        continue;
                    }
                };

                let mut child_targets = Vec::new();
                for child in detail.child_markets {
                    if let Some(status) = &child.status_enum {
                        if !status.eq_ignore_ascii_case("activated") {
                            continue;
                        }
                    }

                    let token_id = child
                        .yes_token_id
                        .or(child.result_token_id)
                        .filter(|id| !id.is_empty() && id != "0");

                    let token_id = match token_id {
                        Some(id) => id,
                        None => continue,
                    };

                    let pair = token_id.clone();
                    child_targets.push((child.market_id.to_string(), token_id, pair));
                }

                if child_targets.is_empty() {
                    continue;
                }

                let mut outcomes = Vec::new();
                for (outcome_id, token_id, pair) in child_targets {
                    outcomes.push(OutcomeMeta {
                        outcome_id,
                        token_id,
                        pair,
                    });
                }

                if !outcomes.is_empty() {
                    categorical_scans.push(CategoricalScanTarget {
                        market_id: market.id.clone(),
                        outcomes,
                    });
                }
            }

            for market_id in desired_market_ids.difference(&subscribed_market_ids) {
                if let Err(e) = ws_client_for_scanner.subscribe_depth_diff(*market_id).await {
                    warn!("Failed to subscribe market.depth.diff {}: {}", market_id, e);
                }
                tokio::time::sleep(subscribe_delay).await;
            }

            for market_id in subscribed_market_ids.difference(&desired_market_ids) {
                if let Err(e) = ws_client_for_scanner.unsubscribe_depth_diff(*market_id).await {
                    warn!("Failed to unsubscribe market.depth.diff {}: {}", market_id, e);
                }
            }
            subscribed_market_ids = desired_market_ids.clone();

            // Categorical markets: use local orderbooks populated by WS
            for scan in categorical_scans {
                let mut outcomes = Vec::new();
                for outcome in scan.outcomes {
                    let mut orderbook = None;
                    let cached = orderbook_store_scan.get(&outcome.pair);
                    if cached.is_none() {
                        orderbook_missing += 1;
                    }
                    if orderbook_store_scan.is_stale(&outcome.pair, max_orderbook_age) {
                        orderbook_stale += 1;
                    } else if let Some(ob) = cached {
                        orderbook = Some(ob);
                    }

                    let orderbook = if let Some(ob) = orderbook {
                        ob
                    } else {
                        scanner_rate_limiter.acquire().await;
                        match scanner.fetch_orderbook(&outcome.token_id, &outcome.pair).await {
                            Ok(ob) => {
                                orderbook_store_scan.upsert(&outcome.pair, ob.clone());
                                orderbook_rest_refresh += 1;
                                ob
                            }
                            Err(e) => {
                                warn!("REST orderbook refresh failed for {}: {}", outcome.pair, e);
                                orderbook_errors += 1;
                                continue;
                            }
                        }
                    };

                    outcomes.push(OutcomeOrderbook {
                        outcome_id: outcome.outcome_id,
                        token_id: outcome.token_id,
                        pair: outcome.pair,
                        orderbook,
                    });
                }

                if outcomes.is_empty() {
                    continue;
                }

                markets_scanned += 1;
                if let Some(opp) = detector.detect_neg_risk_arbitrage(&scan.market_id, &outcomes) {
                    neg_risk_found += 1;
                    info!("📊 NEG-RISK FOUND: {}", opp.summary());
                    if let Err(e) = strategy_tx.send(opp).await {
                        error!("Failed to send opportunity to gateway: {}", e);
                    }
                }
            }

            // Binary markets: use local orderbooks populated by WS
            for market in binary_targets {
                let mut yes_ob = None;
                let cached_yes = orderbook_store_scan.get(&market.yes_pair);
                if cached_yes.is_none() {
                    orderbook_missing += 1;
                }
                if orderbook_store_scan.is_stale(&market.yes_pair, max_orderbook_age) {
                    orderbook_stale += 1;
                } else if let Some(ob) = cached_yes {
                    yes_ob = Some(ob);
                }
                let yes_ob = if let Some(ob) = yes_ob {
                    ob
                } else {
                    scanner_rate_limiter.acquire().await;
                    match scanner.fetch_orderbook(&market.yes_token_id, &market.yes_pair).await {
                        Ok(ob) => {
                            orderbook_store_scan.upsert(&market.yes_pair, ob.clone());
                            orderbook_rest_refresh += 1;
                            ob
                        }
                        Err(e) => {
                            warn!("REST orderbook refresh failed for {}: {}", market.yes_pair, e);
                            orderbook_errors += 1;
                            continue;
                        }
                    }
                };

                let mut no_ob = None;
                let cached_no = orderbook_store_scan.get(&market.no_pair);
                if cached_no.is_none() {
                    orderbook_missing += 1;
                }
                if orderbook_store_scan.is_stale(&market.no_pair, max_orderbook_age) {
                    orderbook_stale += 1;
                } else if let Some(ob) = cached_no {
                    no_ob = Some(ob);
                }
                let no_ob = if let Some(ob) = no_ob {
                    ob
                } else {
                    scanner_rate_limiter.acquire().await;
                    match scanner.fetch_orderbook(&market.no_token_id, &market.no_pair).await {
                        Ok(ob) => {
                            orderbook_store_scan.upsert(&market.no_pair, ob.clone());
                            orderbook_rest_refresh += 1;
                            ob
                        }
                        Err(e) => {
                            warn!("REST orderbook refresh failed for {}: {}", market.no_pair, e);
                            orderbook_errors += 1;
                            continue;
                        }
                    }
                };

                markets_scanned += 1;

                // Check for Yes/No Arbitrage
                if let Some(opp) = detector.detect_yesno_arbitrage(&market, &yes_ob, &no_ob) {
                    yesno_found += 1;
                    info!("🚀 ARBITRAGE FOUND: {}", opp.summary());
                    
                    if let Err(e) = strategy_tx.send(opp).await {
                        error!("Failed to send opportunity to gateway: {}", e);
                    }
                }

                // Check for Mint-Sell Arbitrage
                if let Some(opp) = detector.detect_mint_sell_arbitrage(&market, &yes_ob, &no_ob) {
                    mint_sell_found += 1;
                    info!("💰 MINT-SELL FOUND: {}", opp.summary());
                    if let Err(e) = strategy_tx.send(opp).await {
                        error!("Failed to send opportunity to gateway: {}", e);
                    }
                }
            }

            let duration_ms = scan_start.elapsed().as_millis();
            let stats = detector.drain_stats();
            info!(
                "Scan summary: total_markets={}, liquid={}, scanned={}, binary={}, categorical={}, skipped_missing_tokens={}, orderbook_errors={}, orderbook_missing={}, orderbook_stale={}, orderbook_rest_refresh={}, yesno_found={}, mint_sell_found={}, neg_risk_found={}, duration_ms={}",
                total_markets,
                liquid_count,
                markets_scanned,
                binary_markets,
                categorical_markets,
                skipped_missing_tokens,
                orderbook_errors,
                orderbook_missing,
                orderbook_stale,
                orderbook_rest_refresh,
                yesno_found,
                mint_sell_found,
                neg_risk_found,
                duration_ms
            );
            info!("Arb decision stats: {}", stats.summary());
        }
    });

    // 8. Spawn Confirmation Logger (Shadow Mode / Analytics)
    tokio::spawn(async move {
        while let Some(confirmed) = confirmation_rx.recv().await {
            info!(
                "ORDER_CONFIRMED id={} side={:?} fill={} latency={:?}",
                confirmed.order_id,
                confirmed.side,
                confirmed.actual_fill,
                confirmed.confirmation_time
            );
        }
    });

    // 9. Run Gateway
    if let Err(e) = gateway.run(Some(strategy_rx)).await {
        error!("Gateway fatal error: {}", e);
    }

    Ok(())
}
