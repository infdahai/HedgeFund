use alloy::network::EthereumWallet;
use alloy::providers::ProviderBuilder;
use alloy::signers::local::PrivateKeySigner;
use alloy::signers::Signer;
use alloy::transports::http::Http;
use alloy_primitives::{Address, U256};
use std::sync::Arc;
use std::str::FromStr;

use crate::chain::conditional_tokens::IConditionalTokens::IConditionalTokensInstance;
use crate::config::{ChainConfig, CONDITIONAL_TOKENS_ADDRESS};
use crate::error::{Error, Result};
use crate::types::position::Position;

pub struct ChainClient {
    provider: Arc<alloy::providers::RootProvider<Http<reqwest::Client>>>,
    contract_address: Address,
    account: Address,
    wallet: Option<PrivateKeySigner>,
    rpc_url: String,
}

impl ChainClient {
    pub fn new(config: &ChainConfig, account: Address) -> Result<Self> {
        let provider = ProviderBuilder::new().on_http(
            config
                .rpc_url
                .parse::<::url::Url>()
                .map_err(|e| Error::ConfigError(format!("invalid RPC URL: {}", e)))?,
        );

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
            provider: Arc::new(provider),
            contract_address,
            account,
            wallet,
            rpc_url: config.rpc_url.clone(),
        })
    }

    pub async fn balance_of(&self, token_id: U256) -> Result<U256> {
        let provider = self.provider.clone();
        let contract = IConditionalTokensInstance::new(self.contract_address, provider);

        for attempt in 1..=3 {
            match contract.balanceOf(self.account, token_id).call().await {
                Ok(balance) => return Ok(balance._0),
                Err(e) => {
                    let err_str: String = e.to_string();
                    let is_transient = err_str.contains("timeout")
                        || err_str.contains("connection")
                        || err_str.contains("503")
                        || err_str.contains("429");

                    if is_transient && attempt < 3 {
                        tracing::warn!("RPC error on attempt {}, retrying: {}", attempt, err_str);
                        tokio::time::sleep(std::time::Duration::from_millis(500 * 2u64.pow(attempt as u32 - 1))).await;
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
            let provider = self.provider.clone();
            let contract = IConditionalTokensInstance::new(self.contract_address, provider);

            let accounts: Vec<Address> = vec![self.account; token_ids.len()];
            let ids: Vec<U256> = token_ids.to_vec();

            match contract.balanceOfBatch(accounts.clone(), ids.clone()).call().await {
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
                    
                    if is_transient && attempt < 3 {
                        tracing::warn!("RPC error on attempt {}, retrying: {}", attempt, err_str);
                        tokio::time::sleep(std::time::Duration::from_millis(500 * 2u64.pow(attempt as u32 - 1))).await;
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

        // Create a signer provider for this transaction
        let signer = EthereumWallet::from(wallet.clone());
        let provider = ProviderBuilder::new()
            .with_recommended_fillers()
            .wallet(signer)
            .on_http(self.rpc_url.parse::<::url::Url>().map_err(|e| Error::ConfigError(e.to_string()))?);

        let contract = IConditionalTokensInstance::new(self.contract_address, provider);
        
        let collateral_token = "0xe9e7CEA3DedcA5984780Bafc599bD69ADd087D56".parse::<Address>().unwrap();
        
        let parent_id_fixed = alloy_primitives::FixedBytes([0u8; 32]);
        let condition_id_fixed = alloy_primitives::FixedBytes(condition_id);

        let builder = contract.mergePositions(
            collateral_token,
            parent_id_fixed,
            condition_id_fixed,
            partition,
            amount
        );

        let tx = builder
            .send()
            .await
            .map_err(|e| Error::ContractError {
                method: "mergePositions".into(),
                reason: e.to_string(),
            })?;

        Ok(tx.tx_hash().to_string())
    }



    pub fn account(&self) -> Address {
        self.account
    }

    pub fn contract_address(&self) -> Address {
        self.contract_address
    }
}

impl Clone for ChainClient {
    fn clone(&self) -> Self {
        Self {
            provider: Arc::clone(&self.provider),
            contract_address: self.contract_address,
            account: self.account,
            wallet: self.wallet.clone(),
            rpc_url: self.rpc_url.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_config() -> ChainConfig {
        ChainConfig {
            rpc_url: "https://bsc-dataseed.binance.org".into(),
            chain_id: 56,
            private_key: crate::auth::secret::PrivateKey::default(),
            eoa_address: String::new(),
            conditional_tokens_address: CONDITIONAL_TOKENS_ADDRESS.into(),
            rpc_timeout: std::time::Duration::from_secs(5),
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
