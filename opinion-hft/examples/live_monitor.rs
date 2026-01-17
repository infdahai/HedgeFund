//! Live environment monitor and verification tool.
//! 
//! This example connects to the real Opinion Trade environment using credentials
//! from `.env`, and performs continuous health checks and data monitoring.
//!
//! Usage:
//! `cargo run --example live_monitor`

use opinion_hft::{
    config::Config,
    data::{rest::OpinionClient, websocket::WebSocketClient},
    chain::client::ChainClient,
    execution::RiskManager,
};
use alloy_primitives::Address;
use std::time::Duration;
use tokio::time;
use tracing::{info, warn, error, Level};
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Initialize Logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("🚀 Starting Opinion HFT Live Monitor...");

    // 2. Load Configuration
    let config = Config::from_env()?;
    
    // Validate critical config presence
    if config.api.api_key.is_empty() {
        error!("❌ API Key missing in .env");
        return Ok(());
    }
    info!("✅ Configuration loaded. Chain ID: {}", config.chain.chain_id);

    // 3. Initialize Clients
    info!("🔌 Initializing clients...");
    
    let rest_client = OpinionClient::new(&config.api)?;
    info!("✅ REST Client initialized");

    let mut ws_client = WebSocketClient::new(&config.websocket, config.api.api_key.clone());
    // Start connection in background
    ws_client.connect().await?;
    info!("✅ WebSocket Client connected");

    let account: Address = config.chain.eoa_address.parse().unwrap_or(Address::ZERO);
    let chain_client = ChainClient::new(&config.chain, account)?;
    info!("✅ Chain Client initialized (Account: {:?})", account);

    // 4. Initialize Risk Manager
    let mut risk_manager = RiskManager::new_with_defaults(rest_client.clone());
    info!("✅ Risk Manager initialized");

    // 5. Verification Loop
    info!("👀 Starting monitoring loop (Press Ctrl+C to stop)...");
    
    let mut interval = time::interval(Duration::from_secs(5));
    let mut check_count = 0;

    loop {
        interval.tick().await;
        check_count += 1;

        // --- A. Risk/Health Check ---
        match risk_manager.check_health().await {
            Ok(_) => info!("[Tick {}] System Health: 🟢 OK", check_count),
            Err(e) => warn!("[Tick {}] System Health: ⚠️  Risky - {}", check_count, e),
        }

        // --- B. Chain Connectivity Check (Balance Query) ---
        // Verify RPC connection every 30s
        if check_count % 6 == 0 {
            info!("[Tick {}] 🔍 Verifying Chain Connectivity...", check_count);
            // Using a dummy token ID for connectivity check
            match chain_client.balance_of(alloy_primitives::U256::from(0)).await {
                Ok(_) => info!("[Tick {}] Chain Connectivity: 🟢 OK", check_count),
                Err(e) => error!("[Tick {}] Chain Connectivity: 🔴 Failed - {}", check_count, e),
            }
        }

        // --- C. WebSocket Status ---
        if !ws_client.is_connected().await {
             error!("[Tick {}] WebSocket: 🔴 Disconnected", check_count);
        }
    }
}
