//! Configuration for the Opinion HFT framework.
//!
//! Contains all configurable parameters with sensible defaults based on
//! the Opinion Trade API specifications.

use std::time::Duration;

use crate::error::{Error, Result};

// ============ Critical Constants (DO NOT MODIFY) ============

/// EIP-712 domain name - NOTE THE SPACE!
pub const EIP712_DOMAIN_NAME: &str = "OPINION CTF Exchange";

/// EIP-712 domain version
pub const EIP712_DOMAIN_VERSION: &str = "1";

/// BNB Chain ID
pub const CHAIN_ID: u64 = 56;

/// ConditionalTokens (ERC-1155) contract address
pub const CONDITIONAL_TOKENS_ADDRESS: &str = "0xAD1a38cEc043e70E83a3eC30443dB285ED10D774";

/// MultiSend contract address
pub const MULTISEND_ADDRESS: &str = "0x998739BFdAAdde7C933B942a68053933098f9EDa";

/// REST API base URL
pub const REST_API_BASE_URL: &str = "https://proxy.opinion.trade:8443/openapi/";

/// WebSocket endpoint
pub const WEBSOCKET_URL: &str = "wss://ws.opinion.trade";

/// Default API rate limit (requests per second)
pub const DEFAULT_RATE_LIMIT_TPS: u32 = 15;

/// Price precision (3 decimal places: 0.001 - 0.999)
pub const PRICE_DECIMALS: u32 = 3;

/// Minimum price
pub const MIN_PRICE: f64 = 0.001;

/// Maximum price
pub const MAX_PRICE: f64 = 0.999;

// ============ Configuration Structs ============

/// Main configuration for the Opinion HFT framework.
#[derive(Debug, Clone, Default)]
pub struct Config {
    /// API configuration
    pub api: ApiConfig,

    /// Chain/RPC configuration
    pub chain: ChainConfig,

    /// Execution configuration
    pub execution: ExecutionConfig,

    /// WebSocket configuration
    pub websocket: WebSocketConfig,

    /// Confirmation tracking configuration
    pub confirmation: ConfirmationConfig,
}

/// API-related configuration.
#[derive(Debug, Clone)]
pub struct ApiConfig {
    /// API key for authentication
    pub api_key: String,

    /// REST API base URL
    pub rest_base_url: String,

    /// Request timeout
    pub timeout: Duration,

    /// Maximum retry attempts for 5xx errors
    pub max_retries: u32,

    /// Base delay for exponential backoff
    pub retry_base_delay: Duration,
}

/// Chain/RPC configuration.
#[derive(Debug, Clone)]
pub struct ChainConfig {
    /// RPC endpoint URL
    pub rpc_url: String,

    /// Chain ID (should be 56 for BNB Chain)
    pub chain_id: u64,

    /// Private key for signing (hex encoded, without 0x prefix)
    pub private_key: String,

    /// ConditionalTokens contract address
    pub conditional_tokens_address: String,

    /// RPC request timeout
    pub rpc_timeout: Duration,
}

/// Execution-related configuration.
#[derive(Debug, Clone)]
pub struct ExecutionConfig {
    /// Rate limit (requests per second)
    pub rate_limit_tps: u32,

    /// Burst capacity for rate limiter
    pub burst_capacity: u32,

    /// Maximum orders in priority queue
    pub max_queue_size: usize,

    /// Order expiration buffer (added to current time)
    pub order_expiration_buffer: Duration,
}

/// WebSocket configuration.
#[derive(Debug, Clone)]
pub struct WebSocketConfig {
    /// WebSocket URL
    pub url: String,

    /// Reconnection settings
    pub reconnect: ReconnectConfig,

    /// Ping interval for keepalive
    pub ping_interval: Duration,

    /// Pong timeout (disconnect if no pong received)
    pub pong_timeout: Duration,
}

/// Reconnection configuration for WebSocket.
#[derive(Debug, Clone)]
pub struct ReconnectConfig {
    /// Initial delay before first reconnection attempt
    pub initial_delay: Duration,

    /// Maximum delay between reconnection attempts
    pub max_delay: Duration,

    /// Multiplier for exponential backoff
    pub multiplier: f64,

    /// Maximum number of reconnection attempts (None = unlimited)
    pub max_attempts: Option<u32>,
}

/// Confirmation tracking configuration.
#[derive(Debug, Clone)]
pub struct ConfirmationConfig {
    /// Timeout for order confirmation
    pub timeout: Duration,

    /// Polling interval for on-chain position check
    pub poll_interval: Duration,

    /// Confirmation mode
    pub mode: ConfirmationMode,
}

/// Confirmation mode determines how order fills are verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmationMode {
    /// Trust WebSocket data for fill quantities (faster, less reliable)
    TrustWebSocket,

    /// Always query on-chain for fill quantities (slower, reliable)
    OnChainOnly,

    /// Use WebSocket as trigger, verify with on-chain query (recommended)
    Hybrid,
}

// ============ Default Implementations ============

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            rest_base_url: REST_API_BASE_URL.to_string(),
            timeout: Duration::from_secs(10),
            max_retries: 3,
            retry_base_delay: Duration::from_millis(100),
        }
    }
}

impl Default for ChainConfig {
    fn default() -> Self {
        Self {
            rpc_url: String::new(),
            chain_id: CHAIN_ID,
            private_key: String::new(),
            conditional_tokens_address: CONDITIONAL_TOKENS_ADDRESS.to_string(),
            rpc_timeout: Duration::from_secs(5),
        }
    }
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            rate_limit_tps: DEFAULT_RATE_LIMIT_TPS,
            burst_capacity: 3,
            max_queue_size: 1000,
            order_expiration_buffer: Duration::from_secs(300), // 5 minutes
        }
    }
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            url: WEBSOCKET_URL.to_string(),
            reconnect: ReconnectConfig::default(),
            ping_interval: Duration::from_secs(30),
            pong_timeout: Duration::from_secs(10),
        }
    }
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(16),
            multiplier: 2.0,
            max_attempts: None, // Unlimited
        }
    }
}

impl Default for ConfirmationConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            poll_interval: Duration::from_millis(500),
            mode: ConfirmationMode::Hybrid, // Recommended default
        }
    }
}

// ============ Builder Pattern ============

impl Config {
    /// Create a new configuration builder.
    pub fn builder() -> ConfigBuilder {
        ConfigBuilder::default()
    }

    /// Validate the configuration.
    pub fn validate(&self) -> Result<()> {
        if self.api.api_key.is_empty() {
            return Err(Error::MissingConfig {
                field: "api.api_key".into(),
            });
        }

        if self.chain.rpc_url.is_empty() {
            return Err(Error::MissingConfig {
                field: "chain.rpc_url".into(),
            });
        }

        if self.chain.private_key.is_empty() {
            return Err(Error::MissingConfig {
                field: "chain.private_key".into(),
            });
        }

        if self.chain.chain_id != CHAIN_ID {
            return Err(Error::ConfigError(format!(
                "chain_id must be {} (BNB Chain), got {}",
                CHAIN_ID, self.chain.chain_id
            )));
        }

        if self.execution.rate_limit_tps == 0 {
            return Err(Error::ConfigError(
                "rate_limit_tps must be greater than 0".into(),
            ));
        }

        Ok(())
    }
}

/// Builder for constructing Config with validation.
#[derive(Debug, Default)]
pub struct ConfigBuilder {
    config: Config,
}

impl ConfigBuilder {
    /// Set the API key.
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.config.api.api_key = key.into();
        self
    }

    /// Set the RPC URL.
    pub fn rpc_url(mut self, url: impl Into<String>) -> Self {
        self.config.chain.rpc_url = url.into();
        self
    }

    /// Set the private key (hex encoded, with or without 0x prefix).
    pub fn private_key(mut self, key: impl Into<String>) -> Self {
        let key = key.into();
        self.config.chain.private_key = key.strip_prefix("0x").unwrap_or(&key).to_string();
        self
    }

    /// Set the confirmation mode.
    pub fn confirmation_mode(mut self, mode: ConfirmationMode) -> Self {
        self.config.confirmation.mode = mode;
        self
    }

    /// Set the rate limit TPS.
    pub fn rate_limit_tps(mut self, tps: u32) -> Self {
        self.config.execution.rate_limit_tps = tps;
        self
    }

    /// Set the API request timeout.
    pub fn api_timeout(mut self, timeout: Duration) -> Self {
        self.config.api.timeout = timeout;
        self
    }

    /// Set the WebSocket URL.
    pub fn websocket_url(mut self, url: impl Into<String>) -> Self {
        self.config.websocket.url = url.into();
        self
    }

    /// Set the REST API base URL.
    pub fn rest_base_url(mut self, url: impl Into<String>) -> Self {
        self.config.api.rest_base_url = url.into();
        self
    }

    /// Build and validate the configuration.
    pub fn build(self) -> Result<Config> {
        self.config.validate()?;
        Ok(self.config)
    }

    /// Build without validation (for testing).
    pub fn build_unchecked(self) -> Config {
        self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.chain.chain_id, 56);
        assert_eq!(config.execution.rate_limit_tps, 15);
        assert_eq!(config.confirmation.mode, ConfirmationMode::Hybrid);
    }

    #[test]
    fn test_config_builder() {
        let config = Config::builder()
            .api_key("test-key")
            .rpc_url("https://bsc-rpc.example.com")
            .private_key("0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef")
            .confirmation_mode(ConfirmationMode::OnChainOnly)
            .build()
            .unwrap();

        assert_eq!(config.api.api_key, "test-key");
        assert_eq!(config.chain.rpc_url, "https://bsc-rpc.example.com");
        assert_eq!(config.confirmation.mode, ConfirmationMode::OnChainOnly);
        // Private key should have 0x stripped
        assert!(!config.chain.private_key.starts_with("0x"));
    }

    #[test]
    fn test_config_validation_missing_api_key() {
        let result = Config::builder()
            .rpc_url("https://bsc-rpc.example.com")
            .private_key("1234")
            .build();

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, Error::MissingConfig { field } if field == "api.api_key"));
    }

    #[test]
    fn test_config_validation_missing_rpc_url() {
        let result = Config::builder()
            .api_key("test-key")
            .private_key("1234")
            .build();

        assert!(result.is_err());
    }

    #[test]
    fn test_constants() {
        assert_eq!(EIP712_DOMAIN_NAME, "OPINION CTF Exchange");
        assert_eq!(CHAIN_ID, 56);
        assert_eq!(PRICE_DECIMALS, 3);
        assert!((MIN_PRICE - 0.001).abs() < f64::EPSILON);
        assert!((MAX_PRICE - 0.999).abs() < f64::EPSILON);
    }
}
