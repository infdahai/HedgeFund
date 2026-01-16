//! Error types for the Opinion HFT framework.
//!
//! Uses `thiserror` for ergonomic error definitions with automatic
//! `std::error::Error` implementation.

use thiserror::Error;

/// Result type alias using our custom Error type.
pub type Result<T> = std::result::Result<T, Error>;

/// Main error type for the Opinion HFT framework.
#[derive(Error, Debug)]
pub enum Error {
    // ============ Authentication Errors ============
    /// Invalid private key format or length
    #[error("invalid private key: {0}")]
    InvalidPrivateKey(String),

    /// EIP-712 signing failed
    #[error("signing failed: {0}")]
    SigningError(String),

    /// Signature verification failed
    #[error("signature verification failed: {0}")]
    SignatureVerificationError(String),

    // ============ API Errors ============
    /// HTTP request failed
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),

    /// API returned an error response
    #[error("API error {status}: {message}")]
    ApiError { status: u16, message: String },

    /// Rate limit exceeded (429)
    #[error("rate limit exceeded, retry after {retry_after_ms}ms")]
    RateLimitExceeded { retry_after_ms: u64 },

    /// Request timeout
    #[error("request timeout after {timeout_ms}ms")]
    Timeout { timeout_ms: u64 },

    // ============ WebSocket Errors ============
    /// WebSocket connection failed
    #[error("WebSocket connection failed: {0}")]
    WebSocketConnectionError(String),

    /// WebSocket message parse error
    #[error("WebSocket message parse error: {0}")]
    WebSocketParseError(String),

    /// WebSocket subscription failed
    #[error("WebSocket subscription failed for channel: {channel}")]
    SubscriptionError { channel: String },

    /// WebSocket disconnected unexpectedly
    #[error("WebSocket disconnected: {reason}")]
    WebSocketDisconnected { reason: String },

    // ============ Chain Errors ============
    /// RPC call failed
    #[error("RPC call failed: {0}")]
    RpcError(String),

    /// Contract call failed
    #[error("contract call failed: {method}: {reason}")]
    ContractError { method: String, reason: String },

    /// Transaction not found
    #[error("transaction not found: {tx_hash}")]
    TransactionNotFound { tx_hash: String },

    // ============ Order Errors ============
    /// Invalid order parameters
    #[error("invalid order: {0}")]
    InvalidOrder(String),

    /// Order rejected by API
    #[error("order rejected: {reason}")]
    OrderRejected { reason: String },

    /// Order expired before submission
    #[error("order expired at {expiration}")]
    OrderExpired { expiration: u64 },

    /// Order not found
    #[error("order not found: {order_id}")]
    OrderNotFound { order_id: String },

    // ============ Confirmation Errors ============
    /// Order confirmation timeout
    #[error("confirmation timeout for order {order_id} after {timeout_ms}ms")]
    ConfirmationTimeout { order_id: String, timeout_ms: u64 },

    /// Position mismatch after confirmation
    #[error("position mismatch: expected {expected}, got {actual}")]
    PositionMismatch { expected: String, actual: String },

    // ============ Configuration Errors ============
    /// Invalid configuration
    #[error("invalid configuration: {0}")]
    ConfigError(String),

    /// Missing required configuration
    #[error("missing required config: {field}")]
    MissingConfig { field: String },

    // ============ Internal Errors ============
    /// Internal framework error (should not happen)
    #[error("internal error: {0}")]
    InternalError(String),

    /// Serialization/deserialization error
    #[error("serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    /// Channel send error (internal communication)
    #[error("channel send error: {0}")]
    ChannelError(String),
}

impl Error {
    /// Returns true if this error is retryable.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Error::RateLimitExceeded { .. }
                | Error::Timeout { .. }
                | Error::WebSocketDisconnected { .. }
                | Error::RpcError(_)
        )
    }

    /// Returns true if this is a rate limit error.
    pub fn is_rate_limited(&self) -> bool {
        matches!(self, Error::RateLimitExceeded { .. })
    }

    /// Returns the suggested retry delay in milliseconds, if applicable.
    pub fn retry_after_ms(&self) -> Option<u64> {
        match self {
            Error::RateLimitExceeded { retry_after_ms } => Some(*retry_after_ms),
            Error::Timeout { timeout_ms } => Some(*timeout_ms / 2), // Retry after half the timeout
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_retryable() {
        assert!(Error::RateLimitExceeded {
            retry_after_ms: 1000
        }
        .is_retryable());
        assert!(Error::Timeout { timeout_ms: 5000 }.is_retryable());
        assert!(Error::RpcError("connection reset".into()).is_retryable());
        assert!(!Error::InvalidOrder("bad price".into()).is_retryable());
    }

    #[test]
    fn test_error_rate_limited() {
        assert!(Error::RateLimitExceeded {
            retry_after_ms: 1000
        }
        .is_rate_limited());
        assert!(!Error::Timeout { timeout_ms: 5000 }.is_rate_limited());
    }

    #[test]
    fn test_retry_after() {
        assert_eq!(
            Error::RateLimitExceeded {
                retry_after_ms: 1000
            }
            .retry_after_ms(),
            Some(1000)
        );
        assert_eq!(
            Error::Timeout { timeout_ms: 5000 }.retry_after_ms(),
            Some(2500)
        );
        assert_eq!(Error::InvalidOrder("bad".into()).retry_after_ms(), None);
    }

    #[test]
    fn test_error_display() {
        let err = Error::ApiError {
            status: 400,
            message: "invalid signature".into(),
        };
        assert_eq!(err.to_string(), "API error 400: invalid signature");
    }
}
