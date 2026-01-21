use alloy_primitives::{Address, U256};
use opinion_hft::chain::ChainClient;
use opinion_hft::config::Config;
use opinion_hft::data::rest::OpinionClient;

mod common;
use common::init_proxy_env;

/// Loads configuration from .env for live tests.
fn load_test_config() -> Config {
    init_proxy_env();
    dotenvy::dotenv().ok();
    Config::from_env().expect("Failed to load config from .env. Ensure OPINION_API_KEY is set.")
}

#[tokio::test]

async fn test_get_user_auth_completeness() {
    let config = load_test_config();
    let client =
        OpinionClient::new(&config.api, config.environment).expect("Failed to create REST client");

    println!("🚀 Testing get_user_auth...");
    let auth_info = client
        .get_user_auth()
        .await
        .expect("Failed to get user auth");

    println!("✅ Wallet Address: {}", auth_info.wallet_address);
    println!(
        "✅ Registered Chain User Wallets: {:#?}",
        auth_info.wallet_users
    );

    // Verify wallet address format
    assert!(
        auth_info.wallet_address.starts_with("0x"),
        "Wallet address should start with 0x"
    );
    assert_eq!(
        auth_info.wallet_address.len(),
        42,
        "Wallet address should be 42 characters"
    );

    // Verify EOA consistency if configured in .env
    if !config.chain.eoa_address.is_empty() {
        assert_eq!(
            auth_info.wallet_address.to_lowercase(),
            config.chain.eoa_address.to_lowercase(),
            "EOA address mismatch between API and local config"
        );
        println!(
            "✅ EOA Consistency Verified: {} matches config",
            auth_info.wallet_address
        );
    }
}

#[tokio::test]

async fn test_proxy_address_resolution_bsc() {
    let config = load_test_config();
    let client =
        OpinionClient::new(&config.api, config.environment).expect("Failed to create REST client");

    let chain_id = 56; // BNB Chain (BSC)
    println!(
        "🚀 Testing proxy address resolution for chain {}...",
        chain_id
    );

    let proxy_address = client
        .get_proxy_address(chain_id)
        .await
        .expect("Failed to call get_proxy_address");

    match proxy_address {
        Some(addr) => {
            println!("✅ Proxy/Safe Address Found: {}", addr);
            assert!(addr.starts_with("0x"), "Proxy address should start with 0x");
            assert_eq!(addr.len(), 42, "Proxy address should be 42 characters");
        }
        None => {
            println!("⚠️ No proxy address registered for chain {}. This is expected if only EOA is used.", chain_id);
        }
    }
}

#[tokio::test]

async fn test_exchange_address_mapping() {
    let config = load_test_config();
    let client =
        OpinionClient::new(&config.api, config.environment).expect("Failed to create REST client");

    let chain_id = 56; // BSC
    let usdt_bsc = "0x55d398326f99059fF775485246999027B3197955"; // USDT on BSC

    println!("🚀 Fetching exchange address for USDT on BSC...");
    let exchange_address = client
        .get_exchange_address(chain_id, usdt_bsc)
        .await
        .expect("Failed to get exchange address");

    println!("✅ Exchange Address Resolved: {}", exchange_address);
    assert!(
        exchange_address.starts_with("0x"),
        "Exchange address should start with 0x"
    );
    assert_eq!(
        exchange_address.len(),
        42,
        "Exchange address should be 42 characters"
    );
}

#[tokio::test]

async fn test_live_account_balance_check() {
    let config = load_test_config();
    let rest_client =
        OpinionClient::new(&config.api, config.environment).expect("Failed to create REST client");

    let chain_id = 56;
    println!("🚀 Resolving target address for balance query on BSC...");

    let proxy_address_str = rest_client
        .get_proxy_address(chain_id)
        .await
        .expect("Failed to get proxy address");

    let target_address: Address = if let Some(addr) = proxy_address_str {
        println!("✅ Using Proxy/Safe address for balance query: {}", addr);
        addr.parse().expect("Invalid proxy address")
    } else {
        println!(
            "⚠️ No proxy found, using EOA address: {}",
            config.chain.eoa_address
        );
        config
            .chain
            .eoa_address
            .parse()
            .expect("Invalid EOA address in config")
    };

    let chain_client =
        ChainClient::new(&config.chain, target_address).expect("Failed to create chain client");

    // Query balance of Token ID 1 (standard practice to check connectivity)
    let token_id = U256::from(1u64);
    println!("🚀 Querying on-chain balance for Token ID {}...", token_id);

    let balance = chain_client
        .balance_of(token_id)
        .await
        .expect("Failed to query on-chain balance");

    println!("✅ Balance for {}: {}", target_address, balance);
}

#[tokio::test]

async fn test_auth_rejection_with_invalid_key() {
    let mut config = load_test_config();
    config.api.api_key = "invalid_api_key_for_testing_purposes".to_string();

    let client =
        OpinionClient::new(&config.api, config.environment).expect("Failed to create REST client");

    println!("🚀 Testing auth rejection with invalid key...");
    let result = client.get_user_auth().await;

    assert!(result.is_err(), "Authentication should have been rejected");
    println!("✅ Correctly rejected: {:?}", result.err());
}

#[tokio::test]

async fn test_address_format_validation() {
    let config = load_test_config();
    let client =
        OpinionClient::new(&config.api, config.environment).expect("Failed to create REST client");

    println!("🚀 Fetching quote tokens to validate address formats...");
    let tokens = client
        .get_quote_tokens(56)
        .await
        .expect("Failed to get quote tokens");

    assert!(!tokens.is_empty(), "Quote token list should not be empty");

    for token in tokens {
        println!(
            "🔍 Checking token: {} ({})",
            token.symbol, token.quote_token_address
        );

        // Validate checksum-ish format (all lowercase or standard hex)
        assert!(
            token.quote_token_address.starts_with("0x"),
            "Token address missing 0x"
        );
        assert_eq!(
            token.quote_token_address.len(),
            42,
            "Token address invalid length"
        );

        assert!(
            token.ctf_exchange_address.starts_with("0x"),
            "Exchange address missing 0x"
        );
        assert_eq!(
            token.ctf_exchange_address.len(),
            42,
            "Exchange address invalid length"
        );
    }
    println!("✅ All address formats validated successfully");
}
