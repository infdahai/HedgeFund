use alloy::network::EthereumWallet;
use alloy::providers::{Provider, ProviderBuilder};
use alloy::signers::local::PrivateKeySigner;
use alloy::signers::Signer;
use alloy::transports::http::Http;
use alloy_primitives::{Address, U256};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::Instant;

use crate::chain::conditional_tokens::IConditionalTokens::IConditionalTokensInstance;
use crate::config::{ChainConfig, CHAIN_ID, CONDITIONAL_TOKENS_ADDRESS};
use crate::error::{Error, Result};
use crate::types::position::Position;

pub struct ChainClient {
    // Primary and fallback providers
    primary_provider: Arc<alloy::providers::RootProvider<Http<reqwest::Client>>>,
    fallback_provider: Option<Arc<alloy::providers::RootProvider<Http<reqwest::Client>>>>,

    // Health tracking
    primary_healthy: Arc<AtomicBool>,
    last_health_check: Arc<Mutex<Instant>>,

    // Contract and account info
    contract_address: Address,
    account: Address,
    wallet: Option<PrivateKeySigner>,

    // RPC configuration
    primary_rpc_url: String,
    fallback_rpc_url: Option<String>,
    health_check_interval: Duration,
}

impl ChainClient {
    pub fn new(config: &ChainConfig, account: Address) -> Result<Self> {
        let primary_url = config.primary_rpc_url.as_ref().unwrap_or(&config.rpc_url);

        let primary_provider = ProviderBuilder::new().on_http(
            primary_url
                .parse::<::url::Url>()
                .map_err(|e| Error::ConfigError(format!("invalid primary RPC URL: {}", e)))?,
        );

        let fallback_provider = if let Some(fallback_url) = &config.fallback_rpc_url {
            let provider = ProviderBuilder::new().on_http(
                fallback_url
                    .parse::<::url::Url>()
                    .map_err(|e| Error::ConfigError(format!("invalid fallback RPC URL: {}", e)))?,
            );
            Some(Arc::new(provider))
        } else {
            None
        };

        let contract_address: Address = config
            .conditional_tokens_address
            .parse()
            .unwrap_or_else(|_| CONDITIONAL_TOKENS_ADDRESS.parse().unwrap());

        let wallet = if !config.private_key.expose_secret().is_empty() {
            Some(
                PrivateKeySigner::from_str(config.private_key.expose_secret())
                    .map_err(|e| Error::ConfigError(format!("Invalid private key: {}", e)))?
                    .with_chain_id(Some(config.chain_id)),
            )
        } else {
            None
        };

        Ok(Self {
            primary_provider: Arc::new(primary_provider),
            fallback_provider,
            primary_healthy: Arc::new(AtomicBool::new(true)),
            last_health_check: Arc::new(Mutex::new(Instant::now())),
            contract_address,
            account,
            wallet,
            primary_rpc_url: primary_url.clone(),
            fallback_rpc_url: config.fallback_rpc_url.clone(),
            health_check_interval: config.health_check_interval,
        })
    }

    async fn get_healthy_provider(
        &self,
    ) -> Arc<alloy::providers::RootProvider<Http<reqwest::Client>>> {
        let should_check = {
            let last_check = self.last_health_check.lock().await;
            last_check.elapsed() > self.health_check_interval
        };

        if should_check && !self.primary_healthy.load(Ordering::Relaxed) {
            self.try_recover_primary().await;
        }

        if self.primary_healthy.load(Ordering::Relaxed) {
            return self.primary_provider.clone();
        }

        if let Some(fallback) = &self.fallback_provider {
            tracing::warn!(
                "Primary RPC unhealthy ({}), using fallback",
                self.primary_rpc_url
            );
            return fallback.clone();
        }

        tracing::error!(
            "No healthy RPC available, forcing primary ({})",
            self.primary_rpc_url
        );
        self.primary_provider.clone()
    }

    async fn try_recover_primary(&self) {
        match self.primary_provider.get_chain_id().await {
            Ok(id) if id == CHAIN_ID => {
                self.primary_healthy.store(true, Ordering::Relaxed);
                tracing::info!("Primary RPC recovered: {}", self.primary_rpc_url);
            }
            Ok(wrong_id) => {
                tracing::warn!(
                    "Primary RPC returned wrong chain ID: {} (expected {})",
                    wrong_id,
                    CHAIN_ID
                );
            }
            Err(e) => {
                tracing::warn!("Primary RPC still unhealthy: {}", e);
            }
        }

        *self.last_health_check.lock().await = Instant::now();
    }

    fn is_node_failure(&self, error: &alloy::contract::Error) -> bool {
        let err_str = error.to_string().to_lowercase();
        err_str.contains("connection refused")
            || err_str.contains("network unreachable")
            || err_str.contains("host unreachable")
            || err_str.contains("dns error")
            || err_str.contains("connection reset")
            || err_str.contains("timeout")
            || err_str.contains("500")
            || err_str.contains("502")
            || err_str.contains("503")
            || err_str.contains("504")
    }

    async fn mark_primary_unhealthy(&self) {
        if self.primary_healthy.swap(false, Ordering::Relaxed) {
            tracing::error!("Marking primary RPC as unhealthy: {}", self.primary_rpc_url);
        }
    }

    pub async fn balance_of(&self, token_id: U256) -> Result<U256> {
        for attempt in 1..=3 {
            let provider = self.get_healthy_provider().await;
            let contract = IConditionalTokensInstance::new(self.contract_address, provider);

            match contract.balanceOf(self.account, token_id).call().await {
                Ok(balance) => return Ok(balance._0),
                Err(e) => {
                    let err_str: String = e.to_string();
                    let is_transient = err_str.contains("timeout")
                        || err_str.contains("connection")
                        || err_str.contains("503")
                        || err_str.contains("429");

                    let is_node_fail = self.is_node_failure(&e);
                    if is_node_fail {
                        self.mark_primary_unhealthy().await;
                    }

                    if (is_transient || is_node_fail) && attempt < 3 {
                        tracing::warn!("RPC error on attempt {}, retrying: {}", attempt, err_str);
                        tokio::time::sleep(std::time::Duration::from_millis(
                            500 * 2u64.pow(attempt as u32 - 1),
                        ))
                        .await;
                        continue;
                    }
                    return Err(Error::ContractError {
                        method: "balanceOf".into(),
                        reason: e.to_string(),
                    });
                }
            }
        }
        Err(Error::ContractError {
            method: "balanceOf".into(),
            reason: "max retries exceeded".into(),
        })
    }

    pub async fn balance_of_batch(&self, token_ids: &[U256]) -> Result<Vec<Position>> {
        if token_ids.is_empty() {
            return Ok(Vec::new());
        }

        for attempt in 1..=3 {
            let provider = self.get_healthy_provider().await;
            let contract = IConditionalTokensInstance::new(self.contract_address, provider);

            let accounts: Vec<Address> = vec![self.account; token_ids.len()];
            let ids: Vec<U256> = token_ids.to_vec();

            match contract
                .balanceOfBatch(accounts.clone(), ids.clone())
                .call()
                .await
            {
                Ok(balances) => {
                    let positions = token_ids
                        .iter()
                        .zip(balances._0.as_slice())
                        .map(|(token_id, balance)| Position::new(*token_id, *balance))
                        .collect();
                    return Ok(positions);
                }
                Err(e) => {
                    let err_str: String = e.to_string();
                    let is_transient = err_str.contains("timeout")
                        || err_str.contains("connection")
                        || err_str.contains("503")
                        || err_str.contains("429");

                    let is_node_fail = self.is_node_failure(&e);
                    if is_node_fail {
                        self.mark_primary_unhealthy().await;
                    }

                    if (is_transient || is_node_fail) && attempt < 3 {
                        tracing::warn!("RPC error on attempt {}, retrying: {}", attempt, err_str);
                        tokio::time::sleep(std::time::Duration::from_millis(
                            500 * 2u64.pow(attempt as u32 - 1),
                        ))
                        .await;
                        continue;
                    }
                    return Err(Error::ContractError {
                        method: "balanceOfBatch".into(),
                        reason: e.to_string(),
                    });
                }
            }
        }
        Err(Error::ContractError {
            method: "balanceOfBatch".into(),
            reason: "max retries exceeded".into(),
        })
    }

    /// Merges positions for a condition.
    ///
    /// This is the core logic for the "Mint-Sell-Merge" strategy.
    /// It combines outcome tokens back into collateral.
    pub async fn merge_positions(
        &self,
        condition_id: [u8; 32],
        partition: Vec<U256>,
        amount: U256,
    ) -> Result<String> {
        let wallet = self.wallet.as_ref().ok_or_else(|| {
            Error::ConfigError("Private key required for merging positions".into())
        })?;

        let signer = EthereumWallet::from(wallet.clone());
        let provider = ProviderBuilder::new()
            .with_recommended_fillers()
            .wallet(signer)
            .on_http(
                self.primary_rpc_url
                    .parse::<::url::Url>()
                    .map_err(|e| Error::ConfigError(e.to_string()))?,
            );

        let contract = IConditionalTokensInstance::new(self.contract_address, provider);

        let collateral_token = "0xe9e7CEA3DedcA5984780Bafc599bD69ADd087D56"
            .parse::<Address>()
            .unwrap();

        let parent_id_fixed = alloy_primitives::FixedBytes([0u8; 32]);
        let condition_id_fixed = alloy_primitives::FixedBytes(condition_id);

        let builder = contract.mergePositions(
            collateral_token,
            parent_id_fixed,
            condition_id_fixed,
            partition,
            amount,
        );

        let tx = builder.send().await.map_err(|e| Error::ContractError {
            method: "mergePositions".into(),
            reason: e.to_string(),
        })?;

        Ok(tx.tx_hash().to_string())
    }

    pub fn account(&self) -> Address {
        self.account
    }

    pub async fn get_block_number(&self) -> Result<u64> {
        for attempt in 1..=3 {
            let provider = self.get_healthy_provider().await;
            match provider.get_block_number().await {
                Ok(block_number) => return Ok(block_number),
                Err(e) => {
                    let err_str: String = e.to_string();
                    let is_transient = err_str.contains("timeout")
                        || err_str.contains("connection")
                        || err_str.contains("503")
                        || err_str.contains("429");

                    let is_node_fail = err_str.contains("connection refused")
                        || err_str.contains("network unreachable")
                        || err_str.contains("host unreachable")
                        || err_str.contains("dns error")
                        || err_str.contains("timeout");

                    if is_node_fail {
                        self.mark_primary_unhealthy().await;
                    }

                    if (is_transient || is_node_fail) && attempt < 3 {
                        tracing::warn!("RPC error on attempt {}, retrying: {}", attempt, err_str);
                        tokio::time::sleep(std::time::Duration::from_millis(
                            500 * 2u64.pow(attempt as u32 - 1),
                        ))
                        .await;
                        continue;
                    }
                    return Err(Error::ContractError {
                        method: "get_block_number".into(),
                        reason: e.to_string(),
                    });
                }
            }
        }
        Err(Error::ContractError {
            method: "get_block_number".into(),
            reason: "max retries exceeded".into(),
        })
    }

    pub fn contract_address(&self) -> Address {
        self.contract_address
    }
}

impl Clone for ChainClient {
    fn clone(&self) -> Self {
        Self {
            primary_provider: Arc::clone(&self.primary_provider),
            fallback_provider: self.fallback_provider.clone(),
            primary_healthy: Arc::clone(&self.primary_healthy),
            last_health_check: Arc::clone(&self.last_health_check),
            contract_address: self.contract_address,
            account: self.account,
            wallet: self.wallet.clone(),
            primary_rpc_url: self.primary_rpc_url.clone(),
            fallback_rpc_url: self.fallback_rpc_url.clone(),
            health_check_interval: self.health_check_interval,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_config() -> ChainConfig {
        ChainConfig {
            rpc_url: "https://bsc-dataseed.binance.org".into(),
            primary_rpc_url: None,
            fallback_rpc_url: None,
            chain_id: 56,
            private_key: crate::auth::secret::PrivateKey::default(),
            eoa_address: String::new(),
            conditional_tokens_address: CONDITIONAL_TOKENS_ADDRESS.into(),
            rpc_timeout: std::time::Duration::from_secs(5),
            batch_buffer_ms: 50,
            health_check_interval: std::time::Duration::from_secs(30),
        }
    }

    #[test]
    fn test_chain_client_creation() {
        let config = mock_config();
        let account = Address::ZERO;
        let client = ChainClient::new(&config, account);
        assert!(client.is_ok());
    }

    #[test]
    fn test_chain_client_addresses() {
        let config = mock_config();
        let account: Address = "0x1234567890123456789012345678901234567890"
            .parse()
            .unwrap();
        let client = ChainClient::new(&config, account).unwrap();

        assert_eq!(client.account(), account);
        assert_eq!(
            client.contract_address(),
            CONDITIONAL_TOKENS_ADDRESS.parse::<Address>().unwrap()
        );
    }

    #[test]
    fn test_chain_client_clone() {
        let config = mock_config();
        let account = Address::ZERO;
        let client1 = ChainClient::new(&config, account).unwrap();
        let client2 = client1.clone();

        assert_eq!(client1.account(), client2.account());
        assert_eq!(client1.contract_address(), client2.contract_address());
    }
}
