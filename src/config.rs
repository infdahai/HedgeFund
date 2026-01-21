//! Configuration for the Opinion HFT framework.
//!
//! Contains all configurable parameters with sensible defaults based on
//! the Opinion Trade API specifications.

use std::str::FromStr;
use std::time::Duration;

use crate::auth::secret::{PrivateKey, SecretLoader, SecretString};
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

/// Price precision (1 decimal place: 0.1 - 0.9)
pub const PRICE_DECIMALS: u32 = 1;

/// Minimum price
pub const MIN_PRICE: f64 = 0.1;

/// Maximum price
pub const MAX_PRICE: f64 = 0.9;

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

    /// Risk management configuration
    pub risk: RiskConfig,

    /// Execution environment (local/aws)
    pub environment: Environment,

    pub data: DataCollectionConfig,

    pub parquet: ParquetConfig,
}

/// Risk management configuration.
#[derive(Debug, Clone)]
pub struct RiskConfig {
    /// Maximum allowed API latency before warning/stopping (legacy, for backward compatibility)
    pub max_api_latency: Duration,
    /// Maximum allowed API latency for local environment (proxy, higher latency)
    pub max_api_latency_local: Duration,
    /// Maximum allowed API latency for AWS environment (direct, low latency)
    pub max_api_latency_aws: Duration,
    /// Maximum allowed clock drift between local and server
    pub max_clock_drift: Duration,
    /// Number of consecutive errors to trigger circuit breaker
    pub max_consecutive_errors: u32,
    /// Maximum daily loss before halting (in USDT)
    pub max_daily_loss: f64,
    /// Maximum drawdown percentage before halting (0.0 - 1.0)
    pub max_drawdown_pct: f64,
    /// Maximum slippage tolerance for arbitrage trades (0.0 - 1.0)
    pub max_slippage_pct: f64,
    /// Maximum execution time for atomic arbitrage (ms)
    pub max_execution_time_ms: u64,
    /// Maximum position per market (in USDT)
    pub max_position_per_market: f64,
    /// Maximum total exposure (in USDT)
    pub max_total_exposure: f64,
}

#[derive(Debug, Clone)]
pub struct DataCollectionConfig {
    pub enabled: bool,
    pub db_path: String,
    pub wal: bool,
    pub batch_size: usize,
    pub flush_interval_ms: u64,
    pub orderbook_depth: usize,
    pub queue_capacity: usize,
}

#[derive(Debug, Clone)]
pub struct ParquetConfig {
    pub enabled: bool,
    pub dir: String,
    pub batch_size: usize,
    pub flush_interval_ms: u64,
    pub rotate_size_mb: u64,
    pub queue_capacity: usize,
    pub compression: String,
    pub heartbeat_interval_secs: u64,
}

impl DataCollectionConfig {
    pub fn from_env() -> Self {
        let enabled = std::env::var("OPINION_DATA_ENABLED")
            .ok()
            .and_then(|v| v.parse::<bool>().ok())
            .unwrap_or(true);

        let db_path = std::env::var("OPINION_DATA_DB_PATH")
            .ok()
            .filter(|p| !p.trim().is_empty())
            .unwrap_or_else(|| "data/opinion-hft.sqlite".to_string());

        let wal = std::env::var("OPINION_DATA_WAL")
            .ok()
            .and_then(|v| v.parse::<bool>().ok())
            .unwrap_or(true);

        let batch_size = std::env::var("OPINION_DATA_BATCH_SIZE")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(200);

        let flush_interval_ms = std::env::var("OPINION_DATA_FLUSH_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(1000);

        let orderbook_depth = std::env::var("OPINION_DATA_OB_DEPTH")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(10);

        let queue_capacity = std::env::var("OPINION_DATA_QUEUE_CAP")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(2000);

        Self {
            enabled,
            db_path,
            wal,
            batch_size,
            flush_interval_ms,
            orderbook_depth,
            queue_capacity,
        }
    }
}

impl Default for DataCollectionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            db_path: "data/opinion-hft.sqlite".to_string(),
            wal: true,
            batch_size: 200,
            flush_interval_ms: 1000,
            orderbook_depth: 10,
            queue_capacity: 2000,
        }
    }
}

impl ParquetConfig {
    pub fn from_env() -> Self {
        let enabled = std::env::var("OPINION_PARQUET_ENABLED")
            .ok()
            .and_then(|v| v.parse::<bool>().ok())
            .unwrap_or(false);

        let dir = std::env::var("OPINION_PARQUET_DIR")
            .ok()
            .filter(|p| !p.trim().is_empty())
            .unwrap_or_else(|| "data/parquet".to_string());

        let batch_size = std::env::var("OPINION_PARQUET_BATCH_SIZE")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(1000);

        let flush_interval_ms = std::env::var("OPINION_PARQUET_FLUSH_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(1000);

        let rotate_size_mb = std::env::var("OPINION_PARQUET_ROTATE_MB")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(100);

        let queue_capacity = std::env::var("OPINION_PARQUET_QUEUE_CAP")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(4096);

        let compression = std::env::var("OPINION_PARQUET_COMPRESSION")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| "zstd".to_string());

        let heartbeat_interval_secs = std::env::var("OPINION_PARQUET_HEARTBEAT_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(60);

        Self {
            enabled,
            dir,
            batch_size,
            flush_interval_ms,
            rotate_size_mb,
            queue_capacity,
            compression,
            heartbeat_interval_secs,
        }
    }
}

impl Default for ParquetConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            dir: "data/parquet".to_string(),
            batch_size: 1000,
            flush_interval_ms: 1000,
            rotate_size_mb: 100,
            queue_capacity: 4096,
            compression: "zstd".to_string(),
            heartbeat_interval_secs: 60,
        }
    }
}

impl RiskConfig {
    pub fn get_max_api_latency(&self, env: Environment) -> Duration {
        match env {
            Environment::Local => self.max_api_latency_local,
            Environment::Aws => self.max_api_latency_aws,
        }
    }
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            max_api_latency: Duration::from_millis(3000),
            max_api_latency_local: Duration::from_millis(3000),
            max_api_latency_aws: Duration::from_millis(500),
            max_clock_drift: Duration::from_secs(2),
            max_consecutive_errors: 5,
            max_daily_loss: 100.0,
            max_drawdown_pct: 0.10,
            max_slippage_pct: 0.005,
            max_execution_time_ms: 500,
            max_position_per_market: 500.0,
            max_total_exposure: 2000.0,
        }
    }
}

impl ExecutionConfig {
    pub fn get_health_check_interval(&self, env: Environment) -> Duration {
        match env {
            Environment::Local => self.health_check_interval_local,
            Environment::Aws => self.health_check_interval_aws,
        }
    }
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
    /// RPC endpoint URL (legacy, kept for backward compatibility)
    pub rpc_url: String,

    /// Primary RPC endpoint URL (main node)
    pub primary_rpc_url: Option<String>,

    /// Fallback RPC endpoint URL (backup node)
    pub fallback_rpc_url: Option<String>,

    /// Chain ID (should be 56 for BNB Chain)
    pub chain_id: u64,

    /// Private key for signing (wrapped in Secret for security)
    pub private_key: PrivateKey,

    /// EOA address for position tracking
    pub eoa_address: String,

    /// ConditionalTokens contract address
    pub conditional_tokens_address: String,

    /// RPC request timeout
    pub rpc_timeout: Duration,

    /// Batch request buffer window in milliseconds (for batching balanceOf calls)
    pub batch_buffer_ms: u64,

    /// Health check interval for RPC endpoint monitoring
    pub health_check_interval: Duration,
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

    pub health_check_interval_local: Duration,

    pub health_check_interval_aws: Duration,

    /// Disable all order submission (scan-only mode)
    pub scan_only: bool,
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

/// Execution environment for the bot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Environment {
    Local,
    #[default]
    Aws,
}

impl std::str::FromStr for Environment {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "local" => Ok(Self::Local),
            "aws" => Ok(Self::Aws),
            _ => Err(format!("invalid environment: {}", s)),
        }
    }
}

impl Environment {
    pub fn is_local(&self) -> bool {
        matches!(self, Self::Local)
    }
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
            primary_rpc_url: None,
            fallback_rpc_url: None,
            chain_id: CHAIN_ID,
            private_key: PrivateKey::default(),
            eoa_address: String::new(),
            conditional_tokens_address: CONDITIONAL_TOKENS_ADDRESS.to_string(),
            rpc_timeout: Duration::from_secs(5),
            batch_buffer_ms: 50,
            health_check_interval: Duration::from_secs(30),
        }
    }
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            rate_limit_tps: DEFAULT_RATE_LIMIT_TPS,
            burst_capacity: 3,
            max_queue_size: 1000,
            order_expiration_buffer: Duration::from_secs(300),
            health_check_interval_local: Duration::from_secs(15),
            health_check_interval_aws: Duration::from_secs(3),
            scan_only: false,
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

        if self.chain.private_key.expose_secret().is_empty() {
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

    /// Load configuration from environment variables.
    /// Reads from .env file if present, then from environment.
    pub fn from_env() -> Result<Config> {
        let _ = dotenvy::dotenv();

        let api_key = std::env::var("OPINION_API_KEY").map_err(|_| Error::MissingConfig {
            field: "OPINION_API_KEY".into(),
        })?;

        // Primary RPC URL (if not set, fallback to BSC_RPC_URL for compatibility)
        let primary_rpc = std::env::var("OPINION_RPC_URL_PRIMARY").ok();
        let rpc_url = primary_rpc.clone().unwrap_or_else(|| {
            std::env::var("BSC_RPC_URL")
                .unwrap_or_else(|_| "https://bsc-dataseed.binance.org".to_string())
        });

        // Fallback RPC URL (optional)
        let fallback_rpc = std::env::var("OPINION_RPC_URL_FALLBACK").ok();

        // Batch buffer window (default: 50ms)
        let batch_buffer_ms = std::env::var("OPINION_BATCH_BUFFER_MS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(50);

        // Health check interval (default: 30s)
        let health_check_interval_secs = std::env::var("OPINION_HEALTH_CHECK_INTERVAL")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(30);

        // Use SecretLoader to securely load private key
        let private_key =
            SecretLoader::load_private_key().unwrap_or_else(|_| PrivateKey::default());

        let eoa_address = std::env::var("OPINION_EOA_ADDRESS").unwrap_or_default();

        let ws_url = std::env::var("OPINION_WS_URL")
            .unwrap_or_else(|_| WEBSOCKET_URL.to_string())
            .trim_end_matches('/')
            .to_string();

        let rest_url =
            std::env::var("OPINION_REST_URL").unwrap_or_else(|_| REST_API_BASE_URL.to_string());

        let environment = std::env::var("OPINION_ENV")
            .ok()
            .and_then(|e| Environment::from_str(&e).ok())
            .unwrap_or_default();

        let data_config = DataCollectionConfig::from_env();
        let parquet_config = ParquetConfig::from_env();
        let scan_only = parse_bool_env(std::env::var("OPINION_SCAN_ONLY").ok());

        let mut builder = Config::builder()
            .api_key(api_key)
            .rpc_url(rpc_url)
            .websocket_url(ws_url)
            .rest_base_url(rest_url)
            .environment(environment)
            .data_collection(data_config)
            .parquet(parquet_config);

        if !private_key.expose_secret().is_empty() {
            builder.config.chain.private_key = private_key;
        }

        if !eoa_address.is_empty() {
            builder = builder.eoa_address(eoa_address);
        }

        // Set new RPC configuration fields
        builder.config.chain.primary_rpc_url = primary_rpc;
        builder.config.chain.fallback_rpc_url = fallback_rpc;
        builder.config.chain.batch_buffer_ms = batch_buffer_ms;
        builder.config.chain.health_check_interval =
            Duration::from_secs(health_check_interval_secs);

        builder.scan_only(scan_only).build()
    }
}

#[derive(Debug, Default)]
pub struct ConfigBuilder {
    config: Config,
}

impl ConfigBuilder {
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.config.api.api_key = key.into();
        self
    }

    pub fn rpc_url(mut self, url: impl Into<String>) -> Self {
        self.config.chain.rpc_url = url.into();
        self
    }

    pub fn private_key(mut self, key: impl Into<String>) -> Self {
        let key = key.into();
        let cleaned = key.strip_prefix("0x").unwrap_or(&key).to_string();
        self.config.chain.private_key = SecretString::new(cleaned);
        self
    }

    pub fn confirmation_mode(mut self, mode: ConfirmationMode) -> Self {
        self.config.confirmation.mode = mode;
        self
    }

    pub fn rate_limit_tps(mut self, tps: u32) -> Self {
        self.config.execution.rate_limit_tps = tps;
        self
    }

    pub fn api_timeout(mut self, timeout: Duration) -> Self {
        self.config.api.timeout = timeout;
        self
    }

    pub fn websocket_url(mut self, url: impl Into<String>) -> Self {
        self.config.websocket.url = url.into();
        self
    }

    pub fn rest_base_url(mut self, url: impl Into<String>) -> Self {
        self.config.api.rest_base_url = url.into();
        self
    }

    pub fn data_collection(mut self, data: DataCollectionConfig) -> Self {
        self.config.data = data;
        self
    }

    pub fn parquet(mut self, config: ParquetConfig) -> Self {
        self.config.parquet = config;
        self
    }

    pub fn build(self) -> Result<Config> {
        self.config.validate()?;
        Ok(self.config)
    }

    pub fn build_unchecked(self) -> Config {
        self.config
    }

    pub fn eoa_address(mut self, address: impl Into<String>) -> Self {
        self.config.chain.eoa_address = address.into();
        self
    }

    pub fn environment(mut self, env: Environment) -> Self {
        self.config.environment = env;
        self
    }

    pub fn scan_only(mut self, enabled: bool) -> Self {
        self.config.execution.scan_only = enabled;
        self
    }
}

fn parse_bool_env(value: Option<String>) -> bool {
    value.is_some_and(|raw| {
        matches!(
            raw.trim().to_lowercase().as_str(),
            "1" | "true" | "yes" | "y" | "on"
        )
    })
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
        // Private key should have 0x stripped and be redacted
        assert_eq!(config.chain.private_key.expose_secret().len(), 64);
        assert!(!config.chain.private_key.expose_secret().starts_with("0x"));
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
        assert_eq!(PRICE_DECIMALS, 1);
        assert!((MIN_PRICE - 0.1).abs() < f64::EPSILON);
        assert!((MAX_PRICE - 0.9).abs() < f64::EPSILON);
    }
}
