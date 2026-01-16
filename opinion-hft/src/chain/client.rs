use alloy::transports::http::Http;
use alloy_primitives::{Address, U256};
use std::sync::Arc;

use crate::chain::conditional_tokens::IConditionalTokens::IConditionalTokensInstance;
use crate::config::{ChainConfig, CONDITIONAL_TOKENS_ADDRESS};
use crate::error::{Error, Result};
use crate::types::position::Position;

pub struct ChainClient {
    provider: Arc<alloy::providers::RootProvider<Http<reqwest::Client>>>,
    contract_address: Address,
    account: Address,
}

impl ChainClient {
    pub fn new(config: &ChainConfig, account: Address) -> Result<Self> {
        let provider = alloy::providers::ProviderBuilder::new().on_http(
            config
                .rpc_url
                .parse()
                .map_err(|e| Error::ConfigError(format!("invalid RPC URL: {}", e)))?,
        );

        let contract_address: Address = config
            .conditional_tokens_address
            .parse()
            .unwrap_or_else(|_| CONDITIONAL_TOKENS_ADDRESS.parse().unwrap());

        Ok(Self {
            provider: Arc::new(provider),
            contract_address,
            account,
        })
    }

    pub async fn balance_of(&self, token_id: U256) -> Result<U256> {
        let contract = IConditionalTokensInstance::new(self.contract_address, &*self.provider);

        let balance = contract
            .balanceOf(self.account, token_id)
            .call()
            .await
            .map_err(|e| Error::ContractError {
                method: "balanceOf".into(),
                reason: e.to_string(),
            })?;

        Ok(balance._0)
    }

    pub async fn balance_of_batch(&self, token_ids: &[U256]) -> Result<Vec<Position>> {
        if token_ids.is_empty() {
            return Ok(Vec::new());
        }

        let contract = IConditionalTokensInstance::new(self.contract_address, &*self.provider);

        let accounts: Vec<Address> = vec![self.account; token_ids.len()];
        let ids: Vec<U256> = token_ids.to_vec();

        let balances = contract
            .balanceOfBatch(accounts, ids)
            .call()
            .await
            .map_err(|e| Error::ContractError {
                method: "balanceOfBatch".into(),
                reason: e.to_string(),
            })?;

        let positions = token_ids
            .iter()
            .zip(balances._0.iter())
            .map(|(token_id, balance)| Position::new(*token_id, *balance))
            .collect();

        Ok(positions)
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
            private_key: String::new(),
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
