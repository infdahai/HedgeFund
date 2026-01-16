//! Opinion Trade HFT Framework
//!
//! A high-frequency trading framework for the Opinion Trade prediction market
//! on BNB Chain. Designed for minimum tick-to-trade latency with reliable
//! order confirmation via on-chain verification.
//!
//! # Architecture
//!
//! The framework is organized into three planes:
//!
//! - **Data Plane**: WebSocket client, REST poller, Chain subscriber
//! - **Control Plane**: Strategy engine, Position manager, Risk manager
//! - **Execution Plane**: Rate limiter, Priority queue, Order gateway, Confirmation tracker
//!
//! # Critical Parameters
//!
//! - EIP-712 Domain: "OPINION CTF Exchange" (note the space!)
//! - Chain ID: 56 (BNB Chain)
//! - ConditionalTokens: 0xAD1a38cEc043e70E83a3eC30443dB285ED10D774
//! - API Rate Limit: 15 TPS
//! - Price Precision: 3 decimals (0.001 - 0.999)

pub mod auth;
pub mod chain;
pub mod config;
pub mod data;
pub mod error;
pub mod execution;
pub mod types;

pub use config::Config;
pub use error::{Error, Result};

/// Re-export commonly used types
pub mod prelude {
    pub use crate::auth::Eip712Signer;
    pub use crate::config::Config;
    pub use crate::error::{Error, Result};
    pub use crate::types::{Market, Order, OrderIntent, OrderPriority, OrderSide, Position};
}
