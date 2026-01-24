use anyhow::Result;
use std::collections::HashMap;
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
    pub pending_signal: Option<PendingSignal>,
    pub paper_mode: bool,
    pub token_map: HashMap<String, String>, // pair -> token_id_str
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
            pending_signal: None,
            paper_mode,
            token_map: HashMap::new(),
        }
    }

    pub fn set_token_map(&mut self, map: HashMap<String, String>) {
        self.token_map = map;
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
        // Note: This logic assumes we process one market/pair at a time in this instance.
        // If MicroStrategy is global, 'state' needs to be per-pair or we only trade one at a time.
        // For HFT, usually we track state per instrument.
        // The architecture implies a single 'state' field in MicroStrategy struct,
        // which suggests it manages ONE active trade sequence or it needs a map of states.
        // Architecture doc 14.4.1 defines StrategyState enum but 14.4.2 uses it as `self.state`.
        // However, `positions` is a HashMap. This implies we can hold multiple positions.
        // But `pending_signal` is Option<PendingSignal>.
        // To support multi-market, we should probably have state per pair.
        // For MVP/Paper Phase 1, let's assume we manage state implicitly via `positions` existence
        // and `pending_signal`.
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
        if self.positions.contains_key(pair) {
            return StrategyState::Holding;
        }
        if let Some(signal) = &self.pending_signal {
            if signal.pair == pair {
                return StrategyState::SignalDetected;
            }
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

            self.pending_signal = Some(PendingSignal {
                market_id: feature.market_id.clone(),
                pair: feature.pair.clone(),
                side,
                trigger_time: Instant::now(),
                trigger_obi: feature.obi_l1,
                trigger_price: feature.mid_price,
            });

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
        let signal = match &self.pending_signal {
            Some(s) if s.pair == feature.pair => s.clone(),
            _ => return Ok(()), // Not relevant for this pair
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
            self.pending_signal = None;
            return Ok(());
        }

        if feature.spread_bps > self.config.max_spread_bps {
            debug!(
                "Signal rejected: spread too wide {:.1} bps",
                feature.spread_bps
            );
            self.pending_signal = None;
            return Ok(());
        }

        // Calculate size
        let order_size = self.calculate_order_size(feature);
        if order_size < self.config.min_order_notional {
            debug!("Signal rejected: size {:.2} < min", order_size);
            self.pending_signal = None;
            return Ok(());
        }

        // Execute
        let entry_price = match signal.side {
            TradeSide::Buy => feature.best_ask,
            TradeSide::Sell => feature.best_bid,
        };
        let quantity = order_size / entry_price;

        if self.paper_mode {
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
                format!("paper-{}", Instant::now().elapsed().as_nanos()),
            );
            self.positions.insert(feature.pair.clone(), pos);
            self.pending_signal = None;
        } else if let Some(gw) = gateway {
            if let Some(token_id_str) = self.token_map.get(&feature.pair) {
                use std::str::FromStr;
                let token_id = U256::from_str(token_id_str).unwrap_or(U256::ZERO);

                let order_side = match signal.side {
                    TradeSide::Buy => OrderSide::Buy,
                    TradeSide::Sell => OrderSide::Sell,
                };

                let intent = OrderIntent::new(
                    feature.market_id.clone(),
                    token_id,
                    order_side,
                    entry_price,
                    alloy_primitives::U256::from((quantity * 1e18) as u128),
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
            self.pending_signal = None;
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
                // Submit exit order
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

                        let intent = OrderIntent::new(
                            feature.market_id.clone(),
                            token_id,
                            exit_side,
                            exit_price,
                            alloy_primitives::U256::from((position.quantity * 1e18) as u128),
                        )
                        .with_priority(OrderPriority::Critical); // Exits are critical

                        match gw.submit_order(intent, None).await {
                            Ok(id) => {
                                info!("Submitted exit order: {} reason={:?}", id, reason);
                                // Mark as exit pending or remove?
                                // For simplicity, remove position now, assuming fill.
                                // In real HFT, we wait for fill.
                                // But here we just assume submission means we are 'exiting'.
                                // Or we should transition state.
                                self.positions.remove(&feature.pair);
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

        // Initial simplified check without complex OFI history
        let ofi_ok = true;

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
}
