use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MicroConfig {
    // === Entry thresholds ===
    pub enter_obi_threshold: f64, // OBI > 0.7 triggers signal
    pub enter_ofi_threshold: f64, // OFI > 0 confirms trend
    pub max_spread_bps: f64,      // Max spread 50 bps
    pub min_liquidity: f64,       // Min liquidity 100 shares

    // === Exit thresholds ===
    pub stop_loss_pct: f64,            // Stop loss 0.5%
    pub take_profit_pct: f64,          // Take profit 1.5%
    pub obi_reversal_threshold: f64,   // OBI reversal threshold -0.3
    pub obi_exhaustion_threshold: f64, // OBI exhaustion threshold 0.1

    // === Time limits ===
    pub max_holding_time: Duration,  // Max holding time 60s
    pub signal_confirmation_ms: u64, // Signal confirmation delay 100ms

    // === Position limits ===
    pub max_position_per_market: f64, // Max position per market $50
    pub min_order_notional: f64,      // Min order notional $1.30
    pub max_order_notional: f64,      // Max order notional $20

    // === Cooldown & Blacklist ===
    pub cooldown_after_loss: Duration, // Cooldown 60s after loss
    pub blacklist_duration: Duration,  // Blacklist 1h
    pub max_consecutive_losses: u32,   // Max consecutive losses 2

    // === Fees ===
    pub topic_fee_rate: f64,    // 2%
    pub referral_discount: f64, // 10%
    pub min_fee: f64,           // $0.50
}

impl Default for MicroConfig {
    fn default() -> Self {
        Self {
            // Entry
            enter_obi_threshold: 0.7,
            enter_ofi_threshold: 0.0,
            max_spread_bps: 50.0,
            min_liquidity: 100.0,

            // Exit
            stop_loss_pct: 0.005,   // 0.5%
            take_profit_pct: 0.015, // 1.5%
            obi_reversal_threshold: -0.3,
            obi_exhaustion_threshold: 0.1,

            // Time
            max_holding_time: Duration::from_secs(60),
            signal_confirmation_ms: 100,

            // Position
            max_position_per_market: 50.0,
            min_order_notional: 1.30,
            max_order_notional: 20.0,

            // Cooldown
            cooldown_after_loss: Duration::from_secs(60),
            blacklist_duration: Duration::from_secs(3600),
            max_consecutive_losses: 2,

            // Fees
            topic_fee_rate: 0.02,
            referral_discount: 0.10,
            min_fee: 0.50,
        }
    }
}

impl MicroConfig {
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(val) = std::env::var("OPINION_MICRO_OBI_THRESHOLD") {
            if let Ok(v) = val.parse() {
                config.enter_obi_threshold = v;
            }
        }
        if let Ok(val) = std::env::var("OPINION_MICRO_MAX_SPREAD_BPS") {
            if let Ok(v) = val.parse() {
                config.max_spread_bps = v;
            }
        }
        if let Ok(val) = std::env::var("OPINION_MICRO_STOP_LOSS_PCT") {
            if let Ok(v) = val.parse() {
                config.stop_loss_pct = v;
            }
        }
        if let Ok(val) = std::env::var("OPINION_MICRO_TAKE_PROFIT_PCT") {
            if let Ok(v) = val.parse() {
                config.take_profit_pct = v;
            }
        }
        if let Ok(val) = std::env::var("OPINION_MICRO_MAX_HOLDING_SECS") {
            if let Ok(v) = val.parse() {
                config.max_holding_time = Duration::from_secs(v);
            }
        }
        if let Ok(val) = std::env::var("OPINION_MICRO_MAX_ORDER_NOTIONAL") {
            if let Ok(v) = val.parse() {
                config.max_order_notional = v;
            }
        }

        config
    }
}
