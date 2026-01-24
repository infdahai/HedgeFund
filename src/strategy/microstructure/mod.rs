use anyhow::Result;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tracing::{debug, info, warn};

use crate::strategy::core::{MarketOrderbook, TradeSide};
use crate::types::{OrderIntent, OrderPriority, OrderSide};
use alloy_primitives::U256;

mod config;
mod position;

use crate::strategy::core::microstructure::MicrostructureFeature;
pub use config::MicroConfig;
pub use position::MicroPosition;

// We need to reference OrderGateway from execution, but prevent cyclic dependency issues if any.
// Since this is inside `impl`, it is fine.
// But we need to import the type.
use crate::execution::gateway::OrderGateway;

/// Monotonic counter for generating unique paper order IDs
static PAPER_ORDER_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrategyState {
    Scanning,       // Monitoring markets
    SignalDetected, // OBI threshold crossed
    EntryPending,   // Order submitted
    Holding,        // Position active
    ExitPending,    // Exit submitted
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitReason {
    StopLoss,
    TakeProfit,
    ObiReversal,
    ObiExhaustion,
    TimeStop,
    Manual,
}

#[derive(Debug, Clone)]
pub struct PendingSignal {
    pub market_id: String,
    pub pair: String,
    pub side: TradeSide,
    pub trigger_time: Instant,
    pub trigger_obi: f64,
    pub trigger_price: f64,
}

pub struct MicroStrategy {
    pub config: MicroConfig,
    pub state: StrategyState,
    pub positions: HashMap<String, MicroPosition>, // Keyed by pair (e.g. "123-YES")
    pub feature_cache: HashMap<String, MicrostructureFeature>,
    pub cooldown_until: HashMap<String, Instant>,
    pub blacklist: HashMap<String, Instant>,
    pub consecutive_losses: HashMap<String, u32>,
    pub pending_signals: HashMap<String, PendingSignal>, // Keyed by pair
    pub paper_mode: bool,
    pub token_map: HashMap<String, String>, // pair -> token_id_str
    // Paper trading metrics
    pub total_trades: u32,
    pub winning_trades: u32,
    pub total_pnl: f64,
    pub total_notional: f64,
}

impl MicroStrategy {
    pub fn new(config: MicroConfig, paper_mode: bool) -> Self {
        Self {
            config,
            state: StrategyState::Scanning,
            positions: HashMap::new(),
            feature_cache: HashMap::new(),
            cooldown_until: HashMap::new(),
            blacklist: HashMap::new(),
            consecutive_losses: HashMap::new(),
            pending_signals: HashMap::new(),
            paper_mode,
            token_map: HashMap::new(),
            total_trades: 0,
            winning_trades: 0,
            total_pnl: 0.0,
            total_notional: 0.0,
        }
    }

    pub fn set_token_map(&mut self, map: HashMap<String, String>) {
        self.token_map = map;
    }

    /// Returns paper trading summary (total_trades, winning_trades, total_pnl, win_rate, return_pct)
    pub fn paper_summary(&self) -> (u32, u32, f64, f64, f64) {
        let win_rate = if self.total_trades > 0 {
            self.winning_trades as f64 / self.total_trades as f64
        } else {
            0.0
        };
        let return_pct = if self.total_notional > 0.0 {
            self.total_pnl / self.total_notional
        } else {
            0.0
        };
        (
            self.total_trades,
            self.winning_trades,
            self.total_pnl,
            win_rate,
            return_pct,
        )
    }

    /// Called by ConfirmationTracker when an exit order is confirmed filled.
    /// Removes the position and updates loss tracking.
    pub fn confirm_exit(&mut self, pair: &str, actual_pnl: f64) {
        if let Some(position) = self.positions.remove(pair) {
            info!(
                "Exit confirmed: {} pnl={:.4} return={:.2}%",
                pair,
                actual_pnl,
                position.return_pct() * 100.0
            );

            if actual_pnl < 0.0 {
                let count = self
                    .consecutive_losses
                    .entry(position.market_id.clone())
                    .or_insert(0);
                *count += 1;

                self.cooldown_until.insert(
                    position.market_id.clone(),
                    Instant::now() + self.config.cooldown_after_loss,
                );

                if *count >= self.config.max_consecutive_losses {
                    self.blacklist.insert(
                        position.market_id.clone(),
                        Instant::now() + self.config.blacklist_duration,
                    );
                    warn!(
                        "Market {} blacklisted after confirmed loss",
                        position.market_id
                    );
                }
            } else {
                self.consecutive_losses.remove(&position.market_id);
            }
        }
    }

    pub async fn on_orderbook_update(
        &mut self,
        market_id: &str,
        pair: &str,
        ob: &MarketOrderbook,
        gateway: Option<&OrderGateway>,
    ) -> Result<()> {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // 1. Calculate features
        let prev = self.feature_cache.get(pair);
        let feature = match MicrostructureFeature::from_orderbook(market_id, pair, ob, prev, now_ms)
        {
            Some(f) => f,
            None => return Ok(()),
        };

        let now = Instant::now();

        // 2. Check blacklist/cooldown
        if self.is_blacklisted(market_id, now) || self.is_cooling_down(market_id, now) {
            self.feature_cache.insert(pair.to_string(), feature);
            return Ok(());
        }

        // 3. State Machine
        // Note: This logic processes one market/pair at a time.
        // State is derived from `positions` and `pending_signals` per pair.
        // If we have a position for this pair -> Holding.
        // If we have a pending signal for this pair -> SignalDetected.
        // Otherwise -> Scanning.

        let current_state = self.get_state(pair);

        match current_state {
            StrategyState::Scanning => {
                self.handle_scanning(&feature).await?;
            }
            StrategyState::SignalDetected => {
                self.handle_signal_detected(&feature, gateway).await?;
            }
            StrategyState::EntryPending => {
                // Waiting for fill. In paper mode, we assume fill immediately in next step/loop or logic.
                // For live, we wait for ConfirmationTracker.
            }
            StrategyState::Holding => {
                self.handle_holding(&feature, gateway).await?;
            }
            StrategyState::ExitPending => {
                // Waiting for exit fill.
            }
        }

        self.feature_cache.insert(pair.to_string(), feature);
        Ok(())
    }

    fn get_state(&self, pair: &str) -> StrategyState {
        if let Some(pos) = self.positions.get(pair) {
            if pos.exit_pending {
                return StrategyState::ExitPending;
            }
            return StrategyState::Holding;
        }
        if self.pending_signals.contains_key(pair) {
            return StrategyState::SignalDetected;
        }
        StrategyState::Scanning
    }

    async fn handle_scanning(&mut self, feature: &MicrostructureFeature) -> Result<()> {
        // Simple entry check
        if self.is_entry_signal(feature) {
            let side = if feature.obi_l1 > 0.0 {
                TradeSide::Buy
            } else {
                TradeSide::Sell
            };

            let signal = PendingSignal {
                market_id: feature.market_id.clone(),
                pair: feature.pair.clone(),
                side,
                trigger_time: Instant::now(),
                trigger_obi: feature.obi_l1,
                trigger_price: feature.mid_price,
            };

            self.pending_signals.insert(feature.pair.clone(), signal);

            info!(
                "Signal detected: {} OBI={:.3} side={:?} price={:.3}",
                feature.pair, feature.obi_l1, side, feature.mid_price
            );
        }
        Ok(())
    }

    async fn handle_signal_detected(
        &mut self,
        feature: &MicrostructureFeature,
        gateway: Option<&OrderGateway>,
    ) -> Result<()> {
        let signal = match self.pending_signals.get(&feature.pair) {
            Some(s) => s.clone(),
            None => return Ok(()), // No pending signal for this pair
        };

        // Confirmation delay
        if (signal.trigger_time.elapsed().as_millis() as u64) < self.config.signal_confirmation_ms {
            return Ok(());
        }

        // Validate signal persistence
        let valid = match signal.side {
            TradeSide::Buy => feature.obi_l1 > self.config.enter_obi_threshold * 0.8,
            TradeSide::Sell => feature.obi_l1 < -self.config.enter_obi_threshold * 0.8,
        };

        if !valid {
            info!(
                "Signal invalidated: {} OBI dropped to {:.3}",
                feature.pair, feature.obi_l1
            );
            self.pending_signals.remove(&feature.pair);
            return Ok(());
        }

        if feature.spread_bps > self.config.max_spread_bps {
            debug!(
                "Signal rejected: spread too wide {:.1} bps",
                feature.spread_bps
            );
            self.pending_signals.remove(&feature.pair);
            return Ok(());
        }

        // Calculate size
        let order_size = self.calculate_order_size(feature);
        if order_size < self.config.min_order_notional {
            debug!("Signal rejected: size {:.2} < min", order_size);
            self.pending_signals.remove(&feature.pair);
            return Ok(());
        }

        // Execute
        let entry_price = match signal.side {
            TradeSide::Buy => feature.best_ask,
            TradeSide::Sell => feature.best_bid,
        };
        let quantity = order_size / entry_price;

        if self.paper_mode {
            let order_id = format!(
                "paper-{}",
                PAPER_ORDER_COUNTER.fetch_add(1, Ordering::Relaxed)
            );
            info!(
                "[PAPER] Entry: {} {:?} qty={:.2} price={:.3} notional={:.2}",
                feature.pair, signal.side, quantity, entry_price, order_size
            );
            let pos = MicroPosition::new(
                signal.market_id.clone(),
                signal.pair.clone(),
                signal.side,
                entry_price,
                signal.trigger_obi,
                quantity,
                order_id,
            );
            self.positions.insert(feature.pair.clone(), pos);
            self.pending_signals.remove(&feature.pair);
        } else if let Some(gw) = gateway {
            if let Some(token_id_str) = self.token_map.get(&feature.pair) {
                use std::str::FromStr;
                let token_id = U256::from_str(token_id_str).unwrap_or(U256::ZERO);

                let order_side = match signal.side {
                    TradeSide::Buy => OrderSide::Buy,
                    TradeSide::Sell => OrderSide::Sell,
                };

                let quantity_wei = match Self::quantity_to_wei(quantity) {
                    Some(wei) => wei,
                    None => {
                        warn!("Invalid quantity for entry: {}", quantity);
                        self.pending_signals.remove(&feature.pair);
                        return Ok(());
                    }
                };

                let intent = OrderIntent::new(
                    feature.market_id.clone(),
                    token_id,
                    order_side,
                    entry_price,
                    quantity_wei,
                )
                .with_priority(OrderPriority::High);

                match gw.submit_order(intent, None).await {
                    Ok(id) => {
                        info!("Submitted entry order: {}", id);
                        let pos = MicroPosition::new(
                            signal.market_id.clone(),
                            signal.pair.clone(),
                            signal.side,
                            entry_price,
                            signal.trigger_obi,
                            quantity,
                            id,
                        );
                        self.positions.insert(feature.pair.clone(), pos);
                    }
                    Err(e) => warn!("Entry order failed: {}", e),
                }
            } else {
                warn!("Token ID not found for pair {}", feature.pair);
            }
            self.pending_signals.remove(&feature.pair);
        }

        Ok(())
    }

    async fn handle_holding(
        &mut self,
        feature: &MicrostructureFeature,
        gateway: Option<&OrderGateway>,
    ) -> Result<()> {
        let position = match self.positions.get_mut(&feature.pair) {
            Some(p) => p,
            None => return Ok(()),
        };

        position.update_price(feature.mid_price);

        let exit_reason = Self::check_exit_conditions(&self.config, position, feature);

        if let Some(reason) = exit_reason {
            if self.paper_mode {
                // Track metrics before removing position
                self.total_trades += 1;
                self.total_pnl += position.current_pnl;
                self.total_notional += position.notional;
                if position.current_pnl > 0.0 {
                    self.winning_trades += 1;
                }

                info!(
                    "[PAPER] Exit: {} reason={:?} pnl={:.4} return={:.2}% holding={:.1}s",
                    feature.pair,
                    reason,
                    position.current_pnl,
                    position.return_pct() * 100.0,
                    position.holding_duration().as_secs_f64()
                );

                if position.current_pnl < 0.0 {
                    let count = self
                        .consecutive_losses
                        .entry(position.market_id.clone())
                        .or_insert(0);
                    *count += 1;

                    self.cooldown_until.insert(
                        position.market_id.clone(),
                        Instant::now() + self.config.cooldown_after_loss,
                    );

                    if *count >= self.config.max_consecutive_losses {
                        self.blacklist.insert(
                            position.market_id.clone(),
                            Instant::now() + self.config.blacklist_duration,
                        );
                        warn!("Market {} blacklisted", position.market_id);
                    }
                } else {
                    self.consecutive_losses.remove(&position.market_id);
                }

                self.positions.remove(&feature.pair);
            } else {
                // Submit exit order (live mode)
                // Skip if already exit pending
                if position.exit_pending {
                    debug!("Exit already pending for {}", feature.pair);
                    return Ok(());
                }

                if let Some(gw) = gateway {
                    if let Some(token_id_str) = self.token_map.get(&feature.pair) {
                        use std::str::FromStr;
                        let token_id = U256::from_str(token_id_str).unwrap_or(U256::ZERO);

                        // Exit side is opposite of entry
                        // If Long (Buy YES), we Sell YES to close.
                        // If Short (Sell YES), we Buy YES to close.
                        let exit_side = match position.side {
                            TradeSide::Buy => OrderSide::Sell,
                            TradeSide::Sell => OrderSide::Buy,
                        };

                        let exit_price = match position.side {
                            TradeSide::Buy => feature.best_bid,  // Sell into bid
                            TradeSide::Sell => feature.best_ask, // Buy from ask
                        };

                        // Sanity check price
                        if exit_price <= 0.0 {
                            warn!("Cannot exit {} at 0.0 price", feature.pair);
                            return Ok(());
                        }

                        let quantity_wei = match Self::quantity_to_wei(position.quantity) {
                            Some(wei) => wei,
                            None => {
                                warn!("Invalid quantity for exit: {}", position.quantity);
                                return Ok(());
                            }
                        };

                        let intent = OrderIntent::new(
                            feature.market_id.clone(),
                            token_id,
                            exit_side,
                            exit_price,
                            quantity_wei,
                        )
                        .with_priority(OrderPriority::Critical); // Exits are critical

                        match gw.submit_order(intent, None).await {
                            Ok(id) => {
                                info!("Submitted exit order: {} reason={:?}", id, reason);
                                // Mark as exit pending - DO NOT remove position yet
                                // Wait for ConfirmationTracker to confirm fill
                                position.exit_pending = true;
                                position.exit_order_id = Some(id);
                            }
                            Err(e) => warn!("Exit order failed: {}", e),
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn check_exit_conditions(
        config: &MicroConfig,
        position: &MicroPosition,
        feature: &MicrostructureFeature,
    ) -> Option<ExitReason> {
        let ret = position.return_pct();

        if ret <= -config.stop_loss_pct {
            return Some(ExitReason::StopLoss);
        }
        if ret >= config.take_profit_pct {
            return Some(ExitReason::TakeProfit);
        }

        let obi_reversed = match position.side {
            TradeSide::Buy => feature.obi_l1 < config.obi_reversal_threshold,
            TradeSide::Sell => feature.obi_l1 > -config.obi_reversal_threshold,
        };
        if obi_reversed {
            return Some(ExitReason::ObiReversal);
        }

        if position.holding_duration() > config.max_holding_time {
            return Some(ExitReason::TimeStop);
        }

        None
    }

    fn is_entry_signal(&self, feature: &MicrostructureFeature) -> bool {
        let obi_strong = feature.obi_l1.abs() > self.config.enter_obi_threshold;
        let spread_ok = feature.spread_bps < self.config.max_spread_bps;
        let liquidity_ok = feature.total_liquidity > self.config.min_liquidity;

        // OFI confirmation: OFI EMA should align with OBI direction
        // If OBI is positive (buy pressure), OFI should also be positive
        // If OBI is negative (sell pressure), OFI should also be negative
        let ofi_ok = if self.config.enter_ofi_threshold > 0.0 {
            (feature.obi_l1 > 0.0 && feature.ofi_ema > self.config.enter_ofi_threshold)
                || (feature.obi_l1 < 0.0 && feature.ofi_ema < -self.config.enter_ofi_threshold)
        } else {
            // If threshold is 0, skip OFI check
            true
        };

        obi_strong && spread_ok && liquidity_ok && ofi_ok
    }

    fn calculate_order_size(&self, feature: &MicrostructureFeature) -> f64 {
        let available = feature.total_liquidity * 0.1;
        available.clamp(
            self.config.min_order_notional,
            self.config.max_order_notional,
        )
    }

    fn is_blacklisted(&self, market_id: &str, now: Instant) -> bool {
        self.blacklist.get(market_id).is_some_and(|t| now < *t)
    }

    fn is_cooling_down(&self, market_id: &str, now: Instant) -> bool {
        self.cooldown_until.get(market_id).is_some_and(|t| now < *t)
    }

    /// Convert quantity to wei (18 decimals) as U256 with safety checks
    fn quantity_to_wei(quantity: f64) -> Option<alloy_primitives::U256> {
        if !quantity.is_finite() || quantity <= 0.0 {
            return None;
        }
        // Scale to 18 decimals
        let wei_f64 = quantity * 1e18;
        if !wei_f64.is_finite() || wei_f64 > u128::MAX as f64 {
            return None;
        }
        // Round to nearest integer
        let wei_rounded = wei_f64.round();
        if wei_rounded <= 0.0 {
            return None;
        }
        Some(alloy_primitives::U256::from(wei_rounded as u128))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_feature(obi_l1: f64, spread_bps: f64, total_liquidity: f64) -> MicrostructureFeature {
        MicrostructureFeature {
            market_id: "test".to_string(),
            pair: "test-YES".to_string(),
            timestamp_ms: 0,
            mid_price: 0.50,
            best_bid: 0.49,
            best_ask: 0.51,
            spread_abs: 0.02,
            spread_bps,
            bid_depth_l1: 100.0,
            ask_depth_l1: 100.0,
            obi_l1,
            bid_depth_l5: 500.0,
            ask_depth_l5: 500.0,
            obi_l5: obi_l1,
            ofi: 0.0,
            ofi_ema: 0.0,
            depth_ratio: 1.0,
            total_liquidity,
            next_price_10s: None,
            next_price_30s: None,
            price_change_10s: None,
        }
    }

    #[test]
    fn test_state_scanning_default() {
        let config = MicroConfig::default();
        let strategy = MicroStrategy::new(config, true);
        assert_eq!(strategy.get_state("any-pair"), StrategyState::Scanning);
    }

    #[test]
    fn test_state_holding_with_position() {
        let config = MicroConfig::default();
        let mut strategy = MicroStrategy::new(config, true);

        let pos = MicroPosition::new(
            "market1".to_string(),
            "market1-YES".to_string(),
            TradeSide::Buy,
            0.50,
            0.75,
            100.0,
            "order-1".to_string(),
        );
        strategy.positions.insert("market1-YES".to_string(), pos);

        assert_eq!(strategy.get_state("market1-YES"), StrategyState::Holding);
    }

    #[test]
    fn test_state_exit_pending() {
        let config = MicroConfig::default();
        let mut strategy = MicroStrategy::new(config, true);

        let mut pos = MicroPosition::new(
            "market1".to_string(),
            "market1-YES".to_string(),
            TradeSide::Buy,
            0.50,
            0.75,
            100.0,
            "order-1".to_string(),
        );
        pos.exit_pending = true;
        pos.exit_order_id = Some("exit-1".to_string());
        strategy.positions.insert("market1-YES".to_string(), pos);

        assert_eq!(
            strategy.get_state("market1-YES"),
            StrategyState::ExitPending
        );
    }

    #[test]
    fn test_is_entry_signal_strong_obi() {
        let config = MicroConfig {
            enter_obi_threshold: 0.7,
            max_spread_bps: 50.0,
            min_liquidity: 10.0,
            ..Default::default()
        };

        let strategy = MicroStrategy::new(config, true);

        // Strong OBI, good spread, good liquidity
        let feature = make_feature(0.8, 20.0, 200.0);
        assert!(strategy.is_entry_signal(&feature));

        // Weak OBI
        let feature = make_feature(0.5, 20.0, 200.0);
        assert!(!strategy.is_entry_signal(&feature));

        // Spread too wide
        let feature = make_feature(0.8, 100.0, 200.0);
        assert!(!strategy.is_entry_signal(&feature));

        // Low liquidity
        let feature = make_feature(0.8, 20.0, 5.0);
        assert!(!strategy.is_entry_signal(&feature));
    }

    #[test]
    fn test_exit_stop_loss() {
        let config = MicroConfig::default();
        let mut pos = MicroPosition::new(
            "m1".to_string(),
            "m1-YES".to_string(),
            TradeSide::Buy,
            0.50,
            0.75,
            100.0,
            "o1".to_string(),
        );
        // Price dropped below stop loss
        pos.update_price(0.49); // -2% from 0.50
        pos.current_pnl = -2.0; // Loss

        let feature = make_feature(0.5, 20.0, 200.0);

        let exit = MicroStrategy::check_exit_conditions(&config, &pos, &feature);
        assert_eq!(exit, Some(ExitReason::StopLoss));
    }

    #[test]
    fn test_exit_take_profit() {
        let config = MicroConfig::default();
        let mut pos = MicroPosition::new(
            "m1".to_string(),
            "m1-YES".to_string(),
            TradeSide::Buy,
            0.50,
            0.75,
            100.0,
            "o1".to_string(),
        );
        // Price increased above take profit
        pos.update_price(0.52); // +4% from 0.50
        pos.current_pnl = 2.0; // Profit
                               // notional = 50, pnl = 2, return = 4%

        let feature = make_feature(0.8, 20.0, 200.0);

        let exit = MicroStrategy::check_exit_conditions(&config, &pos, &feature);
        assert_eq!(exit, Some(ExitReason::TakeProfit));
    }

    #[test]
    fn test_exit_obi_reversal() {
        let config = MicroConfig::default();
        let pos = MicroPosition::new(
            "m1".to_string(),
            "m1-YES".to_string(),
            TradeSide::Buy,
            0.50,
            0.75,
            100.0,
            "o1".to_string(),
        );

        // OBI reversed to negative (below -0.3 threshold)
        let feature = make_feature(-0.5, 20.0, 200.0);

        let exit = MicroStrategy::check_exit_conditions(&config, &pos, &feature);
        assert_eq!(exit, Some(ExitReason::ObiReversal));
    }

    #[test]
    fn test_quantity_to_wei_valid() {
        let wei = MicroStrategy::quantity_to_wei(1.0).unwrap();
        assert_eq!(wei.to_string(), "1000000000000000000"); // 1e18
    }

    #[test]
    fn test_quantity_to_wei_small() {
        let wei = MicroStrategy::quantity_to_wei(0.001).unwrap();
        assert_eq!(wei.to_string(), "1000000000000000"); // 1e15
    }

    #[test]
    fn test_quantity_to_wei_invalid() {
        assert!(MicroStrategy::quantity_to_wei(0.0).is_none());
        assert!(MicroStrategy::quantity_to_wei(-1.0).is_none());
        assert!(MicroStrategy::quantity_to_wei(f64::NAN).is_none());
        assert!(MicroStrategy::quantity_to_wei(f64::INFINITY).is_none());
    }

    #[test]
    fn test_confirm_exit_removes_position() {
        let config = MicroConfig::default();
        let mut strategy = MicroStrategy::new(config, true);

        let pos = MicroPosition::new(
            "m1".to_string(),
            "m1-YES".to_string(),
            TradeSide::Buy,
            0.50,
            0.75,
            100.0,
            "o1".to_string(),
        );
        strategy.positions.insert("m1-YES".to_string(), pos);

        assert!(strategy.positions.contains_key("m1-YES"));
        strategy.confirm_exit("m1-YES", 1.0); // Profitable exit
        assert!(!strategy.positions.contains_key("m1-YES"));
    }

    #[test]
    fn test_confirm_exit_loss_triggers_cooldown() {
        let config = MicroConfig::default();
        let mut strategy = MicroStrategy::new(config, true);

        let pos = MicroPosition::new(
            "m1".to_string(),
            "m1-YES".to_string(),
            TradeSide::Buy,
            0.50,
            0.75,
            100.0,
            "o1".to_string(),
        );
        strategy.positions.insert("m1-YES".to_string(), pos);

        strategy.confirm_exit("m1-YES", -1.0); // Loss

        assert!(strategy.cooldown_until.contains_key("m1"));
        assert_eq!(strategy.consecutive_losses.get("m1"), Some(&1));
    }

    #[test]
    fn test_paper_order_id_uniqueness() {
        let id1 = PAPER_ORDER_COUNTER.fetch_add(1, Ordering::Relaxed);
        let id2 = PAPER_ORDER_COUNTER.fetch_add(1, Ordering::Relaxed);
        assert_ne!(id1, id2);
    }
}
