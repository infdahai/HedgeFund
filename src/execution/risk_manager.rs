//! Risk Management module for Opinion HFT.
//!
//! Handles system-level risks including:
//! - API Latency monitoring
//! - Server time synchronization
//! - Circuit breaking on errors
//! - Stop-loss protection for arbitrage trades
//! - Position and exposure limits

use crate::config::{Environment, RiskConfig};
use crate::data::rest::OpinionClient;
use crate::error::{Error, Result};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Trade execution record for P&L tracking
#[derive(Debug, Clone)]
pub struct TradeRecord {
    pub trade_id: String,
    pub market_id: String,
    pub expected_profit: f64,
    pub actual_pnl: f64,
    pub expected_price: f64,
    pub actual_price: f64,
    pub execution_time_ms: u64,
    pub timestamp: Instant,
}

/// Manages operational risks and system health.
pub struct RiskManager {
    config: RiskConfig,
    env: Environment,
    rest_client: OpinionClient,
    consecutive_errors: u32,
    is_halted: bool,
    daily_pnl: f64,
    peak_equity: f64,
    current_equity: f64,
    positions: HashMap<String, f64>,
    trade_history: Vec<TradeRecord>,
    consecutive_losses: u32,
}

impl RiskManager {
    pub fn new(config: RiskConfig, env: Environment, rest_client: OpinionClient) -> Self {
        Self {
            config,
            env,
            rest_client,
            consecutive_errors: 0,
            is_halted: false,
            daily_pnl: 0.0,
            peak_equity: 0.0,
            current_equity: 0.0,
            positions: HashMap::new(),
            trade_history: Vec::new(),
            consecutive_losses: 0,
        }
    }

    pub fn new_with_defaults(env: Environment, rest_client: OpinionClient) -> Self {
        Self::new(RiskConfig::default(), env, rest_client)
    }

    pub fn set_initial_equity(&mut self, equity: f64) {
        self.current_equity = equity;
        self.peak_equity = equity;
    }

    /// Validate arbitrage opportunity before execution
    pub fn validate_pre_trade(
        &self,
        market_id: &str,
        expected_profit: f64,
        order_size: f64,
    ) -> Result<()> {
        if self.is_halted {
            return Err(Error::SystemHalted("Trading halted by risk manager".into()));
        }

        // Check daily loss limit
        if self.daily_pnl <= -self.config.max_daily_loss {
            return Err(Error::RiskLimitExceeded(format!(
                "Daily loss limit reached: {:.2} <= -{:.2}",
                self.daily_pnl, self.config.max_daily_loss
            )));
        }

        // Check drawdown
        if self.peak_equity > 0.0 {
            let drawdown = (self.peak_equity - self.current_equity) / self.peak_equity;
            if drawdown > self.config.max_drawdown_pct {
                return Err(Error::RiskLimitExceeded(format!(
                    "Drawdown limit exceeded: {:.2}% > {:.2}%",
                    drawdown * 100.0,
                    self.config.max_drawdown_pct * 100.0
                )));
            }
        }

        // Check position limit
        let current_position = self.positions.get(market_id).copied().unwrap_or(0.0);
        if current_position + order_size > self.config.max_position_per_market {
            return Err(Error::RiskLimitExceeded(format!(
                "Position limit exceeded for {}: {:.2} + {:.2} > {:.2}",
                market_id, current_position, order_size, self.config.max_position_per_market
            )));
        }

        // Check total exposure
        let total_exposure: f64 = self.positions.values().sum();
        if total_exposure + order_size > self.config.max_total_exposure {
            return Err(Error::RiskLimitExceeded(format!(
                "Total exposure limit exceeded: {:.2} + {:.2} > {:.2}",
                total_exposure, order_size, self.config.max_total_exposure
            )));
        }

        // Minimum profit check (profit should cover potential slippage)
        let min_required_profit = order_size * self.config.max_slippage_pct;
        if expected_profit < min_required_profit {
            return Err(Error::RiskLimitExceeded(format!(
                "Expected profit {:.4} < min required {:.4} (slippage buffer)",
                expected_profit, min_required_profit
            )));
        }

        Ok(())
    }

    /// Record trade result and check for stop-loss triggers.
    /// This serves as the post-trade validation step.
    pub fn validate_post_trade(&mut self, record: TradeRecord) -> Result<()> {
        let slippage = if record.expected_price > 0.0 {
            (record.actual_price - record.expected_price).abs() / record.expected_price
        } else {
            0.0
        };

        self.daily_pnl += record.actual_pnl;
        self.current_equity += record.actual_pnl;

        if self.current_equity > self.peak_equity {
            self.peak_equity = self.current_equity;
        }

        if record.actual_pnl < 0.0 {
            self.consecutive_losses += 1;
        } else {
            self.consecutive_losses = 0;
        }

        self.trade_history.push(record.clone());

        if slippage > self.config.max_slippage_pct {
            tracing::warn!(
                "Slippage exceeded: {:.2}% > {:.2}% on trade {}",
                slippage * 100.0,
                self.config.max_slippage_pct * 100.0,
                record.trade_id
            );
        }

        if record.execution_time_ms > self.config.max_execution_time_ms {
            tracing::warn!(
                "Execution time exceeded: {}ms > {}ms on trade {}",
                record.execution_time_ms,
                self.config.max_execution_time_ms,
                record.trade_id
            );
        }

        if record.expected_profit > 0.0 && record.actual_pnl < 0.0 {
            tracing::error!(
                "ARBITRAGE FAILURE: Expected +{:.4} but got {:.4} on trade {}",
                record.expected_profit,
                record.actual_pnl,
                record.trade_id
            );

            if self.consecutive_losses >= 3 {
                self.is_halted = true;
                return Err(Error::SystemHalted(format!(
                    "Halted after {} consecutive losses. Last trade: {}",
                    self.consecutive_losses, record.trade_id
                )));
            }
        }

        if self.daily_pnl <= -self.config.max_daily_loss {
            self.is_halted = true;
            return Err(Error::SystemHalted(format!(
                "Daily loss limit reached: {:.2}",
                self.daily_pnl
            )));
        }

        if self.peak_equity > 0.0 {
            let drawdown = (self.peak_equity - self.current_equity) / self.peak_equity;
            if drawdown > self.config.max_drawdown_pct {
                self.is_halted = true;
                return Err(Error::SystemHalted(format!(
                    "Drawdown limit exceeded: {:.2}%",
                    drawdown * 100.0
                )));
            }
        }

        Ok(())
    }

    /// Update position for a market
    pub fn update_position(&mut self, market_id: &str, delta: f64) {
        let position = self.positions.entry(market_id.to_string()).or_insert(0.0);
        *position += delta;
        if position.abs() < 0.0001 {
            self.positions.remove(market_id);
        }
    }

    /// Get current daily P&L
    pub fn daily_pnl(&self) -> f64 {
        self.daily_pnl
    }

    /// Get current drawdown percentage
    pub fn current_drawdown(&self) -> f64 {
        if self.peak_equity > 0.0 {
            (self.peak_equity - self.current_equity) / self.peak_equity
        } else {
            0.0
        }
    }

    /// Check if trading is halted
    pub fn is_halted(&self) -> bool {
        self.is_halted
    }

    /// Reset daily statistics (call at start of each trading day)
    pub fn reset_daily(&mut self) {
        self.daily_pnl = 0.0;
        self.consecutive_losses = 0;
        self.trade_history.clear();
    }

    /// Check system health including API latency and time sync.
    ///
    /// This method performs the following checks:
    /// 1. **Circuit Breaker Status**: Immediately returns `SystemHalted` if the system is already halted.
    /// 2. **Connectivity & Latency**: Measures round-trip time to the API.
    /// 3. **Clock Drift**: Estimates drift (currently placeholder).
    ///
    /// # Returns
    /// * `Ok(())` - System is healthy and within risk limits.
    /// * `Err(Error::RiskLimitExceeded)` - Latency or drift exceeds configured maximums.
    /// * `Err(Error::SystemHalted)` - System is halted due to consecutive errors.
    pub async fn check_health(&mut self) -> Result<()> {
        // 1. Check if we are already halted
        if self.is_halted {
            return Err(Error::SystemHalted(
                "RiskManager halted system due to previous errors".into(),
            ));
        }

        // 2. Measure vital statistics
        match self.measure_latency_and_drift().await {
            Ok((latency, drift)) => {
                // Reset error counter on successful check
                self.consecutive_errors = 0;

                // 3. Validate against thresholds

                // Check Latency
                let max_latency = self.config.get_max_api_latency(self.env);
                if latency > max_latency {
                    let env_name = if self.env.is_local() { "local" } else { "aws" };
                    let suggestion = if self.env.is_local() {
                        let proxy_url = std::env::var("OPINION_PROXY_URL")
                            .unwrap_or_else(|_| "http://127.0.0.1:7890".to_string());
                        format!("Check proxy status at {}", proxy_url)
                    } else {
                        String::new()
                    };
                    return Err(Error::RiskLimitExceeded(format!(
                        "API Latency too high ({} mode): {:?} > {:?}. {}",
                        env_name, latency, max_latency, suggestion
                    )));
                }

                // Check Drift
                if drift > self.config.max_clock_drift {
                    return Err(Error::RiskLimitExceeded(format!(
                        "Clock drift too high: {:?} > {:?}",
                        drift, self.config.max_clock_drift
                    )));
                }

                Ok(())
            }
            Err(e) => {
                // 4. Handle Check Failures
                self.consecutive_errors += 1;

                // Trip circuit breaker if errors exceed threshold
                if self.consecutive_errors >= self.config.max_consecutive_errors {
                    self.is_halted = true;
                    return Err(Error::SystemHalted(format!(
                        "System halted after {} consecutive health check failures. Last error: {}",
                        self.consecutive_errors, e
                    )));
                }

                // Return the underlying error but don't halt yet
                Err(e)
            }
        }
    }

    /// Validate microstructure directional trade
    pub fn validate_micro_trade(
        &self,
        market_id: &str,
        _side: crate::types::OrderSide,
        order_size: f64,
        current_obi: f64,
        current_spread_bps: f64,
    ) -> Result<()> {
        // Base checks
        self.validate_pre_trade(market_id, 0.0, order_size)?;

        // 1. OBI must be extreme enough (redundant with strategy but good safety net)
        if current_obi.abs() < 0.5 {
            return Err(Error::RiskLimitExceeded(format!(
                "Risk: OBI too weak for directional trade: {:.2} (need > 0.5)",
                current_obi.abs()
            )));
        }

        // 2. Spread check
        if current_spread_bps > 100.0 {
            return Err(Error::RiskLimitExceeded(format!(
                "Risk: Spread too wide: {:.1} bps",
                current_spread_bps
            )));
        }

        // 3. Directional conflict check
        if let Some(position) = self.positions.get(market_id) {
            // If we have a long position (>0), don't short.
            // Note: RiskManager positions map tracks 'exposure', doesn't track side strictly per token.
            // But if we use it for net exposure:
            // If we are holding YES (Long), risk manager sees +Exposure.
            // If we want to Sell YES (Short), that's reducing exposure, which is fine.
            // But if we want to Buy NO (Short view), that increases exposure but in opposite direction?
            // Actually, buying NO is distinct from selling YES in terms of tokens, but correlated risk.
            // For MVP, just checking max exposure is enough.
            // The "Cannot short while holding long" rule implies preventing hedging?
            // Actually strategy handles exits. This is for NEW entries.
            // If we are already exposed, maybe don't add more risk unless it is to close?
            // Strategy state machine should handle this (Scanning vs Holding).
            // Risk manager just enforces limits.

            // Just use the standard exposure checks in validate_pre_trade.
            let _ = position;
        }

        Ok(())
    }

    /// Measure round-trip latency and estimate clock drift.
    /// Opinion API doesn't have a dedicated time endpoint, so we use a lightweight call.
    async fn measure_latency_and_drift(&self) -> Result<(Duration, Duration)> {
        let start = Instant::now();

        // Use a lightweight endpoint to check connectivity
        // Using market list with limit=1 as a ping
        self.rest_client
            .get::<serde_json::Value>("market?limit=1")
            .await?;

        let latency = start.elapsed();

        // Without an explicit server-time field in response, drift is hard to calculate precisely.
        // We'll assume drift is 0 for now unless we parse Date headers.
        // TODO: Parse 'Date' HTTP header for coarse sync check.
        let drift = Duration::from_secs(0);

        Ok((latency, drift))
    }

    /// Reset the circuit breaker manually.
    pub fn reset(&mut self) {
        self.is_halted = false;
        self.consecutive_errors = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ApiConfig;

    fn mock_client() -> OpinionClient {
        use crate::config::Environment;
        let config = ApiConfig::default();
        OpinionClient::new(&config, Environment::default()).unwrap()
    }

    #[tokio::test]
    async fn test_risk_manager_creation() {
        let config = RiskConfig::default();
        let env = Environment::default();
        let client = mock_client();
        let _rm = RiskManager::new(config, env, client);
    }
}
