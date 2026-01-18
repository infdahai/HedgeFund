use opinion_hft::prelude::*;
// use opinion_hft::config::ConfirmationMode;
use opinion_hft::telemetry::init_telemetry;
use opinion_hft::execution::gateway::OrderGateway;
use opinion_hft::data::rest::OpinionClient;
use opinion_hft::data::websocket::WebSocketClient;
use opinion_hft::chain::client::ChainClient;
use opinion_hft::auth::Eip712Signer;
use opinion_hft::execution::rate_limiter::RateLimiter;
use opinion_hft::execution::confirmation::ConfirmationTracker;
use opinion_hft::execution::risk_manager::RiskManager;
use opinion_hft::strategy::market_scanner::MarketScanner;
use opinion_hft::strategy::arbitrage::ArbitrageDetector;
use tracing::{info, error, warn};
use std::sync::Arc;
use tokio::sync::Mutex;
use alloy_primitives::Address;

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Initialize Telemetry (Async Logging)
    let _guards = init_telemetry();
    info!("Starting Opinion HFT Bot...");

    // 2. Load Configuration
    let config = Config::from_env()?;

    info!("Configuration loaded for chain ID {}", config.chain.chain_id);

    // 3. Initialize Clients
    let maker_address: Address = config.chain.eoa_address.parse().map_err(|e| Error::ConfigError(format!("Invalid EOA: {}", e)))?;

    let chain_client = ChainClient::new(&config.chain, maker_address)?;
    let rest_client = OpinionClient::new(&config.api, config.environment)?;
    let ws_client = WebSocketClient::new(&config.websocket, config.api.api_key.clone(), config.environment);
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


    // 4. Initialize Execution Components
    let rate_limiter = RateLimiter::new(15, 3); // 15 TPS, burst 3
    let (confirmation_tracker, mut confirmation_rx) = ConfirmationTracker::new(
        chain_client.clone(),
        config.confirmation.clone()
    );
    let confirmation_tracker = Arc::new(confirmation_tracker);
    let risk_manager = Arc::new(Mutex::new(RiskManager::new(config.risk.clone(), config.environment, rest_client.clone())));

    // 5. Initialize Gateway
    let mut gateway = OrderGateway::new(
        rest_client.clone(),
        signer,
        rate_limiter.clone(),
        confirmation_tracker.clone(),
        risk_manager.clone(),
        ws_client,
        proxy_address,
        config.execution,
    );

    // Create channel for strategy -> execution
    let (strategy_tx, strategy_rx) = tokio::sync::mpsc::channel(100);

    // 6. Spawn Strategy Loop (Arbitrage Scanner)
    let scanner_client = rest_client.clone();
    let scanner_rate_limiter = rate_limiter.clone();
    
    tokio::spawn(async move {
        info!("Arbitrage Scanner started...");
        let mut scanner = MarketScanner::new(scanner_client);
        let mut detector = ArbitrageDetector::with_defaults();
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60)); // Scan every minute

        loop {
            interval.tick().await;
            
            // Fetch all markets
            match scanner.fetch_all_markets().await {
                Ok(markets) => {
                    info!("Fetched {} markets", markets.len());
                    
                    // Filter for active markets with decent volume (e.g. > 1000 USDT)
                    let liquid_markets = scanner.filter_liquid_markets(1000.0);
                    info!("Scanning {} liquid markets...", liquid_markets.len());

                    for market in liquid_markets {
                        // Rate limit check
                        scanner_rate_limiter.acquire().await;
                        
                        // Fetch orderbooks
                        let yes_ob = match scanner.fetch_orderbook(&market.yes_token_id, &market.yes_pair).await {
                            Ok(ob) => ob,
                            Err(e) => {
                                warn!("Failed to fetch OB for {}: {}", market.yes_pair, e);
                                continue;
                            }
                        };

                        scanner_rate_limiter.acquire().await;
                        let no_ob = match scanner.fetch_orderbook(&market.no_token_id, &market.no_pair).await {
                            Ok(ob) => ob,
                            Err(e) => {
                                warn!("Failed to fetch OB for {}: {}", market.no_pair, e);
                                continue;
                            }
                        };

                        // Check for Yes/No Arbitrage
                        if let Some(opp) = detector.detect_yesno_arbitrage(market, &yes_ob, &no_ob) {
                            info!("🚀 ARBITRAGE FOUND: {}", opp.summary());
                            
                            if let Err(e) = strategy_tx.send(opp).await {
                                error!("Failed to send opportunity to gateway: {}", e);
                            }
                        }

                        // Check for Mint-Sell Arbitrage
                        if let Some(opp) = detector.detect_mint_sell_arbitrage(market, &yes_ob, &no_ob) {
                            info!("💰 MINT-SELL FOUND: {}", opp.summary());
                            if let Err(e) = strategy_tx.send(opp).await {
                                error!("Failed to send opportunity to gateway: {}", e);
                            }
                        }
                    }
                }
                Err(e) => error!("Failed to fetch markets: {}", e),
            }
        }
    });

    // 7. Spawn Confirmation Logger (Shadow Mode / Analytics)
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

    // 8. Run Gateway
    if let Err(e) = gateway.run(Some(strategy_rx)).await {
        error!("Gateway fatal error: {}", e);
    }

    Ok(())
}
