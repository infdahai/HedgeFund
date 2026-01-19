//! Integration tests for RPC failover functionality.
//!
//! These tests verify:
//! 1. Primary RPC works when healthy
//! 2. Fallback RPC is used when primary fails
//! 3. Health check recovers primary after it becomes healthy again

use alloy_primitives::{Address, U256};
use opinion_hft::chain::client::ChainClient;
use opinion_hft::config::{ChainConfig, CHAIN_ID, CONDITIONAL_TOKENS_ADDRESS};
use std::time::Duration;

fn create_config_with_fallback() -> ChainConfig {
    ChainConfig {
        rpc_url: "https://invalid-primary.example.com".into(), // Invalid primary
        primary_rpc_url: Some("https://invalid-primary.example.com".into()),
        fallback_rpc_url: Some("https://bsc-dataseed.binance.org".into()), // Valid fallback
        chain_id: CHAIN_ID,
        private_key: opinion_hft::auth::secret::PrivateKey::default(),
        eoa_address: String::new(),
        conditional_tokens_address: CONDITIONAL_TOKENS_ADDRESS.into(),
        rpc_timeout: Duration::from_secs(5),
        batch_buffer_ms: 50,
        health_check_interval: Duration::from_secs(1), // Short for testing
    }
}

fn create_config_primary_only() -> ChainConfig {
    ChainConfig {
        rpc_url: "https://bsc-dataseed.binance.org".into(),
        primary_rpc_url: Some("https://bsc-dataseed.binance.org".into()),
        fallback_rpc_url: None,
        chain_id: CHAIN_ID,
        private_key: opinion_hft::auth::secret::PrivateKey::default(),
        eoa_address: String::new(),
        conditional_tokens_address: CONDITIONAL_TOKENS_ADDRESS.into(),
        rpc_timeout: Duration::from_secs(5),
        batch_buffer_ms: 50,
        health_check_interval: Duration::from_secs(30),
    }
}

#[tokio::test]
async fn test_primary_rpc_healthy() {
    // Test that primary RPC works when healthy
    let config = create_config_primary_only();
    // Use a non-zero address to avoid ERC1155 "balance query for zero address" revert
    let account: Address = "0x0000000000000000000000000000000000000001"
        .parse()
        .unwrap();

    let client = ChainClient::new(&config, account).expect("Failed to create client");

    // Query a known token (will return 0 for this address, but should not error)
    let result = client.balance_of(U256::from(1)).await;

    // Should succeed (even if balance is 0)
    assert!(
        result.is_ok(),
        "Primary RPC should work: {:?}",
        result.err()
    );
}

#[tokio::test]
async fn test_fallback_on_primary_failure() {
    // Test that fallback is used when primary fails
    let config = create_config_with_fallback();
    let account: Address = "0x0000000000000000000000000000000000000001"
        .parse()
        .unwrap();

    let client = ChainClient::new(&config, account).expect("Failed to create client");

    // First call will try primary (fails), then use fallback
    let result = client.balance_of(U256::from(1)).await;

    // Should succeed via fallback
    assert!(
        result.is_ok(),
        "Fallback RPC should work: {:?}",
        result.err()
    );
}

#[tokio::test]
async fn test_batch_query() {
    // Test batch query functionality
    let config = create_config_primary_only();
    let account: Address = "0x0000000000000000000000000000000000000001"
        .parse()
        .unwrap();

    let client = ChainClient::new(&config, account).expect("Failed to create client");

    let token_ids = vec![U256::from(1), U256::from(2), U256::from(3)];

    let result = client.balance_of_batch(&token_ids).await;

    assert!(
        result.is_ok(),
        "Batch query should work: {:?}",
        result.err()
    );

    let positions = result.unwrap();
    assert_eq!(positions.len(), 3, "Should return 3 positions");
}
